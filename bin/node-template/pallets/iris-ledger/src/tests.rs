use super::*;
use frame_support::{assert_ok};
use mock::*;
use sp_core::Pair;
use sp_core::{
	offchain::{testing, OffchainWorkerExt, TransactionPoolExt, OffchainDbExt}
};
use sp_keystore::{testing::KeyStore, KeystoreExt, SyncCryptoStore};
use std::sync::Arc;

#[test]
fn iris_ledger_can_lock() {
	let (p, _) = sp_core::sr25519::Pair::generate();
	let pairs = vec![(p.clone().public(), 10)];
	new_test_ext_funded(pairs).execute_with(|| {
		assert_ok!(IrisLedger::lock_currency(Origin::signed(p.clone().public()), 1));
		let mut locked_amount = crate::Ledger::<Test>::get(p.public().clone());
		assert_eq!(1, locked_amount);
	});
}

#[test]
fn iris_ledger_can_unlock_and_transfer() {
	let (p, _) = sp_core::sr25519::Pair::generate();
	let (p2, _) = sp_core::sr25519::Pair::generate();

	let pairs = vec![(p.clone().public(), 10)];
	new_test_ext_funded(pairs).execute_with(|| {
		assert_ok!(IrisLedger::lock_currency(Origin::signed(p.clone().public()), 1));
		
		assert_ok!(
			IrisLedger::unlock_currency_and_transfer(
				Origin::signed(p.clone().public()),
				p2.clone().public(),
			)
		);
		let mut locked_amount = crate::Ledger::<Test>::get(p.public().clone());
		assert_eq!(0, locked_amount);
	});
}
