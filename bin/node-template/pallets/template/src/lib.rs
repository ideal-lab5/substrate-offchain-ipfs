#![cfg_attr(not(feature = "std"), no_std)]

//! # Iris Storage Pallet
//!
//!
//! ## Overview
//! Disclaimer: This pallet is in the tadpole state
//!
//! ### Goals
//! The Iris module provides functionality for creation and management of storage assets and access management
//! 
//! ### Dispatchable Functions 
//!
//! #### Permissionless functions
//! * create_storage_asset
//! * request_data
//!
//! #### Permissioned Functions
//! * submit_ipfs_add_results
//! * submit_ipfs_identity
//! * submit_rpc_ready
//! * destroy_ticket
//! * mint_tickets
//!

use scale_info::TypeInfo;
use codec::{Encode, Decode};
use frame_support::{
    ensure,
    traits::ReservableCurrency,
};
use frame_system::{
    self as system, ensure_signed,
    offchain::{
        SendSignedTransaction, 
        Signer,
    },
};

use sp_core::{
    offchain::{
        Duration, IpfsRequest, IpfsResponse, OpaqueMultiaddr, Timestamp, StorageKind,
    },
    crypto::KeyTypeId,
    Bytes,
};

use sp_io::offchain::timestamp;
use sp_runtime::{
    offchain::{ 
        ipfs,
    },
    RuntimeDebug,
    traits::{
        StaticLookup,
        Convert,
    },
};
use sp_std::{
    str,
    vec::Vec,
    prelude::*,
    convert::TryInto,
    collections::btree_map::BTreeMap,
};

use pallet_session::historical;

pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"iris");
pub type SessionIndex = u32;
pub type EraIndex = u32;
pub type RewardPoint = u32;

/// Information regarding the active era (era in used in session).
#[derive(Encode, Decode, RuntimeDebug, TypeInfo)]
pub struct ActiveEraInfo {
	/// Index of era.
	pub index: EraIndex,
	/// Anchor era number of this era.
	pub set_id: u32,
	/// Moment of start expressed as millisecond from `$UNIX_EPOCH`.
	///
	/// Start can be none if start hasn't been set for the era yet,
	/// Start is set on the first on_finalize of the era to guarantee usage of `Time`.
	start: Option<u64>,
}

/// Reward points of an era. Used to split era total payout between validators.
///
/// This points will be used to reward validators and their respective nominators.
#[derive(PartialEq, Encode, Decode, Default, RuntimeDebug, TypeInfo)]
pub struct EraRewardPoints<AccountId: Ord> {
	/// Total number of points. Equals the sum of reward points for each validator.
	total: RewardPoint,
	/// The reward points earned by a given validator.
	individual: BTreeMap<AccountId, RewardPoint>,
}

/// Means for interacting with a specialized version of the `session` trait.
///
/// This is needed because `Staking` sets the `ValidatorIdOf` of the `pallet_session::Config`
pub trait SessionInterface<AccountId>: frame_system::Config {
	/// Disable a given validator by stash ID.
	///
	/// Returns `true` if new era should be forced at the end of this session.
	/// This allows preventing a situation where there is too many validators
	/// disabled and block production stalls.
	fn disable_validator(validator: &AccountId) -> Result<bool, ()>;
	/// Get the validators from session.
	fn validators() -> Vec<AccountId>;
	/// Prune historical session tries up to but not including the given index.
	fn prune_historical_up_to(up_to: SessionIndex);

	fn is_active_validator(id: KeyTypeId, key_data: &[u8]) -> Option<AccountId>;
}

impl<T: Config> SessionInterface<<T as frame_system::Config>::AccountId> for T
where
	T: pallet_session::Config<ValidatorId = <T as frame_system::Config>::AccountId>,
	T: pallet_session::historical::Config<
		FullIdentification = u128,
		FullIdentificationOf = ExposureOf<T>,
	>,
	T::SessionHandler: pallet_session::SessionHandler<<T as frame_system::Config>::AccountId>,
	T::SessionManager: pallet_session::SessionManager<<T as frame_system::Config>::AccountId>,
	T::ValidatorIdOf: Convert<
		<T as frame_system::Config>::AccountId,
		Option<<T as frame_system::Config>::AccountId>,
	>,
{
	fn disable_validator(validator: &<T as frame_system::Config>::AccountId) -> Result<bool, ()> {
		<pallet_session::Pallet<T>>::disable(validator)
	}

	fn validators() -> Vec<<T as frame_system::Config>::AccountId> {
		<pallet_session::Pallet<T>>::validators()
	}

	fn prune_historical_up_to(up_to: SessionIndex) {
		<pallet_session::historical::Pallet<T>>::prune_up_to(up_to);
	}

	fn is_active_validator(
		id: KeyTypeId,
		key_data: &[u8],
	) -> Option<<T as frame_system::Config>::AccountId> {
		let who = <pallet_session::Pallet<T>>::key_owner(id, key_data);
		if who.is_none() {
			return None;
		}

		Self::validators().into_iter().find(|v| {
			log::info!("check {:#?} == {:#?}", v, who);
			T::ValidatorIdOf::convert(v.clone()) == who
		})
	}
}

pub mod crypto {
	use crate::KEY_TYPE;
	use sp_core::sr25519::Signature as Sr25519Signature;
	use sp_runtime::app_crypto::{app_crypto, sr25519};
	use sp_runtime::{traits::Verify, MultiSignature, MultiSigner};

	app_crypto!(sr25519, KEY_TYPE);

	pub struct TestAuthId;
	// implemented for untime
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

#[derive(Encode, Decode, RuntimeDebug, PartialEq, TypeInfo)]
pub enum DataCommand<LookupSource, AssetId, Balance, AccountId> {
    /// (ipfs_address, cid, requesting node address, filename, asset id, balance)
    AddBytes(OpaqueMultiaddr, Vec<u8>, LookupSource, Vec<u8>, AssetId, Balance),
    // /// owner, cid
    CatBytes(AccountId, Vec<u8>, AccountId),
}

/// Something that can provide a set of validators for the next era.
pub trait ValidatorsProvider<AccountId> {
	/// A new set of validators.
	fn validators() -> Vec<(AccountId, u128)>;
}

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
	use frame_support::{dispatch::DispatchResult, pallet_prelude::*};
	use frame_system::{
        pallet_prelude::*,
        offchain::{
            AppCrypto,
            CreateSignedTransaction,
        },
    };
	use sp_core::offchain::OpaqueMultiaddr;
	use sp_std::{str, vec::Vec};

	#[pallet::config]
    /// the module configuration trait
	pub trait Config:CreateSignedTransaction<Call<Self>> + frame_system::Config + pallet_assets::Config {
        /// The overarching event type
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        /// the authority id used for sending signed txs
        type AuthorityId: AppCrypto<Self::Public, Self::Signature>;
        /// the overarching call type
	    type Call: From<Call<Self>>;
        /// the currency used
        type Currency: ReservableCurrency<Self::AccountId>;
        /// interface for interacting with the session pallet
        type SessionInterface: self::SessionInterface<Self::AccountId>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);


	#[pallet::type_value]
	pub(crate) fn HistoryDepthOnEmpty() -> u32 {
		84u32
	}

    /// map the public key to a list of multiaddresses
    #[pallet::storage]
    #[pallet::getter(fn bootstrap_nodes)]
    pub(super) type BootstrapNodes<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        Vec<u8>,
        Vec<OpaqueMultiaddr>,
        ValueQuery,
    >;

    /// A queue of data to publish or obtain on IPFS.
	#[pallet::storage]
    #[pallet::getter(fn data_queue)]
	pub(super) type DataQueue<T: Config> = StorageValue<
        _,
        Vec<DataCommand<
            <T::Lookup as StaticLookup>::Source, 
            T::AssetId,
            T::Balance,
            T::AccountId>
        >,
        ValueQuery
    >;

    /// Store the map associating owned CID to a specific asset ID
    ///
    /// asset_admin_accountid -> CID -> asset id
    #[pallet::storage]
    #[pallet::getter(fn created_asset_classes)]
    pub(super) type AssetClassOwnership<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        Vec<u8>,
        T::AssetId,
        ValueQuery,
    >;

    /// Store the map associated a node with the assets to which they have access
    ///
    /// asset_owner_accountid -> CID -> asset_class_owner_accountid
    #[pallet::storage]
    #[pallet::getter(fn asset_access)]
    pub(super) type AssetAccess<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        Vec<u8>,
        T::AccountId,
        ValueQuery,
    >;

    /*
        START: SESSION STORAGE STUFF
    */
    
	/// Number of eras to keep in history.
	///
	/// Information is kept for eras in `[current_era - history_depth; current_era]`.
	///
	/// Must be more than the number of eras delayed by session otherwise. I.e. active era must
	/// always be in history. I.e. `active_era > current_era - history_depth` must be
	/// guaranteed.
	#[pallet::storage]
	#[pallet::getter(fn history_depth)]
	pub(crate) type HistoryDepth<T> = StorageValue<_, u32, ValueQuery, HistoryDepthOnEmpty>;

    /// The current era index.
	///
	/// This is the latest planned era, depending on how the Session pallet queues the validator
	/// set, it might be active or not.
	#[pallet::storage]
	#[pallet::getter(fn current_era)]
	pub type CurrentEra<T> = StorageValue<_, EraIndex>;

	/// The active era information, it holds index and start.
	///
	/// The active era is the era being currently rewarded. Validator set of this era must be
	/// equal to [`SessionInterface::validators`].
	#[pallet::storage]
	#[pallet::getter(fn active_era)]
	pub type ActiveEra<T> = StorageValue<_, ActiveEraInfo>;

	/// The session index at which the era start for the last `HISTORY_DEPTH` eras.
	///
	/// Note: This tracks the starting session (i.e. session index when era start being active)
	/// for the eras in `[CurrentEra - HISTORY_DEPTH, CurrentEra]`.
	#[pallet::storage]
	#[pallet::getter(fn eras_start_session_index)]
	pub type ErasStartSessionIndex<T> = StorageMap<_, Twox64Concat, EraIndex, SessionIndex>;

	/// Exposure of validator at era.
	///
	/// This is keyed first by the era index to allow bulk deletion and then the stash account.
	///
	/// Is it removed after `HISTORY_DEPTH` eras.
	/// If stakers hasn't been set or has been removed then empty exposure is returned.
	#[pallet::storage]
	#[pallet::getter(fn eras_stakers)]
	pub type ErasStakers<T: Config> =
		StorageDoubleMap<_, Twox64Concat, EraIndex, Twox64Concat, T::AccountId, u128, ValueQuery>;

	/// The total validator era payout for the last `HISTORY_DEPTH` eras.
	///
	/// Eras that haven't finished yet or has been removed doesn't have reward.
	#[pallet::storage]
	#[pallet::getter(fn eras_validator_reward)]
	pub type ErasValidatorReward<T: Config> = StorageMap<_, Twox64Concat, EraIndex, u128>;

	/// Rewards for the last `HISTORY_DEPTH` eras.
	/// If reward hasn't been set or has been removed then 0 reward is returned.
	#[pallet::storage]
	#[pallet::getter(fn eras_reward_points)]
	pub type ErasRewardPoints<T: Config> =
		StorageMap<_, Twox64Concat, EraIndex, EraRewardPoints<T::AccountId>, ValueQuery>;

	/// The total amount staked for the last `HISTORY_DEPTH` eras.
	/// If total hasn't been set or has been removed then 0 stake is returned.
	#[pallet::storage]
	#[pallet::getter(fn eras_total_stake)]
	pub type ErasTotalStake<T: Config> = StorageMap<_, Twox64Concat, EraIndex, u128, ValueQuery>;

	/// A mapping from still-bonded eras to the first session index of that era.
	///
	/// Must contains information for eras for the range:
	/// `[active_era - bounding_duration; active_era]`
	#[pallet::storage]
	pub(crate) type BondedEras<T: Config> =
		StorageValue<_, Vec<(EraIndex, SessionIndex)>, ValueQuery>;

	/// The last planned session scheduled by the session pallet.
	///
	/// This is basically in sync with the call to [`SessionManager::new_session`].
	#[pallet::storage]
	#[pallet::getter(fn current_planned_session)]
	pub type CurrentPlannedSession<T> = StorageValue<_, SessionIndex, ValueQuery>;

	/// The payout for validators and the system for the current era.
	#[pallet::storage]
	#[pallet::getter(fn era_payout)]
	pub type EraPayout<T> = StorageValue<_, u128, ValueQuery>;

    /*
        END
    */
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
        /// A request to add bytes was queued
        QueuedDataToAdd(T::AccountId),
        /// A request to retrieve bytes was queued
        QueuedDataToCat(T::AccountId),
        /// A new asset class was created (add bytes command processed)
        AssetClassCreated(T::AssetId),
        /// A new asset was created (tickets minted)
        AssetCreated(T::AssetId),
        /// A node's request to access data via the RPC endpoint has been processed
        DataReady(T::AccountId),
        /// A node has published ipfs identity results on chain
        PublishedIdentity(T::AccountId),
	}

	#[pallet::error]
	pub enum Error<T> {
        /// could not build the ipfs request
		CantCreateRequest,
        /// the request to IPFS timed out
        RequestTimeout,
        /// the request to IPFS failed
        RequestFailed,
        /// The tx could not be signed
        OffchainSignedTxError,
        /// you cannot sign a tx
        NoLocalAcctForSigning,
        /// could not create a new asset
        CantCreateAssetClass,
        /// could not mint a new asset
        CantMintAssets,
        /// there is no asset associated with the specified cid
        NoSuchOwnedContent,
        /// the specified asset class does not exist
        NoSuchAssetClass,
        /// the account does not have a sufficient balance
        InsufficientBalance,
	}

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
         fn on_initialize(block_number: T::BlockNumber) -> Weight {
            // needs to be synchronized with offchain_worker actitivies
            if block_number % 2u32.into() == 1u32.into() {
                <DataQueue<T>>::kill();
            }

            0
        }
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

	#[pallet::call]
	impl<T: Config> Pallet<T> {

        /// submits an on-chain request to fetch data and add it to iris 
        /// 
        /// * `addr`: the multiaddress where the data exists
        ///       example: /ip4/192.168.1.170/tcp/4001/p2p/12D3KooWMvyvKxYcy9mjbFbXcogFSCvENzQ62ogRxHKZaksFCkAp
        /// * `cid`: the cid to fetch from the multiaddress
        ///       example: QmPZv7P8nQUSh2CpqTvUeYemFyjvMjgWEs8H1Tm8b3zAm9
        /// * `id`: (temp) the unique id of the asset class -> should be generated instead
        /// * `balance`: the balance the owner is willing to use to back the asset class which will be created
        ///
        #[pallet::weight(0)]
        pub fn create_storage_asset(
            origin: OriginFor<T>,
            admin: <T::Lookup as StaticLookup>::Source,
            addr: Vec<u8>,
            cid: Vec<u8>,
            name: Vec<u8>,
            id: T::AssetId,
            balance: T::Balance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let multiaddr = OpaqueMultiaddr(addr);
            <DataQueue<T>>::mutate(
                |queue| queue.push(DataCommand::AddBytes(
                    multiaddr,
                    cid,
                    admin.clone(),
                    name.clone(),
                    id.clone(),
                    balance.clone(),
                )));
            Self::deposit_event(Event::QueuedDataToAdd(who.clone()));
			Ok(())
        }

        /// Queue a request to retrieve data behind some owned CID from the IPFS network
        ///
        /// * owner: The owner node
        /// * cid: the cid to which you are requesting access
        ///
		#[pallet::weight(0)]
        pub fn request_data(
            origin: OriginFor<T>,
            owner: <T::Lookup as StaticLookup>::Source,
            cid: Vec<u8>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let owner_account = T::Lookup::lookup(owner)?;

            <DataQueue<T>>::mutate(
                |queue| queue.push(DataCommand::CatBytes(
                    owner_account.clone(),
                    cid.clone(),
                    who.clone(),
                )
            ));

            Self::deposit_event(Event::QueuedDataToCat(who.clone()));
            
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
            // DANGER: This can currently be called by anyone, not just an OCW.
            // if we send an unsigned transaction then we can ensure there is no origin
            // however, the call to create the asset requires an origin, which is a little problematic
            // ensure_none(origin)?;
            let who = ensure_signed(origin)?;
            let new_origin = system::RawOrigin::Signed(who).into();

            <pallet_assets::Pallet<T>>::create(new_origin, id.clone(), admin.clone(), balance)
                .map_err(|_| Error::<T>::CantCreateAssetClass)?;
            
            let which_admin = T::Lookup::lookup(admin.clone())?;
            <AssetClassOwnership<T>>::insert(which_admin, cid.clone(), id.clone());
            
            Self::deposit_event(Event::AssetClassCreated(id.clone()));
            
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
            Self::deposit_event(Event::PublishedIdentity(who.clone()));
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
            Self::deposit_event(Event::DataReady(beneficiary));
            Ok(())
        }

        /// Only callable by the owner of the asset class 
        /// mint a static number of assets (tickets) for some asset class
        ///
        /// * origin: should be the owner of the asset class
        /// * beneficiary: the address to which the newly minted assets are assigned
        /// * cid: a cid owned by the origin, for which an asset class exists
        /// * amount: the number of tickets to mint
        ///
        #[pallet::weight(0)]
        pub fn mint_tickets(
            origin: OriginFor<T>,
            beneficiary: <T::Lookup as StaticLookup>::Source,
            cid: Vec<u8>,
            #[pallet::compact] amount: T::Balance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let new_origin = system::RawOrigin::Signed(who.clone()).into();
            let beneficiary_accountid = T::Lookup::lookup(beneficiary.clone())?;

            ensure!(AssetClassOwnership::<T>::contains_key(who.clone(), cid.clone()), Error::<T>::NoSuchOwnedContent);
            
            let asset_id = AssetClassOwnership::<T>::get(who.clone(), cid.clone(),);
            <pallet_assets::Pallet<T>>::mint(new_origin, asset_id.clone(), beneficiary.clone(), amount)
                .map_err(|_| Error::<T>::CantMintAssets)?;
            
            <AssetAccess<T>>::insert(beneficiary_accountid.clone(), cid.clone(), who.clone());
        
            Self::deposit_event(Event::AssetCreated(asset_id.clone()));
            Ok(())
        }

        /// TODO: leaving this as is for now... I feel like this will require some further thought. We almost need a dex-like feature
        /// Purchase a ticket to access some content.
        ///
        /// * origin: any origin
        /// * owner: The owner to identify the asset class for which a ticket is to be purchased
        /// * cid: The CID to identify the asset class for which a ticket is to be purchased
        /// * amount: The number of tickets to purchase
        ///
        #[pallet::weight(0)]
        pub fn purchase_ticket(
            origin: OriginFor<T>,
            _owner: <T::Lookup as StaticLookup>::Source,
            _cid: Vec<u8>,
            #[pallet::compact] _amount: T::Balance,
            _test: T::Signature,
        ) -> DispatchResult {
            ensure_signed(origin)?;
            // determine price for amount of asset and verify origin has a min balance
            // transfer native currency to asset class admin
            // admin transfers the requested amount of tokens to the buyer
            Ok(())
        }
	}
}

impl<T: Config> Pallet<T> {
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
        let ipfs_request = ipfs::PendingRequest::new(req).map_err(|_| Error::<T>::CantCreateRequest)?;
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
        
        let (public_key, addrs) = if let IpfsResponse::Identity(public_key, addrs) = Self::ipfs_request(IpfsRequest::Identity, deadline)? {
            (public_key, addrs)
        } else {
            unreachable!("only `Identity` is a valid response type.");
        };

        if !<BootstrapNodes::<T>>::contains_key(public_key.clone()) {
            if let Some(bootstrap_node) = &<BootstrapNodes::<T>>::iter().nth(0) {
                if let Some(bootnode_maddr) = bootstrap_node.1.clone().pop() {
                    if let IpfsResponse::Success = Self::ipfs_request(IpfsRequest::Connect(bootnode_maddr.clone()), deadline)? {
                        log::info!("Succesfully connected to a bootstrap node: {:?}", &bootnode_maddr.0);
                    } else {
                        log::info!("Failed to connect to the bootstrap node with multiaddress: {:?}", &bootnode_maddr.0);
                        // TODO: this should probably be some recursive function? but we should never exceed a depth of 2 so maybe not
                        if let Some(next_bootnode_maddr) = bootstrap_node.1.clone().pop() {
                            if let IpfsResponse::Success = Self::ipfs_request(IpfsRequest::Connect(next_bootnode_maddr.clone()), deadline)? {
                                log::info!("Succesfully connected to a bootstrap node: {:?}", &next_bootnode_maddr.0);
                            } else {
                                log::info!("Failed to connect to the bootstrap node with multiaddress: {:?}", &next_bootnode_maddr.0);
                            }       
                        }
                    }
                }
            }
            // TODO: should create func to encompass the below logic 
            let signer = Signer::<T, T::AuthorityId>::all_accounts();
            if !signer.can_sign() {
                log::error!(
                    "No local accounts available. Consider adding one via `author_insertKey` RPC.",
                );
            }
             
            let results = signer.send_signed_transaction(|_account| { 
                Call::submit_ipfs_identity{
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
        let data_queue = DataQueue::<T>::get();
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
                DataCommand::CatBytes(owner, cid, recipient) => {
                    ensure!(AssetClassOwnership::<T>::contains_key(owner.clone(), cid.clone()), Error::<T>::NoSuchOwnedContent);
                    let asset_id = AssetClassOwnership::<T>::get(owner.clone(), cid.clone());
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

    /*
        START: SESSION STUFF
    */
        
    	/// Plan a new session potentially trigger a new era.
	fn new_session(session_index: SessionIndex, is_genesis: bool) -> Option<Vec<T::AccountId>> {
		if let Some(current_era) = Self::current_era() {
			// Initial era has been set.
			let current_era_start_session_index = Self::eras_start_session_index(current_era)
				.unwrap_or_else(|| {
					frame_support::print("Error: start_session_index must be set for current_era");
					0
				});

			let era_length =
				session_index.checked_sub(current_era_start_session_index).unwrap_or(0); // Must never happen.

			// log!(info, "Era length: {:?}", era_length);
			// if era_length < T::SessionsPerEra::get() {
			// 	// The 5th session of the era.
			// 	if T::AppchainInterface::is_activated()
			// 		&& (era_length == T::SessionsPerEra::get() - 1)
			// 	{
			// 		let next_set_id = T::AppchainInterface::next_set_id();
			// 		let message = PlanNewEraPayload { new_era: next_set_id };

			// 		let res = T::UpwardMessagesInterface::submit(
			// 			&T::AccountId::default(),
			// 			PayloadType::PlanNewEra,
			// 			&message.try_to_vec().unwrap(),
			// 		);
			// 		log!(info, "UpwardMessage::PlanNewEra: {:?}", res);
			// 		if res.is_ok() {
			// 			Self::deposit_event(Event::<T>::PlanNewEra(next_set_id));
			// 		} else {
			// 			Self::deposit_event(Event::<T>::PlanNewEraFailed);
			// 		}
			// 	}
			// 	return None;
			// }

			// New era.
			Self::try_trigger_new_era(session_index)
		} else {
			// Set initial era.
			log::info!("Starting the first era.");
			Self::try_trigger_new_era(session_index)
		}
	}

	/// Start a session potentially starting an era.
	fn start_session(start_session: SessionIndex) {
		let next_active_era = Self::active_era().map(|e| e.index + 1).unwrap_or(0);
		// This is only `Some` when current era has already progressed to the next era, while the
		// active era is one behind (i.e. in the *last session of the active era*, or *first session
		// of the new current era*, depending on how you look at it).
		if let Some(next_active_era_start_session_index) =
			Self::eras_start_session_index(next_active_era)
		{
			if next_active_era_start_session_index == start_session {
				Self::start_era(start_session);
			} else if next_active_era_start_session_index < start_session {
				// This arm should never happen, but better handle it than to stall the staking
				// pallet.
				frame_support::print("Warning: A session appears to have been skipped.");
				Self::start_era(start_session);
			}
		}
	}

	/// End a session potentially ending an era.
	fn end_session(session_index: SessionIndex) {
		if let Some(active_era) = Self::active_era() {
			if let Some(next_active_era_start_session_index) =
				Self::eras_start_session_index(active_era.index + 1)
			{
				if next_active_era_start_session_index == session_index + 1 {
					Self::end_era(active_era, session_index);
				}
			}
		}
	}

	/// * Increment `active_era.index`,
	/// * reset `active_era.start`,
	/// * update `BondedEras` and apply slashes.
	fn start_era(start_session: SessionIndex) {
		// let active_era = ActiveEra::<T>::mutate(|active_era| {
		// 	let next_set_id = T::AppchainInterface::next_set_id();
		// 	let new_index = active_era.as_ref().map(|info| info.index + 1).unwrap_or(0);
		// 	*active_era = Some(ActiveEraInfo {
		// 		index: new_index,
		// 		set_id: next_set_id - 1,
		// 		// Set new active era start in next `on_finalize`. To guarantee usage of `Time`
		// 		start: None,
		// 	});
		// 	new_index
		// });

		// let bonding_duration = T::BondingDuration::get();

		// BondedEras::<T>::mutate(|bonded| {
		// 	bonded.push((active_era, start_session));

		// 	if active_era > bonding_duration {
		// 		let first_kept = active_era - bonding_duration;

		// 		// Prune out everything that's from before the first-kept index.
		// 		let n_to_prune =
		// 			bonded.iter().take_while(|&&(era_idx, _)| era_idx < first_kept).count();

		// 		if let Some(&(_, first_session)) = bonded.first() {
		// 			T::SessionInterface::prune_historical_up_to(first_session);
		// 		}
		// 	}
		// });
	}

    /// Compute payout for era.
	fn end_era(active_era: ActiveEraInfo, _session_index: SessionIndex) {
        return;
		// if !T::AppchainInterface::is_activated() || <EraPayout<T>>::get() == 0 {
		// 	return;
		// }

		// // Note: active_era_start can be None if end era is called during genesis config.
		// if let Some(active_era_start) = active_era.start {
		// 	if <ErasValidatorReward<T>>::get(&active_era.index).is_some() {
		// 		log!(warn, "era reward {:?} has already been paid", active_era.index);
		// 		return;
		// 	}

		// 	let now_as_millis_u64 = T::UnixTime::now().as_millis().saturated_into::<u64>();
		// 	let _era_duration = (now_as_millis_u64 - active_era_start).saturated_into::<u64>();
		// 	let validator_payout = Self::era_payout();

		// 	// Set ending era reward.
		// 	<ErasValidatorReward<T>>::insert(&active_era.index, validator_payout);

		// 	let excluded_validators = Self::get_exclude_validators(active_era.index);

		// 	let excluded_validators_str = excluded_validators
		// 		.iter()
		// 		.map(|validator| {
		// 			let prefix = String::from("0x");
		// 			let hex_validator = prefix + &hex::encode(validator.encode());
		// 			hex_validator
		// 		})
		// 		.collect::<Vec<String>>();

		// 	log!(debug, "Exclude validators: {:?}", excluded_validators_str.clone());

		// 	let message = EraPayoutPayload {
		// 		end_era: active_era.set_id,
		// 		excluded_validators: excluded_validators_str.clone(),
		// 	};

		// 	let amount = validator_payout.checked_into().ok_or(Error::<T>::AmountOverflow).unwrap();
		// 	T::Currency::deposit_creating(&Self::account_id(), amount);
		// 	log!(debug, "Will send EraPayout message, era_payout is {:?}", <EraPayout<T>>::get());

		// 	let res = T::UpwardMessagesInterface::submit(
		// 		&T::AccountId::default(),
		// 		PayloadType::EraPayout,
		// 		&message.try_to_vec().unwrap(),
		// 	);
		// 	log!(info, "UpwardMessage::EraPayout: {:?}", res);
		// 	if res.is_ok() {
		// 		Self::deposit_event(Event::<T>::EraPayout(active_era.set_id, excluded_validators));
		// 	} else {
		// 		Self::deposit_event(Event::<T>::EraPayoutFailed(active_era.set_id));
		// 	}
		// }
	}

    
	/// Plan a new era.
	///
	/// * Bump the current era storage (which holds the latest planned era).
	/// * Store start session index for the new planned era.
	/// * Clean old era information.
	/// * Store staking information for the new planned era
	///
	/// Returns the new validator set.
	pub fn trigger_new_era(
		start_session_index: SessionIndex,
		validators: Vec<(T::AccountId, u128)>,
	) -> Vec<T::AccountId> {
		// Increment or set current era.
		let new_planned_era = CurrentEra::<T>::mutate(|s| {
			*s = Some(s.map(|s| s + 1).unwrap_or(0));
			s.unwrap()
		});
		ErasStartSessionIndex::<T>::insert(&new_planned_era, &start_session_index);

		// Clean old era information.
		if let Some(old_era) = new_planned_era.checked_sub(Self::history_depth() + 1) {
			Self::clear_era_information(old_era);
		}

		// Set staking information for the new era.
		Self::store_stakers_info(validators, new_planned_era)
	}

	/// Potentially plan a new era.
	///
	/// Get planned validator set from `T::ValidatorsProvider`.
	fn try_trigger_new_era(start_session_index: SessionIndex) -> Option<Vec<T::AccountId>> {
		let validators = T::ValidatorsProvider::validators();
		log!(info, "Next validator set: {:?}", validators);

		<Pallet<T>>::deposit_event(Event::<T>::TriggerNewEra);
		Some(Self::trigger_new_era(start_session_index, validators))
	}

    /// Process the output of the validators provider.
	///
	/// Store staking information for the new planned era
	pub fn store_stakers_info(
		validators: Vec<(T::AccountId, u128)>,
		new_planned_era: EraIndex,
	) -> Vec<T::AccountId> {
		let elected_stashes = validators.iter().cloned().map(|(x, _)| x).collect::<Vec<_>>();

		let mut total_stake: u128 = 0;
		validators.into_iter().for_each(|(who, weight)| {
			total_stake = total_stake.saturating_add(weight);
			<ErasStakers<T>>::insert(new_planned_era, &who, weight);
		});

		// Insert current era staking information
		<ErasTotalStake<T>>::insert(&new_planned_era, total_stake);

		if new_planned_era > 0 {
			log::info!(
				"New validator set of size {:?} has been processed for era {:?}",
				elected_stashes.len(),
				new_planned_era,
			);
		}

		elected_stashes
	}

    	/// Clear all era information for given era.
	fn clear_era_information(era_index: EraIndex) {
		<ErasStakers<T>>::remove_prefix(era_index, None);
		<ErasValidatorReward<T>>::remove(era_index);
		<ErasRewardPoints<T>>::remove(era_index);
		<ErasTotalStake<T>>::remove(era_index);
		ErasStartSessionIndex::<T>::remove(era_index);
    }
	

    /*
        END: SESSION STUFF
    */
}


/// A typed conversion from stash account ID to the active exposure of nominators
/// on that account.
///
/// Active exposure is the exposure of the validator set currently validating, i.e. in
/// `active_era`. It can differ from the latest planned exposure in `current_era`.
pub struct ExposureOf<T>(sp_std::marker::PhantomData<T>);

impl<T: Config> Convert<T::AccountId, Option<u128>> for ExposureOf<T> {
	fn convert(validator: T::AccountId) -> Option<u128> {
		<Pallet<T>>::active_era()
			.map(|active_era| <Pallet<T>>::eras_stakers(active_era.index, &validator))
	}
}

/// In this implementation `new_session(session)` must be called before `end_session(session-1)`
/// i.e. the new session must be planned before the ending of the previous session.
///
/// Once the first new_session is planned, all session must start and then end in order, though
/// some session can lag in between the newest session planned and the latest session started.
impl<T: Config> pallet_session::SessionManager<T::AccountId> for Pallet<T> {
	fn new_session(new_index: SessionIndex) -> Option<Vec<T::AccountId>> {
		log::info!("planning new session {}", new_index);
		CurrentPlannedSession::<T>::put(new_index);
		Self::new_session(new_index, false)
	}
	fn new_session_genesis(new_index: SessionIndex) -> Option<Vec<T::AccountId>> {
		log::info!("planning new session {} at genesis", new_index);
		CurrentPlannedSession::<T>::put(new_index);
		Self::new_session(new_index, true)
	}
	fn start_session(start_index: SessionIndex) {
		log::info!("starting session {}", start_index);
		Self::start_session(start_index)
	}
	fn end_session(end_index: SessionIndex) {
		log::info!("ending session {}", end_index);
		Self::end_session(end_index)
	}
}

impl<T: Config> historical::SessionManager<T::AccountId, u128> for Pallet<T> {
	fn new_session(new_index: SessionIndex) -> Option<Vec<(T::AccountId, u128)>> {
		<Self as pallet_session::SessionManager<_>>::new_session(new_index).map(|validators| {
			let current_era = Self::current_era()
				// Must be some as a new era has been created.
				.unwrap_or(0);

			validators
				.into_iter()
				.map(|v| {
					let exposure = Self::eras_stakers(current_era, &v);
					(v, exposure)
				})
				.collect()
		})
	}
	fn new_session_genesis(new_index: SessionIndex) -> Option<Vec<(T::AccountId, u128)>> {
		<Self as pallet_session::SessionManager<_>>::new_session_genesis(new_index).map(
			|validators| {
				let current_era = Self::current_era()
					// Must be some as a new era has been created.
					.unwrap_or(0);

				validators
					.into_iter()
					.map(|v| {
						let exposure = Self::eras_stakers(current_era, &v);
						(v, exposure)
					})
					.collect()
			},
		)
	}
	fn start_session(start_index: SessionIndex) {
		<Self as pallet_session::SessionManager<_>>::start_session(start_index)
	}
	fn end_session(end_index: SessionIndex) {
		<Self as pallet_session::SessionManager<_>>::end_session(end_index)
	}
}