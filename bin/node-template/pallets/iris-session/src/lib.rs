//! # Iris Session Pallet
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

#![cfg_attr(not(feature = "std"), no_std)]

mod mock;
mod tests;

use frame_support::{
	ensure,
	pallet_prelude::*,
	traits::{
		EstimateNextSessionRotation, Get, ValidatorSet, ValidatorSetWithIdentification,
	},
};
use log;
pub use pallet::*;
use sp_runtime::traits::{Convert, Zero};
use sp_staking::offence::{Offence, OffenceError, ReportOffence};
use sp_std::{
	collections::btree_set::BTreeSet,
	convert::TryInto,
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
// pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"iris");
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
        _,
        Blake2_128Concat,
        Vec<u8>,
        Vec<OpaqueMultiaddr>,
        ValueQuery,
    >;

	/// map substrate public key to ipfs public key
	#[pallet::storage]
	#[pallet::getter(fn substrate_ipfs_bridge)]
	pub(super) type SubstrateIpfsBridge<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		Vec<u8>,
		ValueQuery,
	>;

	/// Maps an asset id to a collection of nodes that want to provider storage
	#[pallet::storage]
	#[pallet::getter(fn candidate_storage_providers)]
	pub(super) type CandidateStorageProviders<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AssetId,
		Vec<T::AccountId>,
		ValueQuery,
	>;

	/// maps an asset id to a collection of nodes that are providing storage
	#[pallet::storage]
	#[pallet::getter(fn storage_providers)]
	pub(super) type StorageProviders<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AssetId,
		Vec<T::AccountId>,
		ValueQuery,
	>;

	/// maps an asset id to a collection of nodes that have inserted the pin for the underlying cid
	#[pallet::storage]
	#[pallet::getter(fn pinners)]
	pub(super) type Pinners<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AssetId,
		Vec<T::AccountId>,
		ValueQuery,
	>;

	#[pallet::storage]
	#[pallet::getter(fn validators)]
	pub type Validators<T: Config> = StorageValue<_, Vec<T::AccountId>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn approved_validators)]
	pub type ApprovedValidators<T: Config> = StorageValue<_, Vec<T::AccountId>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn validators_to_remove)]
	pub type OfflineValidators<T: Config> = StorageValue<_, Vec<T::AccountId>, ValueQuery>;

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
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: T::BlockNumber) {
			if sp_io::offchain::is_validator() {
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
		#[pallet::weight(0)]
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
		#[pallet::weight(0)]
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
		#[pallet::weight(0)]
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

		#[pallet::weight(0)]
		pub fn request_join_storage_pool(
			origin: OriginFor<T>,
			pool_owner: <T::Lookup as StaticLookup>::Source,
			pool_id: T::AssetId,
		) -> DispatchResult {
			// submit a request to join a storage pool in the next session
			let who = ensure_signed(origin)?;
			let new_origin = system::RawOrigin::Signed(who.clone()).into();
			let candidate_storage_providers = <CandidateStorageProviders::<T>>::get(pool_id.clone());
			ensure!(!candidate_storage_providers.contains(&who), Error::<T>::AlreadyACandidate);
			// TODO: we need a better scheme for *generating* pool ids -> should always be unique (cid + owner maybe?)
			<CandidateStorageProviders<T>>::mutate(pool_id.clone(), |sp| {
				sp.push(who.clone());
			});
			let owner = T::Lookup::lookup(pool_owner)?;
			<pallet_iris_assets::Pallet<T>>::insert_pin_request(new_origin, owner, pool_id);
			Self::deposit_event(Event::RequestJoinStoragePoolSuccess(who.clone(), pool_id.clone()));
			Ok(())
		}

		        /// should only be called by offchain workers... how to ensure this?
        /// submits IPFS results on chain and creates new ticket config in runtime storage
        ///
        /// * `admin`: The admin account
        /// * `cid`: The cid generated by the OCW
        /// * `id`: The AssetId (passed through from the create_storage_asset call)
        /// * `balance`: The balance (passed through from the create_storage_asset call)
        ///
        #[pallet::weight(0)]
        pub fn submit_ipfs_add_results(
            origin: OriginFor<T>,
            admin: <T::Lookup as StaticLookup>::Source,
            cid: Vec<u8>,
            id: T::AssetId,
            balance: T::Balance,
        ) -> DispatchResult {
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
            Ok(())
        }

        /// Should only be callable by OCWs (TODO)
        /// Submit the results of an `ipfs identity` call to be stored on chain
        ///
        /// * origin: a validator node
        /// * public_key: The IPFS node's public key
        /// * multiaddresses: A vector of multiaddresses associate with the public key
        ///
        #[pallet::weight(0)]
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

		#[pallet::weight(0)]
		pub fn submit_ipfs_pin_result(
			origin: OriginFor<T>,
			asset_id: T::AssetId,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let current_pinners = <Pinners::<T>>::get(asset_id.clone());
			ensure!(!current_pinners.contains(&who), Error::<T>::AlreadyPinned);
			// TODO: we need a better scheme for *generating* pool ids -> should always be unique (cid + owner maybe?)
			<Pinners<T>>::mutate(asset_id.clone(), |p| {
				p.push(who.clone());
			});
			Ok(())
		}

        /// Should only be callable by OCWs (TODO)
        /// Submit the results onchain to notify a beneficiary that their data is available: TODO: how to safely share host? spam protection on rpc endpoints?
        ///
        /// * `beneficiary`: The account that requested the data
        /// * `host`: The node's host where the data has been made available (RPC endpoint)
        ///
        #[pallet::weight(0)]
        pub fn submit_rpc_ready(
            origin: OriginFor<T>,
            beneficiary: T::AccountId,
            // host: Vec<u8>,
        ) -> DispatchResult {
            ensure_signed(origin)?;
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

		// Clear the offline validator list to avoid repeated deletion.
		<OfflineValidators<T>>::put(Vec::<T::AccountId>::new());
	}

	/// move candidates to the active provider pool for some asset id
	fn select_candidate_storage_providers() {
		// TODO: for now, we will just copy the candidates to the active providers map
		// no real selection is happening yet, just doing this to get it in place for now
		// iterate over asset ids
		// if there are candidate storage providers => for each candidate that pinned the file, move them to storage providers
		for assetid in <pallet_iris_assets::Pallet<T>>::asset_ids().into_iter() {
			// if there are candidates for the asset id
			if <CandidateStorageProviders<T>>::contains_key(assetid.clone()) {
				let candidates = <CandidateStorageProviders<T>>::get(assetid.clone());
				let pinners = <Pinners<T>>::get(assetid.clone());
				let pinner_candidate_intersection = 
					candidates.into_iter().filter(|c| pinners.contains(c)).collect::<Vec<T::AccountId>>();
				// need to only move candidates that have actually proved they pinned the content	
				<StorageProviders::<T>>::insert(assetid.clone(), pinner_candidate_intersection);
			} 
		}
	}

	/// implementation for RPC runtime aPI to retrieve bytes from the node's local storage
    /// 
    /// * public_key: The account's public key as bytes
    /// * signature: The signer's signature as bytes
    /// * message: The signed message as bytes
    ///
    pub fn retrieve_bytes(
        _public_key: Bytes,
		_signature: Bytes,
		message: Bytes,
    ) -> Bytes {
        // TODO: Verify signature, update offchain storage keys...
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
                },
                DataCommand::CatBytes(owner, asset_id, recipient) => {
					// TODO: Could potentially remove the owner here by restructuring runtime storage -> would need unique asset ids
					if let cid = <pallet_iris_assets::Pallet<T>>::asset_class_ownership(
						owner.clone(), asset_id.clone()
					) {
						let balance = <pallet_assets::Pallet<T>>::balance(asset_id.clone(), recipient.clone());
						let balance_primitive = TryInto::<u64>::try_into(balance).ok();
					
						ensure!(balance_primitive != Some(0), Error::<T>::InsufficientBalance);
						match Self::ipfs_request(IpfsRequest::CatBytes(cid.clone()), deadline) {
							Ok(IpfsResponse::CatBytes(data)) => {
								log::info!("IPFS: Fetched data from IPFS.");
								// add to offchain index
								sp_io::offchain::local_storage_set(
									StorageKind::PERSISTENT,
									&cid,
									&data,
								);
								let signer = Signer::<T, T::AuthorityId>::all_accounts();
								if !signer.can_sign() {
									log::error!(
										"No local accounts available. Consider adding one via `author_insertKey` RPC.",
									);
								}
								let results = signer.send_signed_transaction(|_account| { 
									Call::submit_rpc_ready {
										beneficiary: recipient.clone(),
									}
								});
						
								for (_, res) in &results {
									match res {
										Ok(()) => log::info!("Submitted ipfs results"),
										Err(e) => log::error!("Failed to submit transaction: {:?}",  e),
									}
								}
							},
							Ok(_) => unreachable!("only CatBytes can be a response for that request type."),
							Err(e) => log::error!("IPFS: cat error: {:?}", e),
						}
					} else {
						log::error!("the provided owner/cid does not map to a valid asset id: {:?}, {:?}", owner, asset_id)
					}
                },
				DataCommand::PinCID(acct, asset_id, cid) => {
					let (public_key, addrs) = 
						if let IpfsResponse::Identity(public_key, addrs) = 
							Self::ipfs_request(IpfsRequest::Identity, deadline)? {
						(public_key, addrs)
					} else {
						unreachable!("only `Identity` is a valid response type.");
					};
					let expected_pub_key = <SubstrateIpfsBridge::<T>>::get(acct);
					// todo: create new error enum if this is the route i choose
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
		// Remove any offline validators. This will only work when the runtime
		// also has the im-online pallet.
		Self::remove_offline_validators();
		Self::select_candidate_storage_providers();
		log::debug!(target: LOG_TARGET, "New session called; updated validator set provided.");
		Some(Self::validators())
	}

	fn end_session(end_index: u32) {
		log::info!("Ending session with index: {:?}", end_index)
	}

	fn start_session(start_index: u32) {
		log::info!("Starting session with index: {:?}", start_index);
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
