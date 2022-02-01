//! Tests for the Validator Set pallet.

#![cfg(test)]

use super::*;
use crate::mock::{authorities, new_test_ext, Origin, Session, Test, IrisSession};
use frame_support::{assert_noop, assert_ok, pallet_prelude::*};
use sp_runtime::testing::UintAuthorityId;
use sp_core::Pair;
use sp_core::{
	offchain::{testing, OffchainWorkerExt, TransactionPoolExt, OffchainDbExt}
};
use sp_keystore::{testing::KeyStore, KeystoreExt, SyncCryptoStore};
use std::sync::Arc;

#[test]
fn iris_session_simple_setup_should_work() {
	new_test_ext().execute_with(|| {
		assert_eq!(authorities(), vec![UintAuthorityId(1), UintAuthorityId(2), UintAuthorityId(3)]);
		assert_eq!(crate::Validators::<Test>::get(), vec![1u64, 2u64, 3u64]);
		assert_eq!(Session::validators(), vec![1, 2, 3]);
	});
}

#[test]
fn iris_session_add_validator_updates_validators_list() {
	new_test_ext().execute_with(|| {
		assert_ok!(IrisSession::add_validator(Origin::root(), 4));
		assert_eq!(crate::Validators::<Test>::get(), vec![1u64, 2u64, 3u64, 4u64])
	});
}

#[test]
fn iris_session_remove_validator_updates_validators_list() {
	new_test_ext().execute_with(|| {
		assert_ok!(ValidatorSet::remove_validator(Origin::root(), 2));
		assert_eq!(ValidatorSet::validators(), vec![1u64, 3u64]);
	});
}

#[test]
fn iris_session_add_validator_fails_with_invalid_origin() {
	new_test_ext().execute_with(|| {
		assert_noop!(ValidatorSet::add_validator(Origin::signed(1), 4), DispatchError::BadOrigin);
	});
}

#[test]
fn iris_session_remove_validator_fails_with_invalid_origin() {
	new_test_ext().execute_with(|| {
		assert_noop!(
			ValidatorSet::remove_validator(Origin::signed(1), 4),
			DispatchError::BadOrigin
		);
	});
}

#[test]
fn iris_session_duplicate_check() {
	new_test_ext().execute_with(|| {
		assert_ok!(ValidatorSet::add_validator(Origin::root(), 4));
		assert_eq!(ValidatorSet::validators(), vec![1u64, 2u64, 3u64, 4u64]);
		assert_noop!(ValidatorSet::add_validator(Origin::root(), 4), Error::<Test>::Duplicate);
	});
}

// RPC tests

// #[test]
// fn iris_session_submit_rpc_ready_works_for_valid_values() {
// 	let (p, _) = sp_core::sr25519::Pair::generate();
// 	new_test_ext_funded(p.clone()).execute_with(|| {
// 		assert_ok!(Iris::submit_rpc_ready(
// 			Origin::signed(p.clone().public()),
// 			p.clone().public(),
// 		));
// 	});
// }

// // test OCW functionality
// // can add bytes to network

// #[test]
// fn iris_can_add_bytes_to_ipfs() {
// 	let (p, _) = sp_core::sr25519::Pair::generate();
// 	let (offchain, state) = testing::TestOffchainExt::new();
// 	let (pool, _) = testing::TestTransactionPoolExt::new();
// 	const PHRASE: &str =
// 		"news slush supreme milk chapter athlete soap sausage put clutch what kitten";
// 	let keystore = KeyStore::new();
// 	SyncCryptoStore::sr25519_generate_new(
// 		&keystore,
// 		crate::KEY_TYPE,
// 		Some(&format!("{}/hunter1", PHRASE)),
// 	)
// 	.unwrap();

// 	let mut t = new_test_ext_funded(p.clone());
// 	t.register_extension(OffchainWorkerExt::new(offchain));
// 	t.register_extension(TransactionPoolExt::new(pool));
// 	t.register_extension(KeystoreExt(Arc::new(keystore)));

// 	let multiaddr_vec = "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWMvyvKxYcy9mjbFbXcogFSCvENzQ62ogRxHKZaksFCkAp".as_bytes().to_vec();
// 	let cid_vec = "QmPZv7P8nQUSh2CpqTvUeYemFyjvMjgWEs8H1Tm8b3zAm9".as_bytes().to_vec();
// 	let bytes = "hello test".as_bytes().to_vec();
// 	let name: Vec<u8> = "test.txt".as_bytes().to_vec();
// 	let id = 1;
// 	let balance = 1;
// 	// mock IPFS calls
// 	{	
// 		let mut state = state.write();
// 		// connect to external node
// 		state.expect_ipfs_request(testing::IpfsPendingRequest {
// 			response: Some(IpfsResponse::Success),
// 			..Default::default()
// 		});
// 		// fetch data
// 		state.expect_ipfs_request(testing::IpfsPendingRequest {
// 			id: sp_core::offchain::IpfsRequestId(0),
// 			response: Some(IpfsResponse::CatBytes(bytes.clone())),
// 			..Default::default()
// 		});
// 		// disconnect from the external node
// 		state.expect_ipfs_request(testing::IpfsPendingRequest {
// 			response: Some(IpfsResponse::Success),
// 			..Default::default()
// 		});
// 		// add bytes to your local node 
// 		state.expect_ipfs_request(testing::IpfsPendingRequest {
// 			response: Some(IpfsResponse::AddBytes(cid_vec.clone())),
// 			..Default::default()
// 		});
// 	}

// 	t.execute_with(|| {
// 		// WHEN: I invoke the create_storage_assets extrinsic
// 		assert_ok!(Iris::create_storage_asset(
// 			Origin::signed(p.clone().public()),
// 			p.clone().public(),
// 			multiaddr_vec.clone(),
// 			cid_vec.clone(),
// 			name.clone(),
// 			id.clone(),
// 			balance.clone(),
// 		));
// 		// THEN: the offchain worker adds data to IPFS
// 		assert_ok!(Iris::handle_data_requests());
// 	});
// }

// // can fetch bytes and add to offchain storage
// #[test]
// fn iris_can_fetch_bytes_and_add_to_offchain_storage() {
// 	let (p, _) = sp_core::sr25519::Pair::generate();
// 	let (offchain, state) = testing::TestOffchainExt::new();
// 	let (pool, _) = testing::TestTransactionPoolExt::new();
// 	const PHRASE: &str =
// 		"news slush supreme milk chapter athlete soap sausage put clutch what kitten";
// 	let keystore = KeyStore::new();
// 	SyncCryptoStore::sr25519_generate_new(
// 		&keystore,
// 		crate::KEY_TYPE,
// 		Some(&format!("{}/hunter1", PHRASE)),
// 	)
// 	.unwrap();

// 	let mut t = new_test_ext_funded(p.clone());
// 	t.register_extension(OffchainWorkerExt::new(offchain.clone()));
// 	t.register_extension(OffchainDbExt::new(offchain));
// 	t.register_extension(TransactionPoolExt::new(pool));
// 	t.register_extension(KeystoreExt(Arc::new(keystore)));

// 	let multiaddr_vec = "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWMvyvKxYcy9mjbFbXcogFSCvENzQ62ogRxHKZaksFCkAp".as_bytes().to_vec();
// 	let cid_vec = "QmPZv7P8nQUSh2CpqTvUeYemFyjvMjgWEs8H1Tm8b3zAm9".as_bytes().to_vec();
// 	let bytes = "hello test".as_bytes().to_vec();
// 	let name: Vec<u8> = "test.txt".as_bytes().to_vec();
// 	let id = 1;
// 	let balance = 1;
// 	// mock IPFS calls
// 	{	
// 		let mut state = state.write();
// 		// connect to external node
// 		state.expect_ipfs_request(testing::IpfsPendingRequest {
// 			response: Some(IpfsResponse::Success),
// 			..Default::default()
// 		});
// 		// fetch data
// 		state.expect_ipfs_request(testing::IpfsPendingRequest {
// 			id: sp_core::offchain::IpfsRequestId(0),
// 			response: Some(IpfsResponse::CatBytes(bytes.clone())),
// 			..Default::default()
// 		});
// 		// disconnect from the external node
// 		state.expect_ipfs_request(testing::IpfsPendingRequest {
// 			response: Some(IpfsResponse::Success),
// 			..Default::default()
// 		});
// 		// add bytes to your local node 
// 		state.expect_ipfs_request(testing::IpfsPendingRequest {
// 			response: Some(IpfsResponse::AddBytes(cid_vec.clone())),
// 			..Default::default()
// 		});
// 		// fetch data
// 		state.expect_ipfs_request(testing::IpfsPendingRequest {
// 			id: sp_core::offchain::IpfsRequestId(0),
// 			response: Some(IpfsResponse::CatBytes(bytes.clone())),
// 			..Default::default()
// 		});
// 	}

// 	t.execute_with(|| {
// 		// WHEN: I invoke the create_storage_assets extrinsic
// 		assert_ok!(Iris::create_storage_asset(
// 			Origin::signed(p.clone().public()),
// 			p.clone().public(),
// 			multiaddr_vec.clone(),
// 			cid_vec.clone(),
// 			name.clone(),
// 			id.clone(),
// 			balance.clone(),
// 		));
// 		// AND: I create an owned asset class
// 		assert_ok!(Iris::submit_ipfs_add_results(
// 			Origin::signed(p.clone().public()),
// 			p.clone().public(),
// 			cid_vec.clone(),
// 			id.clone(),
// 			balance.clone(),
// 		));
// 		// AND: I invoke the mint_tickets extrinsic
// 		assert_ok!(Iris::mint_tickets(
// 			Origin::signed(p.clone().public()),
// 			p.clone().public(),
// 			cid_vec.clone(),
// 			balance.clone(),
// 		));
// 		// AND: I request the owned content from iris
// 		assert_ok!(Iris::request_data(
// 			Origin::signed(p.clone().public()),
// 			p.clone().public(),
// 			cid_vec.clone(),
// 		));
// 		// THEN: the offchain worker adds data to IPFS
// 		assert_ok!(Iris::handle_data_requests());
// 		// AND: The data is available in local offchain storage
// 	});	
// }
