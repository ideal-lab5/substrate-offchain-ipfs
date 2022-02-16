#![cfg_attr(not(feature = "std"), no_std)]

//! # Iris Storage Pallet
//!
//!
//! ## Overview
//!
//! ### Goals
//! The Iris module provides functionality for creation and management of storage assets and access management
//! 
//! ### Dispatchable Functions 
//!
//! #### Permissionless functions
//! * create_storage_asset
//!
//! #### Permissioned Functions
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
};

use sp_core::offchain::OpaqueMultiaddr;

use sp_runtime::{
    RuntimeDebug,
    traits::StaticLookup,
};
use sp_std::{
    vec::Vec,
    prelude::*,
};

#[derive(Encode, Decode, RuntimeDebug, PartialEq, TypeInfo)]
pub enum DataCommand<LookupSource, AssetId, Balance, AccountId> {
    /// (ipfs_address, cid, requesting node address, filename, asset id, balance)
    AddBytes(OpaqueMultiaddr, Vec<u8>, LookupSource, Vec<u8>, AssetId, Balance),
    /// (requestor, owner, assetid)
    CatBytes(AccountId, AccountId, AssetId),
    /// (node, assetid, CID)
    PinCID(AccountId, AssetId, Vec<u8>),
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
    };
	use sp_core::offchain::OpaqueMultiaddr;
	use sp_std::{str, vec::Vec};

	#[pallet::config]
    /// the module configuration trait
	pub trait Config: frame_system::Config + pallet_assets::Config {
        /// The overarching event type
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        /// the overarching call type
	    type Call: From<Call<Self>>;
        /// the currency used
        type Currency: ReservableCurrency<Self::AccountId>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

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

    /// A collection of asset ids
    /// TODO: currently allows customized asset ids but in the future
    /// we can use this to dynamically generate unique asset ids for content
    #[pallet::storage]
    #[pallet::getter(fn asset_ids)]
    pub(super) type AssetIds<T: Config> = StorageValue<
        _,
        Vec<T::AssetId>,
        ValueQuery,
    >;

    /// Store the map associating owned CID to a specific asset ID
    ///
    /// asset_admin_accountid -> CID -> asset id
    #[pallet::storage]
    #[pallet::getter(fn asset_class_ownership)]
    pub(super) type AssetClassOwnership<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        T::AssetId,
        Vec<u8>,
        ValueQuery,
    >;

    /// Store the map associating a node with the assets to which they have access
    ///
    /// asset_owner_accountid -> CID -> asset_class_owner_accountid
    #[pallet::storage]
    #[pallet::getter(fn asset_access)]
    pub(super) type AssetAccess<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        T::AssetId,
        T::AccountId,
        ValueQuery,
    >;

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
        /// A node has published ipfs identity results on chain
        PublishedIdentity(T::AccountId),
        QueuedDataToPin,
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
        #[pallet::weight(100)]
        pub fn create(
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

        /// Only callable by the owner of the asset class 
        /// mint a static number of assets (tickets) for some asset class
        ///
        /// * origin: should be the owner of the asset class
        /// * beneficiary: the address to which the newly minted assets are assigned
        /// * cid: a cid owned by the origin, for which an asset class exists
        /// * amount: the number of tickets to mint
        ///
        #[pallet::weight(100)]
        pub fn mint(
            origin: OriginFor<T>,
            beneficiary: <T::Lookup as StaticLookup>::Source,
            asset_id: T::AssetId,
            #[pallet::compact] amount: T::Balance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(AssetClassOwnership::<T>::contains_key(who.clone(), asset_id.clone()), Error::<T>::NoSuchOwnedContent);

            let new_origin = system::RawOrigin::Signed(who.clone()).into();
            let beneficiary_accountid = T::Lookup::lookup(beneficiary.clone())?;
            <pallet_assets::Pallet<T>>::mint(new_origin, asset_id.clone(), beneficiary.clone(), amount)
                .map_err(|_| Error::<T>::CantMintAssets)?;
            <AssetAccess<T>>::insert(beneficiary_accountid.clone(), asset_id.clone(), who.clone());
        
            Self::deposit_event(Event::AssetCreated(asset_id.clone()));
            Ok(())
        }
        
        /// request to fetch bytes from ipfs and add to offchain storage
        /// 
        /// * `owner`: The owner of the content to be fetched 
        /// * `asset_id`: The asset id identifying the content
        /// 
		#[pallet::weight(100)]
		pub fn request_bytes(
			origin: OriginFor<T>,
			owner: <T::Lookup as StaticLookup>::Source,
			asset_id: T::AssetId,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
            let owner_account = T::Lookup::lookup(owner)?;
            <DataQueue<T>>::mutate(
                |queue| queue.push(DataCommand::CatBytes(
                    who.clone(),
                    owner_account.clone(),
                    asset_id.clone(),
                )));
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
        #[pallet::weight(100)]
        pub fn submit_ipfs_add_results(
            origin: OriginFor<T>,
            admin: <T::Lookup as StaticLookup>::Source,
            cid: Vec<u8>,
            id: T::AssetId,
            balance: T::Balance,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let new_origin = system::RawOrigin::Signed(who).into();

            <pallet_assets::Pallet<T>>::create(new_origin, id.clone(), admin.clone(), balance)
                .map_err(|_| Error::<T>::CantCreateAssetClass)?;
            
            let which_admin = T::Lookup::lookup(admin.clone())?;
            <AssetClassOwnership<T>>::insert(which_admin, id.clone(), cid.clone());
            <AssetIds<T>>::mutate(|ids| ids.push(id.clone()));
            
            Self::deposit_event(Event::AssetClassCreated(id.clone()));
            
            Ok(())
        }

        /// Add a request to pin a cid to the DataQueue for your embedded IPFS node
        /// 
        /// * asset_owner: The owner of the asset class
        /// * asset_id: The asset id of some asset class
        ///
        #[pallet::weight(100)]
        pub fn insert_pin_request(
            origin: OriginFor<T>,
            asset_owner: T::AccountId,
            asset_id: T::AssetId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(<AssetClassOwnership<T>>::contains_key(asset_owner.clone(), asset_id.clone()), Error::<T>::NoSuchOwnedContent);
            let cid = <AssetClassOwnership<T>>::get(
                asset_owner.clone(), 
                asset_id.clone(),
            );
            <DataQueue<T>>::mutate(
                |queue| queue.push(DataCommand::PinCID( 
                    who.clone(),
                    asset_id.clone(),
                    cid.clone(),
                )));

            Self::deposit_event(Event::QueuedDataToPin);
            
            Ok(())
        }

	}
}

impl<T: Config> Pallet<T> {

}