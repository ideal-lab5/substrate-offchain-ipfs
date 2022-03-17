use super::*;
use frame_support::{assert_ok};
use mock::*;
use sp_core::Pair;
// use sp_core::{
// 	offchain::{testing, OffchainWorkerExt, TransactionPoolExt, OffchainDbExt}
// };
// use sp_keystore::{testing::KeyStore, KeystoreExt, SyncCryptoStore};
// use std::sync::Arc;

#[test]
fn iris_assets_initial_state() {
	new_test_ext().execute_with(|| {
		// Given: The node is initialized at block 0
		// When: I query runtime storagey
		let data_queue = crate::DataQueue::<Test>::get();
		let len = data_queue.len();
		// Then: Runtime storage is empty
		assert_eq!(len, 0);
	});
}

#[test]
fn iris_assets_ipfs_add_bytes_works_for_valid_value() {
	// Given: I am a valid node with a positive balance
	let (p, _) = sp_core::sr25519::Pair::generate();
	let pairs = vec![(p.clone().public(), 10)];
	let multiaddr_vec = "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWMvyvKxYcy9mjbFbXcogFSCvENzQ62ogRxHKZaksFCkAp".as_bytes().to_vec();
	let cid_vec = "QmPZv7P8nQUSh2CpqTvUeYemFyjvMjgWEs8H1Tm8b3zAm9".as_bytes().to_vec();
	let name: Vec<u8> = "test.txt".as_bytes().to_vec();
	let id = 1;
	let balance = 1;

	// 
	let expected_data_command = crate::DataCommand::AddBytes(
		OpaqueMultiaddr(multiaddr_vec.clone()),
		cid_vec.clone(),
		p.clone().public(),
		name.clone(),
		id.clone(),
		balance.clone(),
	);

	new_test_ext_funded(pairs).execute_with(|| {
		// WHEN: I invoke the create_storage_assets extrinsic
		assert_ok!(Iris::create(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			multiaddr_vec.clone(),
			cid_vec.clone(),
			name.clone(),
			id.clone(),
			balance.clone(),
		));

		// THEN: There is a single DataCommand::AddBytes in the DataQueue
		let mut data_queue = crate::DataQueue::<Test>::get();
		let len = data_queue.len();
		assert_eq!(len, 1);
		let actual_data_command = data_queue.pop();
		assert_eq!(actual_data_command, Some(expected_data_command));
	});
}

#[test]
fn iris_assets_request_data_works_for_valid_values() {
	// GIVEN: I am a valid Iris node with a positive balance
	let (p, _) = sp_core::sr25519::Pair::generate();
	let pairs = vec![(p.clone().public(), 10)];
	// let cid_vec = "QmPZv7P8nQUSh2CpqTvUeYemFyjvMjgWEs8H1Tm8b3zAm9".as_bytes().to_vec();
	let asset_id = 1;
	let expected_data_command = crate::DataCommand::CatBytes(
		p.clone().public(),
		p.clone().public(),
		asset_id.clone(),
	);
	new_test_ext_funded(pairs).execute_with(|| {
		// WHEN: I invoke the request_data extrinsic
		assert_ok!(Iris::request_bytes(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			asset_id.clone(),
		));

		// THEN: There should be a single DataCommand::CatBytes in the DataQueue
		let mut data_queue = crate::DataQueue::<Test>::get();
		let len = data_queue.len();
		assert_eq!(len, 1);
		let actual_data_command = data_queue.pop();
		assert_eq!(actual_data_command, Some(expected_data_command));
	});
}

#[test]
fn iris_assets_submit_ipfs_add_results_works_for_valid_values() {
	// GIVEN: I am a valid Iris node with a positive valance
	let (p, _) = sp_core::sr25519::Pair::generate();
	let pairs = vec![(p.clone().public(), 10)];
	let cid_vec = "QmPZv7P8nQUSh2CpqTvUeYemFyjvMjgWEs8H1Tm8b3zAm9".as_bytes().to_vec();
	let id = 1;
	let balance = 1;

	new_test_ext_funded(pairs).execute_with(|| {
		// WHEN: I invoke the submit_ipfs_add_results extrinsic
		assert_ok!(Iris::submit_ipfs_add_results(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			cid_vec.clone(),
			id.clone(),
			balance.clone(),
		));

		// THEN: a new asset class is created
		// AND: A new entry is added to the AssetClassOwnership StorageDoubleMap
		let asset_class_owner = crate::AssetClassOwnership::<Test>::get(id.clone());
		assert_eq!(asset_class_owner, p.clone().public());
	});
}

#[test]
fn iris_assets_mint_tickets_works_for_valid_values() {
	// GIVEN: I am a valid Iris node with a positive valance
	let (p, _) = sp_core::sr25519::Pair::generate();
	let pairs = vec![(p.clone().public(), 10)];
	let cid_vec = "QmPZv7P8nQUSh2CpqTvUeYemFyjvMjgWEs8H1Tm8b3zAm9".as_bytes().to_vec();
	let balance = 1;
	let id = 1;

	new_test_ext_funded(pairs).execute_with(|| {
		// AND: I create an owned asset class
		assert_ok!(Iris::submit_ipfs_add_results(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			cid_vec.clone(),
			id.clone(),
			balance.clone(),
		));
		// WHEN: I invoke the mint_tickets extrinsic
		assert_ok!(Iris::mint(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			id.clone(),
			balance.clone(),
		));
		// THEN: new assets are created and awarded to the benficiary
		// AND: A new entry is added to the AssetAccess StorageDoubleMap
		let asset_class_owner = crate::AssetAccess::<Test>::get(p.clone().public(), id.clone());
		assert_eq!(asset_class_owner, p.clone().public())
	});
}

#[test]
fn iris_assets_can_transer_assets() {
	// GIVEN: I am  valid Iris node with a positive balance	// GIVEN: I am a valid Iris node with a positive valance
	let (p, _) = sp_core::sr25519::Pair::generate();
	let (p2, _) = sp_core::sr25519::Pair::generate();
	let pairs = vec![(p.clone().public(), 10), (p2.clone().public(), 10)];
	let cid_vec = "QmPZv7P8nQUSh2CpqTvUeYemFyjvMjgWEs8H1Tm8b3zAm9".as_bytes().to_vec();
	let balance = 1;
	let id = 1;

	new_test_ext_funded(pairs).execute_with(|| {
		// AND: I create an owned asset class
		assert_ok!(Iris::submit_ipfs_add_results(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			cid_vec.clone(),
			id.clone(),
			balance.clone(),
		));
		// WHEN: I invoke the mint_tickets extrinsic
		assert_ok!(Iris::mint(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			id.clone(),
			balance.clone(),
		));
		// THEN: new assets are created and awarded to the benficiary
		// AND: A new entry is added to the AssetAccess StorageDoubleMap
		let asset_class_owner = crate::AssetAccess::<Test>::get(p.clone().public(), id.clone());
		assert_eq!(asset_class_owner, p.clone().public());
		// THEN: I can transfer my owned asset to another address
		assert_ok!(Iris::transfer_asset(
			Origin::signed(p.clone().public()),
			p2.clone().public(),
			id.clone(),
			balance.clone(),
		));
	});
}

#[test]
fn iris_assets_can_burn_asset() {
	// Given: I am a valid node with a positive balance
	let (p, _) = sp_core::sr25519::Pair::generate();
	let pairs = vec![(p.clone().public(), 10)];
	let cid_vec = "QmPZv7P8nQUSh2CpqTvUeYemFyjvMjgWEs8H1Tm8b3zAm9".as_bytes().to_vec();
	let id = 1;
	let balance = 2;

	new_test_ext_funded(pairs).execute_with(|| {
		// GIVEN: I create an asset class
		assert_ok!(Iris::submit_ipfs_add_results(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			cid_vec.clone(),
			id.clone(),
			balance.clone(),
		));
		// AND: I mint some assets
		assert_ok!(Iris::mint(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			id.clone(),
			balance.clone(),
		));
		// WHEN: I burn 1 asset
		assert_ok!(Iris::burn(
			Origin::signed(p.clone().public()),
			p.clone().public(),
			id.clone(),
			1,
		));
	});	
}
