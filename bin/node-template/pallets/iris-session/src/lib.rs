//! # Iris Session Pallet
//!
//! @author driemworks
//! 
//! ## Description 
//! 
//! validators and storage providers
//! are treated as seprate roles, where you must first be a validator after which you can 
//! request to join a storage pool for some asset id (become a storage provider). If the ipfs
//! node has sufficient storage capacity to successfully pin the underlying CID of the asset class,
//! then that node is considered a storage provider as long as it is online.
//! 
//! The Iris Session Pallet allows addition and removal of
//! storage providers via extrinsics (transaction calls), in
//! Substrate-based PoA networks. It also integrates with the im-online pallet
//! to automatically remove offline storage providers.
//!
//! The pallet uses the Session pallet and implements related traits for session
//! management. Currently it uses periodic session rotation provided by the
//! session pallet to automatically rotate sessions. For this reason, the
//! validator addition and removal becomes effective only after 2 sessions
//! (queuing + applying).
//! 
//! 

#![cfg_attr(not(feature = "std"), no_std)]

mod mock;
mod tests;

use frame_support::{
	ensure,
	pallet_prelude::*,
	traits::{
		EstimateNextSessionRotation, Get,
		ValidatorSet, ValidatorSetWithIdentification,
	},
};
use log;
use scale_info::TypeInfo;
pub use pallet::*;
use sp_runtime::traits::{Convert, Zero};
use sp_staking::offence::{Offence, OffenceError, ReportOffence};
use sp_std::{
	collections::{ btree_set::BTreeSet, btree_map::BTreeMap },
	str,
	vec::Vec,
	prelude::*
};
use sp_core::{
    offchain::{
        Duration, IpfsRequest, IpfsResponse, OpaqueMultiaddr, Timestamp, StorageKind,
    },
	crypto::KeyTypeId,
    Bytes,
};
use frame_system::{
	self as system, 
	ensure_signed,
	offchain::{
		SendSignedTransaction,
		Signer,
		SubmitTransaction,
	}
};
use sp_io::offchain::timestamp;
use sp_runtime::{
	offchain::ipfs,
	traits::StaticLookup,
};
use pallet_iris_assets::{
	DataCommand,
};

pub const LOG_TARGET: &'static str = "runtime::iris-session";
// TODO: should a new KeyTypeId be defined? e.g. b"iris"
pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"aura");

pub mod crypto {
	use crate::KEY_TYPE;
	use sp_core::sr25519::Signature as Sr25519Signature;
	use sp_runtime::app_crypto::{app_crypto, sr25519};
	use sp_runtime::{traits::Verify, MultiSignature, MultiSigner};

	app_crypto!(sr25519, KEY_TYPE);

	pub struct TestAuthId;
	// implemented for runtime
	impl frame_system::offchain::AppCrypto<MultiSigner, MultiSignature> for TestAuthId {
		type RuntimeAppPublic = Public;
		type GenericSignature = sp_core::sr25519::Signature;
		type GenericPublic = sp_core::sr25519::Public;
	}

	// implemented for mock runtime in test
	impl frame_system::offchain::AppCrypto<<Sr25519Signature as Verify>::Signer, Sr25519Signature>
		for TestAuthId
	{
		type RuntimeAppPublic = Public;
		type GenericSignature = sp_core::sr25519::Signature;
		type GenericPublic = sp_core::sr25519::Public;
	}
}

/// Counter for the number of eras that have passed.
pub type EraIndex = u32;
/// counter for the number of "reward" points earned by a given storage provider
pub type RewardPoint = u32;

/// Reward points for storage providers of some specific assest id during an era.
#[derive(PartialEq, Encode, Decode, Default, RuntimeDebug, TypeInfo)]
pub struct EraRewardPoints<AccountId> {
	/// the total number of points
	total: RewardPoint,
	/// the reward points for individual validators, sum(i.rewardPoint in individual) = total
	individual: BTreeMap<AccountId, RewardPoint>,
}

/// Information regarding the active era (era in used in session).
#[derive(Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct ActiveEraInfo {
	/// Index of era.
	pub index: EraIndex,
	/// Moment of start expressed as millisecond from `$UNIX_EPOCH`.
	///
	/// Start can be none if start hasn't been set for the era yet,
	/// Start is set on the first on_finalize of the era to guarantee usage of `Time`.
	start: Option<u64>,
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_system::{
		pallet_prelude::*,
		offchain::{
			AppCrypto,
			CreateSignedTransaction,
		}
	};

	/// Configure the pallet by specifying the parameters and types on which it
	/// depends.
	#[pallet::config]
	pub trait Config: CreateSignedTransaction<Call<Self>> + 
					  frame_system::Config +
					  pallet_session::Config +
					  pallet_iris_assets::Config 
	{
		/// The Event type.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		/// the overarching call type
		type Call: From<Call<Self>>;
		/// Origin for adding or removing a validator.
		type AddRemoveOrigin: EnsureOrigin<Self::Origin>;
		/// Minimum number of validators to leave in the validator set during
		/// auto removal.
		type MinAuthorities: Get<u32>;
		/// the maximum number of session that a node can earn less than MinEraRewardPoints before suspension
		type MaxDeadSession: Get<u32>;
		/// the authority id used for sending signed txs
        type AuthorityId: AppCrypto<Self::Public, Self::Signature>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

    /// map the ipfs public key to a list of multiaddresses
    #[pallet::storage]
    #[pallet::getter(fn bootstrap_nodes)]
    pub(super) type BootstrapNodes<T: Config> = StorageMap<
        _, Blake2_128Concat, Vec<u8>, Vec<OpaqueMultiaddr>, ValueQuery,
    >;

	/// map substrate public key to ipfs public key
	#[pallet::storage]
	#[pallet::getter(fn substrate_ipfs_bridge)]
	pub(super) type SubstrateIpfsBridge<T: Config> = StorageMap<
		_, Blake2_128Concat, T::AccountId, Vec<u8>, ValueQuery,
	>;

	/// Maps an asset id to a collection of nodes that want to provider storage
	#[pallet::storage]
	#[pallet::getter(fn candidate_storage_providers)]
	pub(super) type QueuedStorageProviders<T: Config> = StorageMap<
		_, Blake2_128Concat, T::AssetId, Vec<T::AccountId>, ValueQuery,
	>;

	/// maps an asset id to a collection of nodes that are providing storage
	#[pallet::storage]
	#[pallet::getter(fn storage_providers)]
	pub(super) type StorageProviders<T: Config> = StorageMap<
		_, Blake2_128Concat, T::AssetId, Vec<T::AccountId>, ValueQuery,
	>;

	/// maps an asset id to a collection of nodes that have inserted the pin for the underlying cid
	#[pallet::storage]
	#[pallet::getter(fn pinners)]
	pub(super) type Pinners<T: Config> = StorageMap<
		_, Blake2_128Concat, T::AssetId, Vec<T::AccountId>, ValueQuery,
	>;

	/// The current era index.
	///
	/// This is the latest planned era, depending on how the Session pallet queues the validator
	/// set, it might be active or not.
	#[pallet::storage]
	#[pallet::getter(fn current_era)]
	pub type CurrentEra<T> = StorageValue<
		_, EraIndex
	>;

	/// The active era information, it holds index and start.
	///
	/// The active era is the era being currently rewarded. Validator set of this era must be
	/// equal to [`SessionInterface::validators`].
	#[pallet::storage]
	#[pallet::getter(fn active_era)]
	// TODO: Do I need the ActiveEraInfo?
	pub type ActiveEra<T> = StorageValue<
		_, EraIndex
	>;
	
	/// Rewards for the last `HISTORY_DEPTH` eras.
	/// If reward hasn't been set or has been removed then 0 reward is returned.
	#[pallet::storage]
	#[pallet::getter(fn eras_reward_points)]
	pub type ErasRewardPoints<T: Config> = StorageDoubleMap<
		_, Blake2_128Concat, EraIndex, Blake2_128Concat, T::AssetId, EraRewardPoints<T::AccountId>, ValueQuery,
	>;

	///
	/// 
	#[pallet::storage]
	#[pallet::getter(fn validators)]
	pub type Validators<T: Config> = StorageValue<
		_, Vec<T::AccountId>, ValueQuery>;

	///
	/// 
	#[pallet::storage]
	#[pallet::getter(fn approved_validators)]
	pub type ApprovedValidators<T: Config> = StorageValue<
		_, Vec<T::AccountId>, ValueQuery>;

	///
	/// 
	#[pallet::storage]
	#[pallet::getter(fn validators_to_remove)]
	pub type OfflineValidators<T: Config> = StorageValue<
		_, Vec<T::AccountId>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn total_session_rewards)]
	pub type SessionParticipation<T: Config> = StorageMap<
		_, Blake2_128Concat, EraIndex, Vec<T::AccountId>, ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn unproductive_sessions)]
	pub type UnproductiveSessions<T: Config> = StorageMap<
		_, Blake2_128Concat, T::AccountId, u32, ValueQuery,
	>;

	///
	/// 
	// #[pallet::storage]
	// #[pallet::getter(fn dead_validator)]
	// pub type DeadValidators<T: Config> = StorageMap<
	// 	_, Blake2_128Concat, u32, Vec<T::AccountId>, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// New validator addition initiated. Effective in ~2 sessions.
		ValidatorAdditionInitiated(T::AccountId),
		/// Validator removal initiated. Effective in ~2 sessions.
		ValidatorRemovalInitiated(T::AccountId),
		/// Validator published their ipfs public key and maddrs
		PublishedIdentity(T::AccountId),
		/// A validator requested to join a storage pool
		RequestJoinStoragePoolSuccess(T::AccountId, T::AssetId),
	}

	
	#[pallet::validate_unsigned]
	impl<T: Config> ValidateUnsigned for Pallet<T> {
		type Call = Call<T>;

		/// Validate unsigned call to this module.
		///
		fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
			if let Call::submit_rpc_ready { asset_id } = call {
				Self::validate_transaction_parameters()
			} else if let Call::submit_ipfs_identity{ public_key, multiaddresses } = call {
				Self::validate_transaction_parameters()
			} else {
				InvalidTransaction::Call.into()
			}
		}
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Target (post-removal) validator count is below the minimum.
		TooLowValidatorCount,
		/// Validator is already in the validator set.
		Duplicate,
		/// Validator is not approved for re-addition.
		ValidatorNotApproved,
		/// Only the validator can add itself back after coming online.
		BadOrigin,
		/// could not build the ipfs request
		CantCreateRequest,
		/// the request to IPFS timed out
		RequestTimeout,
		/// the request to IPFS failed
		RequestFailed,
		/// the specified asset id does not correspond to any owned content
		NoSuchOwnedContent,
		/// the nodes balance is insufficient to complete this operation
		InsufficientBalance,
		/// the node is already a candidate for some storage pool
		AlreadyACandidate,
		/// the node has already pinned the CID
		AlreadyPinned,
		/// the node is not a candidate storage provider for some asset id
		NotACandidate,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: T::BlockNumber) {
			// every 5 blocks
			if block_number % 5u32.into() == 0u32.into() {
				if let Err(e) = Self::connection_housekeeping() {
					log::error!("IPFS: Encountered an error while processing data requests: {:?}", e);
				}
			}
			// handle data requests each block
			if let Err(e) = Self::handle_data_requests() {
				log::error!("IPFS: Encountered an error while processing data requests: {:?}", e);
			}

			// every 5 blocks
			if block_number % 5u32.into() == 0u32.into() {
				if let Err(e) = Self::print_metadata() {
					log::error!("IPFS: Encountered an error while obtaining metadata: {:?}", e);
				}
			}
		}
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		pub initial_validators: Vec<T::AccountId>,
	}

	#[cfg(feature = "std")]
	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self { initial_validators: Default::default() }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> GenesisBuild<T> for GenesisConfig<T> {
		fn build(&self) {
			Pallet::<T>::initialize_validators(&self.initial_validators);
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Add a new validator.
		///
		/// New validator's session keys should be set in Session pallet before
		/// calling this.
		///
		/// The origin can be configured using the `AddRemoveOrigin` type in the
		/// host runtime. Can also be set to sudo/root.
		///
		#[pallet::weight(100)]
		pub fn add_validator(origin: OriginFor<T>, validator_id: T::AccountId) -> DispatchResult {
			T::AddRemoveOrigin::ensure_origin(origin)?;
			Self::do_add_validator(validator_id.clone())?;
			Self::approve_validator(validator_id)?;
			Ok(())
		}

		/// Remove a validator.
		///
		/// The origin can be configured using the `AddRemoveOrigin` type in the
		/// host runtime. Can also be set to sudo/root.
		#[pallet::weight(100)]
		pub fn remove_validator(
			origin: OriginFor<T>,
			validator_id: T::AccountId,
		) -> DispatchResult {
			T::AddRemoveOrigin::ensure_origin(origin)?;
			Self::do_remove_validator(validator_id.clone())?;
			Self::unapprove_validator(validator_id)?;
			Ok(())
		}

		/// Add an approved validator again when it comes back online.
		///
		/// For this call, the dispatch origin must be the validator itself.
		#[pallet::weight(100)]
		pub fn add_validator_again(
			origin: OriginFor<T>,
			validator_id: T::AccountId,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			ensure!(who == validator_id, Error::<T>::BadOrigin);

			let approved_set: BTreeSet<_> = <ApprovedValidators<T>>::get().into_iter().collect();
			ensure!(approved_set.contains(&validator_id), Error::<T>::ValidatorNotApproved);

			Self::do_add_validator(validator_id)?;

			Ok(())
		}

		#[pallet::weight(100)]
		pub fn join_storage_pool(
			origin: OriginFor<T>,
			pool_owner: <T::Lookup as StaticLookup>::Source,
			pool_id: T::AssetId,
		) -> DispatchResult {
			// submit a request to join a storage pool in the next session
			let who = ensure_signed(origin)?;
			let new_origin = system::RawOrigin::Signed(who.clone()).into();
			let candidate_storage_providers = <QueuedStorageProviders::<T>>::get(pool_id.clone());
			ensure!(!candidate_storage_providers.contains(&who), Error::<T>::AlreadyACandidate);
			<QueuedStorageProviders<T>>::mutate(pool_id.clone(), |sp| {
				sp.push(who.clone());
			});
			let owner = T::Lookup::lookup(pool_owner)?;
			<pallet_iris_assets::Pallet<T>>::insert_pin_request(new_origin, owner, pool_id).map_err(|_| Error::<T>::CantCreateRequest);
			Self::deposit_event(Event::RequestJoinStoragePoolSuccess(who.clone(), pool_id.clone()));
			Ok(())
		}

		/// TODO: I really need to address the fact that this is callable by anyone
		/// Someone could randomly make an asset class on your behalf, making you the admin
		/// 
		/// should only be called by offchain workers... how to ensure this?
        /// submits IPFS results on chain and creates new ticket config in runtime storage
        ///
        /// * `admin`: The admin account
        /// * `cid`: The cid generated by the OCW
        /// * `id`: The AssetId (passed through from the create_storage_asset call)
        /// * `balance`: The balance (passed through from the create_storage_asset call)
        ///
        #[pallet::weight(100)]
        pub fn submit_ipfs_add_results(
            origin: OriginFor<T>,
            admin: <T::Lookup as StaticLookup>::Source,
            cid: Vec<u8>,
            id: T::AssetId,
            balance: T::Balance,
        ) -> DispatchResult {
			// TODO: explore consequences of changing to ensure_root
			let who = ensure_signed(origin)?;
			let new_origin = system::RawOrigin::Signed(who.clone()).into();
			// creates the asset class
            <pallet_iris_assets::Pallet<T>>::submit_ipfs_add_results(
				new_origin,
				admin,
				cid,
				id,
				balance,
			)?;
			// award point to all validators
			if let Some(active_era) = ActiveEra::<T>::get() {
				<ErasRewardPoints<T>>::mutate(active_era.clone(), id, |era_rewards| {
					// reward all validators
					for v in <Validators::<T>>::get() {
						SessionParticipation::<T>::mutate(active_era.clone(), |participants| {
							participants.push(v.clone());
						});
						*era_rewards.individual.entry(v.clone()).or_default() += 1;
						era_rewards.total += 1;
					}
				});
			} else {
				// error
			}
            Ok(())
        }

        /// Should only be callable by OCWs (TODO)
        /// Submit the results of an `ipfs identity` call to be stored on chain
        ///
        /// * origin: a validator node
        /// * public_key: The IPFS node's public key
        /// * multiaddresses: A vector of multiaddresses associate with the public key
        ///
        #[pallet::weight(100)]
        pub fn submit_ipfs_identity(
            origin: OriginFor<T>,
            public_key: Vec<u8>,
            multiaddresses: Vec<OpaqueMultiaddr>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            <BootstrapNodes::<T>>::insert(public_key.clone(), multiaddresses.clone());
            <SubstrateIpfsBridge::<T>>::insert(who.clone(), public_key.clone());
			Self::deposit_event(Event::PublishedIdentity(who.clone()));
            Ok(())
        }

		/// should only be callable by validator nodes (TODO)
		/// 
		/// * `asset_id`: The asset id corresponding to the data that was pinned
		/// * `pinner': The node claiming to have pinned the data
		/// 
		#[pallet::weight(100)]
		pub fn submit_ipfs_pin_result(
			origin: OriginFor<T>,
			asset_id: T::AssetId,
			pinner: T::AccountId,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			// verify they are a candidate storage provider
			let candidate_storage_providers = <QueuedStorageProviders::<T>>::get(asset_id.clone());
			ensure!(candidate_storage_providers.contains(&pinner), Error::<T>::NotACandidate);
			// verify not already pinning the content
			let current_pinners = <Pinners::<T>>::get(asset_id.clone());
			ensure!(!current_pinners.contains(&pinner), Error::<T>::AlreadyPinned);
			// TODO: we need a better scheme for *generating* pool ids -> should always be unique (cid + owner maybe?)
			<Pinners<T>>::mutate(asset_id.clone(), |p| {
				p.push(pinner.clone());
			});
			// award point to pinner
			if let Some(active_era) = ActiveEra::<T>::get() {
				SessionParticipation::<T>::mutate(active_era.clone(), |p| {
					p.push(pinner.clone());
				});
				<ErasRewardPoints<T>>::mutate(active_era, asset_id, |era_rewards| {
					*era_rewards.individual.entry(pinner.clone()).or_default() += 1;
					era_rewards.total += 1;
				});
			}
			Ok(())
		}

        /// Should only be callable by OCWs (TODO)
        /// Submit the results onchain to notify a beneficiary that their data is available: TODO: how to safely share host? spam protection on rpc endpoints?
        ///
        /// * `beneficiary`: The account that requested the data
        /// * `host`: The node's host where the data has been made available (RPC endpoint)
        ///
        #[pallet::weight(100)]
        pub fn submit_rpc_ready(
            origin: OriginFor<T>,
			asset_id: T::AssetId,
        ) -> DispatchResult {
            // ensure_signed(origin)?;
			if let Some(active_era) = ActiveEra::<T>::get() {
				<ErasRewardPoints<T>>::mutate(active_era.clone(), asset_id.clone(), |era_rewards| {
					// reward all active storage providers
					for k in StorageProviders::<T>::get(asset_id.clone()).into_iter() {
						SessionParticipation::<T>::mutate(active_era.clone(), |p| {
							p.push(k.clone());
						});
						*era_rewards.individual.entry(k.clone()).or_default() += 1;
						era_rewards.total += 1;
					}
				});
			} else {
				// error -> no active era found
			}
            // Self::deposit_event(Event::DataReady(beneficiary));
            Ok(())
        }
	}
}

impl<T: Config> Pallet<T> {

	fn initialize_validators(validators: &[T::AccountId]) {
		assert!(validators.len() > 1, "At least 2 validators should be initialized");
		assert!(<Validators<T>>::get().is_empty(), "Validators are already initialized!");
		<Validators<T>>::put(validators);
		<ApprovedValidators<T>>::put(validators);
	}

	fn do_add_validator(validator_id: T::AccountId) -> DispatchResult {
		let validator_set: BTreeSet<_> = <Validators<T>>::get().into_iter().collect();
		ensure!(!validator_set.contains(&validator_id), Error::<T>::Duplicate);
		<Validators<T>>::mutate(|v| v.push(validator_id.clone()));
		UnproductiveSessions::<T>::mutate(validator_id.clone(), |v| {
			*v = 0;
		});

		Self::deposit_event(Event::ValidatorAdditionInitiated(validator_id.clone()));
		log::debug!(target: LOG_TARGET, "Validator addition initiated.");

		Ok(())
	}

	fn do_remove_validator(validator_id: T::AccountId) -> DispatchResult {
		let mut validators = <Validators<T>>::get();

		// Ensuring that the post removal, target validator count doesn't go
		// below the minimum.
		ensure!(
			validators.len().saturating_sub(1) as u32 >= T::MinAuthorities::get(),
			Error::<T>::TooLowValidatorCount
		);

		validators.retain(|v| *v != validator_id);

		<Validators<T>>::put(validators);

		Self::deposit_event(Event::ValidatorRemovalInitiated(validator_id.clone()));
		log::debug!(target: LOG_TARGET, "Validator removal initiated.");

		Ok(())
	}

	/// Ensure the candidate validator is eligible to be a validator
	fn approve_validator(validator_id: T::AccountId) -> DispatchResult {
		let approved_set: BTreeSet<_> = <ApprovedValidators<T>>::get().into_iter().collect();
		ensure!(!approved_set.contains(&validator_id), Error::<T>::Duplicate);
		<ApprovedValidators<T>>::mutate(|v| v.push(validator_id.clone()));
		Ok(())
	}

	/// Remote a validator from the list of approved validators
	fn unapprove_validator(validator_id: T::AccountId) -> DispatchResult {
		let mut approved_set = <ApprovedValidators<T>>::get();
		approved_set.retain(|v| *v != validator_id);
		Ok(())
	}

	// Adds offline validators to a local cache for removal at new session.
	fn mark_for_removal(validator_id: T::AccountId) {
		<OfflineValidators<T>>::mutate(|v| v.push(validator_id));
		log::debug!(target: LOG_TARGET, "Offline validator marked for auto removal.");
	}

	// Removes offline validators from the validator set and clears the offline
	// cache. It is called in the session change hook and removes the validators
	// who were reported offline during the session that is ending. We do not
	// check for `MinAuthorities` here, because the offline validators will not
	// produce blocks and will have the same overall effect on the runtime.
	fn remove_offline_validators() {
		let validators_to_remove: BTreeSet<_> = <OfflineValidators<T>>::get().into_iter().collect();

		// Delete from active validator set.
		<Validators<T>>::mutate(|vs| vs.retain(|v| !validators_to_remove.contains(v))); 
		log::debug!(
			target: LOG_TARGET,
			"Initiated removal of {:?} offline validators.",
			validators_to_remove.len()
		);

		// remove as storage provider
		// remove from pinners (and unpin the cid)
		// Clear the offline validator list to avoid repeated deletion.
		<OfflineValidators<T>>::put(Vec::<T::AccountId>::new());
	}

	/// move candidates to the active provider pool for some asset id
	/// TODO: this undoubtedly will not scale very well 
	fn select_candidate_storage_providers() {
		// if there are candidate storage providers => for each candidate that pinned the file, move them to storage providers
		for assetid in <pallet_iris_assets::Pallet<T>>::asset_ids().into_iter() {
			// if there are candidates for the asset id
			if <QueuedStorageProviders<T>>::contains_key(assetid.clone()) {
				let candidates = <QueuedStorageProviders<T>>::get(assetid.clone());
				let pinners = <Pinners<T>>::get(assetid.clone());
				let pinner_candidate_intersection = 
					candidates.into_iter().filter(|c| pinners.contains(c)).collect::<Vec<T::AccountId>>();
				log::info!("Adding {:?} more storage providers", pinner_candidate_intersection.len());
				// need to only move candidates that have actually proved they pinned the content
				<StorageProviders::<T>>::insert(assetid.clone(), pinner_candidate_intersection);
			} 
		}
	}

	fn mark_dead_validators(era_index: EraIndex) {
		// for each validator that didn't participate, mark for removal
		let partipating_validators = SessionParticipation::<T>::get(era_index.clone());
		for acct in Validators::<T>::get() {
			if !partipating_validators.contains(&acct) {
				if UnproductiveSessions::<T>::get(acct.clone()) <= T::MaxDeadSession::get() {
					UnproductiveSessions::<T>::mutate(acct.clone(), |v| {
						*v += 1;
					});
				} else {
					// TODO
					Self::do_remove_validator(acct);
				}
			}
		}
	}

	fn validate_transaction_parameters() -> TransactionValidity {
		ValidTransaction::with_tag_prefix("rpc_ready")
			.longevity(5)
			.propagate(true)
			.build()
	}

	/// implementation for RPC runtime API to retrieve bytes from the node's local storage
    /// 
    /// * public_key: The account's public key as bytes
    /// * signature: The signer's signature as bytes
    /// * message: The signed message as bytes
    ///
    pub fn retrieve_bytes(
		message: Bytes,
    ) -> Bytes {
        let message_vec: Vec<u8> = message.to_vec();
		if let Some(data) = sp_io::offchain::local_storage_get(StorageKind::PERSISTENT, &message_vec) {
			Bytes(data.clone())
		} else {
			Bytes(Vec::new())
		}
    }
	
	 /// send a request to the local IPFS node; can only be called be an off-chain worker
	 fn ipfs_request(
        req: IpfsRequest,
        deadline: impl Into<Option<Timestamp>>,
    ) -> Result<IpfsResponse, Error<T>> {
        let ipfs_request = ipfs::PendingRequest::new(req)
			.map_err(|_| Error::<T>::CantCreateRequest)?;
        ipfs_request.try_wait(deadline)
            .map_err(|_| Error::<T>::RequestTimeout)?
            .map(|r| r.response)
            .map_err(|e| {
                if let ipfs::Error::IoError(err) = e {
                    log::error!("IPFS: request failed: {}", str::from_utf8(&err).unwrap());
                } else {
                    log::error!("IPFS: request failed: {:?}", e);
                }
                Error::<T>::RequestFailed
            })
    }
	
	/// manage connection to the iris ipfs swarm
    ///
    /// If the node is already a bootstrap node, do nothing. Otherwise submits a signed tx 
    /// containing the public key and multiaddresses of the embedded ipfs node.
    /// 
    /// Returns an error if communication with the embedded IPFS fails
    fn connection_housekeeping() -> Result<(), Error<T>> {
        let deadline = Some(timestamp().add(Duration::from_millis(5_000)));
        
        let (public_key, addrs) = 
			if let IpfsResponse::Identity(public_key, addrs) = 
				Self::ipfs_request(IpfsRequest::Identity, deadline)? {
            (public_key, addrs)
        } else {
            unreachable!("only `Identity` is a valid response type.");
        };

        if !BootstrapNodes::<T>::contains_key(public_key.clone()) {
            if let Some(bootstrap_node) = &BootstrapNodes::<T>::iter().nth(0) {
                if let Some(bootnode_maddr) = bootstrap_node.1.clone().pop() {
                    if let IpfsResponse::Success = Self::ipfs_request(IpfsRequest::Connect(bootnode_maddr.clone()), deadline)? {
                        log::info!("Succesfully connected to a bootstrap node: {:?}", &bootnode_maddr.0);
                    } else {
                        log::info!("Failed to connect to the bootstrap node with multiaddress: {:?}", &bootnode_maddr.0);
                        // TODO: this should probably be some recursive function? but we should never exceed a depth of 2 so maybe not
                        if let Some(next_bootnode_maddr) = bootstrap_node.1.clone().pop() {
                            if let IpfsResponse::Success 
								= Self::ipfs_request(IpfsRequest::Connect(next_bootnode_maddr.clone()), deadline)? {
                                log::info!("Succesfully connected to a bootstrap node: {:?}", &next_bootnode_maddr.0);
                            } else {
                                log::info!("Failed to connect to the bootstrap node with multiaddress: {:?}", &next_bootnode_maddr.0);
                            }       
                        }
                    }
                }
            }
			
            let signer = Signer::<T, T::AuthorityId>::all_accounts();
            if !signer.can_sign() {
                log::error!(
                    "No local accounts available. Consider adding one via `author_insertKey` RPC.",
                );
            }
             
            let results = signer.send_signed_transaction(|_account| { 
                Call::submit_ipfs_identity {
                    public_key: public_key.clone(),
                    multiaddresses: addrs.clone(),
                }
            });
    
            for (_, res) in &results {
                match res {
                    Ok(()) => log::info!("Submitted ipfs identity results"),
                    Err(e) => log::error!("Failed to submit transaction: {:?}",  e),
                }
            }
        }
        Ok(())
    }

	/// process any requests in the DataQueue
	/// TODO: This needs some *major* refactoring
    fn handle_data_requests() -> Result<(), Error<T>> {
		let data_queue = <pallet_iris_assets::Pallet<T>>::data_queue();
		let len = data_queue.len();
		if len != 0 {
			log::info!("IPFS: {} entr{} in the data queue", len, if len == 1 { "y" } else { "ies" });
		}
		// TODO: Needs refactoring
		let deadline = Some(timestamp().add(Duration::from_millis(5_000)));
		for cmd in data_queue.into_iter() {
			match cmd {
				DataCommand::AddBytes(addr, cid, admin, _name, id, balance) => {
					if sp_io::offchain::is_validator() {
						Self::ipfs_request(IpfsRequest::Connect(addr.clone()), deadline)?;
						log::info!(
							"IPFS: connected to {}",
							str::from_utf8(&addr.0).expect("our own calls can be trusted to be UTF-8; qed")
						);
						match Self::ipfs_request(IpfsRequest::CatBytes(cid.clone()), deadline) {
							Ok(IpfsResponse::CatBytes(data)) => {
								log::info!("IPFS: fetched data");
								Self::ipfs_request(IpfsRequest::Disconnect(addr.clone()), deadline)?;
								log::info!(
									"IPFS: disconnected from {}",
									str::from_utf8(&addr.0).expect("our own calls can be trusted to be UTF-8; qed")
								);
								match Self::ipfs_request(IpfsRequest::AddBytes(data.clone()), deadline) {
									Ok(IpfsResponse::AddBytes(new_cid)) => {
										log::info!(
											"IPFS: added data with Cid {}",
											str::from_utf8(&new_cid).expect("our own IPFS node can be trusted here; qed")
										);
										let signer = Signer::<T, T::AuthorityId>::all_accounts();
										if !signer.can_sign() {
											log::error!(
												"No local accounts available. Consider adding one via `author_insertKey` RPC.",
											);
										}
										let results = signer.send_signed_transaction(|_account| { 
											Call::submit_ipfs_add_results{
												admin: admin.clone(),
												cid: new_cid.clone(),
												id: id.clone(),
												balance: balance.clone(),
											}
										});
								
										for (_, res) in &results {
											match res {
												Ok(()) => log::info!("Submitted ipfs results"),
												Err(e) => log::error!("Failed to submit transaction: {:?}",  e),
											}
										}
									},
									Ok(_) => unreachable!("only AddBytes can be a response for that request type."),
									Err(e) => log::error!("IPFS: add error: {:?}", e),
								}
							},
							Ok(_) => unreachable!("only CatBytes can be a response for that request type."),
							Err(e) => log::error!("IPFS: cat error: {:?}", e),
						}
					}
				},
				DataCommand::CatBytes(requestor, owner, asset_id) => {
					// fetch ipfs id
					let public_key = 
						if let IpfsResponse::Identity(public_key, addrs) = 
							Self::ipfs_request(IpfsRequest::Identity, deadline)? {
						public_key
					} else {
						unreachable!("only `Identity` is a valid response type.");
					};
					// verify ipfs pub key
					let expected_pub_key = <SubstrateIpfsBridge::<T>>::get(requestor.clone());
					ensure!(public_key == expected_pub_key, Error::<T>::BadOrigin);

					if let cid = <pallet_iris_assets::Pallet<T>>::asset_class_ownership(
						owner.clone(), asset_id.clone()
					) {	
						ensure!(
							owner.clone() == <pallet_iris_assets::Pallet<T>>::asset_access(requestor.clone(), asset_id.clone()),
							Error::<T>::InsufficientBalance
						);
						match Self::ipfs_request(IpfsRequest::CatBytes(cid.clone()), deadline) {
							Ok(IpfsResponse::CatBytes(data)) => {
								log::info!("IPFS: Fetched data from IPFS.");
								// add to offchain index
								sp_io::offchain::local_storage_set(
									StorageKind::PERSISTENT,
									&cid,
									&data,
								);
								let call = Call::submit_rpc_ready {
									asset_id: asset_id.clone(),
								};
								SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(call.into())
									.map_err(|()| Error::<T>::CantCreateRequest)?;
							},
							Ok(_) => unreachable!("only CatBytes can be a response for that request type."),
							Err(e) => log::error!("IPFS: cat error: {:?}", e),
						}
					} else {
						log::error!("the provided owner/cid does not map to a valid asset id: {:?}, {:?}", owner, asset_id)
					}
				},
				DataCommand::PinCID(acct, asset_id, cid) => {
					if sp_io::offchain::is_validator() {
						let (public_key, addrs) = 
							if let IpfsResponse::Identity(public_key, addrs) = 
								Self::ipfs_request(IpfsRequest::Identity, deadline)? {
							(public_key, addrs)
						} else {
							unreachable!("only `Identity` is a valid response type.");
						};
						let expected_pub_key = <SubstrateIpfsBridge::<T>>::get(acct.clone());
						ensure!(public_key == expected_pub_key, Error::<T>::BadOrigin);
						match Self::ipfs_request(IpfsRequest::InsertPin(cid.clone(), false), deadline) {
							Ok(IpfsResponse::Success) => {
								log::info!("IPFS: Pinned CID {:?}", cid.clone());
								let signer = Signer::<T, T::AuthorityId>::all_accounts();
								if !signer.can_sign() {
									log::error!(
										"No local accounts available. Consider adding one via `author_insertKey` RPC.",
									);
								}
								let results = signer.send_signed_transaction(|_account| { 
									Call::submit_ipfs_pin_result{
										asset_id: asset_id,
										pinner: acct.clone(),
									}
								});
						
								for (_, res) in &results {
									match res {
										Ok(()) => log::info!("Submitted ipfs results"),
										Err(e) => log::error!("Failed to submit transaction: {:?}",  e),
									}
								}
							},
							Ok(_) => unreachable!("only Success can be a response for that request type"),
							Err(e) => log::error!("IPFS: insert pin error: {:?}", e),
						}
					}
				}
			}
		}

        Ok(())
    }
    
    fn print_metadata() -> Result<(), Error<T>> {
        let deadline = Some(timestamp().add(Duration::from_millis(5_000)));

        let peers = if let IpfsResponse::Peers(peers) = Self::ipfs_request(IpfsRequest::Peers, deadline)? {
            peers
        } else {
            unreachable!("only Peers can be a response for that request type; qed");
        };
        let peer_count = peers.len();

        log::info!(
            "IPFS: currently connected to {} peer{}",
            peer_count,
            if peer_count == 1 { "" } else { "s" },
        );
        Ok(())
    }
}

// Provides the new set of validators to the session module when session is
// being rotated.
impl<T: Config> pallet_session::SessionManager<T::AccountId> for Pallet<T> {
	// Plan a new session and provide new validator set.
	fn new_session(new_index: u32) -> Option<Vec<T::AccountId>> {
		log::info!("Starting new session with index: {:?}", new_index);
		// TODO: how staking pallet uses this, 'trigger_new_era'
		CurrentEra::<T>::mutate(|s| *s = Some(new_index));
		Self::remove_offline_validators();
		// TODO: REMOVE OFFLINE STORAGE PROVIDERS
		Self::select_candidate_storage_providers();
		log::debug!(target: LOG_TARGET, "New session called; updated validator set provided.");
		Some(Self::validators())
	}

	fn end_session(end_index: u32) {
		log::info!("Ending session with index: {:?}", end_index);
		Self::mark_dead_validators(end_index);
	}

	fn start_session(start_index: u32) {
		log::info!("Starting session with index: {:?}", start_index);
		ActiveEra::<T>::mutate(|s| *s = Some(start_index)); 
	}
}

impl<T: Config> EstimateNextSessionRotation<T::BlockNumber> for Pallet<T> {
	fn average_session_length() -> T::BlockNumber {
		Zero::zero()
	}

	fn estimate_current_session_progress(
		_now: T::BlockNumber,
	) -> (Option<sp_runtime::Permill>, frame_support::dispatch::Weight) {
		(None, Zero::zero())
	}

	fn estimate_next_session_rotation(
		_now: T::BlockNumber,
	) -> (Option<T::BlockNumber>, frame_support::dispatch::Weight) {
		(None, Zero::zero())
	}
}

// Implementation of Convert trait for mapping ValidatorId with AccountId.
pub struct ValidatorOf<T>(sp_std::marker::PhantomData<T>);

impl<T: Config> Convert<T::ValidatorId, Option<T::ValidatorId>> for ValidatorOf<T> {
	fn convert(account: T::ValidatorId) -> Option<T::ValidatorId> {
		Some(account)
	}
}

impl<T: Config> ValidatorSet<T::AccountId> for Pallet<T> {
	type ValidatorId = T::ValidatorId;
	type ValidatorIdOf = T::ValidatorIdOf;

	fn session_index() -> sp_staking::SessionIndex {
		pallet_session::Pallet::<T>::current_index()
	}

	fn validators() -> Vec<Self::ValidatorId> {
		pallet_session::Pallet::<T>::validators()
	}
}

impl<T: Config> ValidatorSetWithIdentification<T::AccountId> for Pallet<T> {
	type Identification = T::ValidatorId;
	type IdentificationOf = ValidatorOf<T>;
}

// Offence reporting and unresponsiveness management.
impl<T: Config, O: Offence<(T::AccountId, T::AccountId)>>
	ReportOffence<T::AccountId, (T::AccountId, T::AccountId), O> for Pallet<T>
{
	fn report_offence(_reporters: Vec<T::AccountId>, offence: O) -> Result<(), OffenceError> {
		let offenders = offence.offenders();

		for (v, _) in offenders.into_iter() {
			Self::mark_for_removal(v);
		}

		Ok(())
	}

	fn is_known_offence(
		_offenders: &[(T::AccountId, T::AccountId)],
		_time_slot: &O::TimeSlot,
	) -> bool {
		false
	}
}
