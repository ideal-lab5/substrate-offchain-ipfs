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
    traits::{Currency, LockIdentifier, LockableCurrency, WithdrawReasons},
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

type IRIS_LOCK_ID: LockIdentifier: *b"irislock";

type BalanceOf<T> =
        <<T as Config>::IrisCurrency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

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
	use frame_support::{
        dispatch::DispatchResult, 
        pallet_prelude::*,
        traits::ExistenceRequirement::KeepAlive,
    };
	use frame_system::{
        pallet_prelude::*,
    };
	use sp_core::offchain::OpaqueMultiaddr;
	use sp_std::{str, vec::Vec};

	#[pallet::config]
    /// the module configuration trait
	pub trait Config: frame_system::Config {
        /// The overarching event type
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        /// the overarching call type
	    type Call: From<Call<Self>>;
        /// the currency used
        type IrisCurrency: LockableCurrency<Self::AccountId, Moment = Self::BlockNumber>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

    // TODO: Move this to it's own pallet?
    #[pallet::storage]
    #[pallet::getter(fn iris_ledger)]
    pub(super) type IrisLedger<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        LockIdentifier,
        T::Balance,
        ValueQuery,
    >;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
        /// some amount of currency was locked
        Locked(T::AccountId, BalanceOf<T>),
        /// currency was unlocked by an account
        Unlocked(T::AccountId),
	}

	#[pallet::error]
	pub enum Error<T> {

	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {

        #[pallet::weight(100)]
        pub fn lock_currency(
            origin: OriginFor<T>,
            amount: T::IrisCurrency,
            cid : Vec<u8>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // let lock_id: LockIdentifier = cid;
            T::IrisCurrency::set_lock(
                IRIS_LOCK_ID,
                &who,
                amount.clone(),
                WithdrawReasons::all(),
            );
            <IrisLedger<T>>::insert(who.clone(), IRIS_LOCK_ID, amount.clone());
            Self::deposit_event(Event::Locked(who, amount));
            Ok(())
        }

        #[pallet::weight(100)]
        pub fn unlock_currency_and_transfer(
            origin: OriginFor<T>,
            target: T::AccountId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // assume ammount in ledger matches locked amount for now......
            let amount = <IrisLedger<T>>::get(who.clone(), IRIS_LOCK_ID);
            T::IrisCurrency::remove_lock(IRIS_LOCK_ID, &who);

            let new_origin = system::RawOrigin::Signed(who).into();
            T::IrisCurrency::transfer(
                new_origin,
                target,
                amount,
                KeepAlive,
            )?;
            Self::deposit_event(Event::Unlocked(who));
            Ok(())
        }

	}
}

impl<T: Config> Pallet<T> {

}