#![cfg_attr(not(feature = "std"), no_std)]

//! # Iris Ledger Pallet
//!
//! ## Overview
//! This pallet provides functionality for nodes to lock funds, unlock funds, and transfer them to other nodes. 
//! In essence, it enables nodes to promise funds to other nodes from within the context of a smart contract (via a chain extension) 
//! or by calling the exgtrinsics directly.
//! 
//! ### Dispatchable Functions 
//! 
//! * lock_currency
//! 
//! * unlock_currency_and_transfer
//!
//!

use scale_info::TypeInfo;
use codec::{Encode, Decode};
use frame_support::{
    dispatch::DispatchResult,
    traits::{Currency, LockIdentifier, LockableCurrency, WithdrawReasons},
};
use frame_system::{
    self as system, ensure_signed,
};

use sp_core::offchain::OpaqueMultiaddr;

use sp_runtime::{
    RuntimeDebug,
    traits::{
		AtLeast32BitUnsigned, Bounded, CheckedAdd, CheckedSub, Saturating, StaticLookup, Zero,
	},
};
use sp_std::{
    prelude::*,
};
use codec::HasCompact;

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

    const IRIS_LOCK_ID: LockIdentifier = *b"irislock";

    type BalanceOf<T> =
        <<T as Config>::IrisCurrency as Currency<<T as frame_system::Config>::AccountId>>::Balance;


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
    pub(super) type Ledger<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        BalanceOf<T>,
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

	// #[pallet::error]
	// pub enum Error<T> {

	// }

	#[pallet::call]
	impl<T: Config> Pallet<T> {

        /// lock some amount of currency
        /// 
        /// * amount: the amount of currency to lock
        /// 
        #[pallet::weight(100)]
        pub fn lock_currency(
            origin: OriginFor<T>,
            #[pallet::compact] amount: BalanceOf<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            T::IrisCurrency::set_lock(
                IRIS_LOCK_ID,
                &who,
                amount.clone(),
                WithdrawReasons::all(),
            );
            <Ledger<T>>::insert(who.clone(), amount.clone());
            Self::deposit_event(Event::Locked(who, amount));
            Ok(())
        }

        /// Unlock currency and transfer it to a target node
        /// 
        /// * target: the node to which currency will be transferred 
        /// 
        #[pallet::weight(100)]
        pub fn unlock_currency_and_transfer(
            origin: OriginFor<T>,
            target: T::AccountId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // assume ammount in ledger matches locked amount for now
            let amount = <Ledger<T>>::get(who.clone());
            
            T::IrisCurrency::remove_lock(IRIS_LOCK_ID, &who);
            T::IrisCurrency::transfer(
                &who,
                &target,
                amount,
                KeepAlive,
            )?;

            <Ledger<T>>::remove(who.clone());
            Self::deposit_event(Event::Unlocked(who));
            Ok(())
        }

	}
}

impl<T: Config> Pallet<T> {

}