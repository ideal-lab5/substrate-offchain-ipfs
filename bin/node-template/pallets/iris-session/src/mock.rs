//! Mock helpers for Validator Set pallet.

#![cfg(test)]

use super::*;
use crate::{self as pallet_iris_session, Config};
use pallet_iris_assets;
use frame_support::{parameter_types, traits::GenesisBuild, BasicExternalities};
use frame_system::EnsureRoot;
use pallet_session::*;
use sp_runtime::{
	impl_opaque_keys,
	testing::{Header, UintAuthorityId, TestXt},
	traits::{BlakeTwo256, IdentityLookup, OpaqueKeys, IdentifyAccount, Verify, Extrinsic as ExtrinsicT},
	KeyTypeId, RuntimeAppPublic, Perbill,
};
use sp_core::{
	crypto::key_types::DUMMY,
	offchain::{testing, OffchainWorkerExt, TransactionPoolExt},
	sr25519::Signature,
	H256,
	Pair,
};
use sp_keystore::{testing::KeyStore, KeystoreExt, SyncCryptoStore};

use std::cell::RefCell;
use std::sync::Arc;

impl_opaque_keys! {
	pub struct MockSessionKeys {
		pub dummy: UintAuthorityId,
	}
}

impl From<UintAuthorityId> for MockSessionKeys {
	fn from(dummy: UintAuthorityId) -> Self {
		Self { dummy }
	}
}

pub const KEY_ID_A: KeyTypeId = KeyTypeId([4; 4]);
pub const KEY_ID_B: KeyTypeId = KeyTypeId([9; 4]);

#[derive(Debug, Clone, codec::Encode, codec::Decode, PartialEq, Eq)]
pub struct PreUpgradeMockSessionKeys {
	pub a: [u8; 32],
	pub b: [u8; 64],
}

impl OpaqueKeys for PreUpgradeMockSessionKeys {
	type KeyTypeIdProviders = ();

	fn key_ids() -> &'static [KeyTypeId] {
		&[KEY_ID_A, KEY_ID_B]
	}

	fn get_raw(&self, i: KeyTypeId) -> &[u8] {
		match i {
			i if i == KEY_ID_A => &self.a[..],
			i if i == KEY_ID_B => &self.b[..],
			_ => &[],
		}
	}
}

type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Test>;
type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
	pub enum Test where
		Block = Block,
		NodeBlock = Block,
		UncheckedExtrinsic = UncheckedExtrinsic,
	{
		System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
		Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
		Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
		Assets: pallet_assets::{Pallet, Storage, Event<T>},
		IrisAssets: pallet_iris_assets::{Pallet, Call, Storage, Event<T>},
		IrisSession: pallet_iris_session::{Pallet, Call, Storage, Event<T>, Config<T>},
	}
);

// pub static V0: (sp_core::sr25519::Public, UintAuthorityId) = (sp_core::sr25519::Pair::generate_with_phrase(Some("0")).0.public(), UintAuthorityId(0));
// pub static V1: (sp_core::sr25519::Public, UintAuthorityId) = (sp_core::sr25519::Pair::generate_with_phrase(Some("1")).0.public(), UintAuthorityId(1));
// pub static V2: (sp_core::sr25519::Public, UintAuthorityId) = (sp_core::sr25519::Pair::generate_with_phrase(Some("2")).0.public(), UintAuthorityId(2));

thread_local! {
	pub static VALIDATORS: RefCell<Vec<(sp_core::sr25519::Public, UintAuthorityId)>> = RefCell::new(
		vec![(sp_core::sr25519::Pair::generate_with_phrase(Some("0")).0.public(), UintAuthorityId(0)),
		(sp_core::sr25519::Pair::generate_with_phrase(Some("1")).0.public(), UintAuthorityId(1)),
		(sp_core::sr25519::Pair::generate_with_phrase(Some("2")).0.public(), UintAuthorityId(2))]);
	pub static NEXT_VALIDATORS: RefCell<Vec<(sp_core::sr25519::Public, UintAuthorityId)>> = RefCell::new(
		vec![(sp_core::sr25519::Pair::generate_with_phrase(Some("0")).0.public(), UintAuthorityId(0)),
		(sp_core::sr25519::Pair::generate_with_phrase(Some("1")).0.public(), UintAuthorityId(1)),
		(sp_core::sr25519::Pair::generate_with_phrase(Some("2")).0.public(), UintAuthorityId(2))]);
	pub static AUTHORITIES: RefCell<Vec<UintAuthorityId>> =
		RefCell::new(vec![UintAuthorityId(1), UintAuthorityId(2), UintAuthorityId(3)]);
	pub static FORCE_SESSION_END: RefCell<bool> = RefCell::new(false);
	pub static SESSION_LENGTH: RefCell<u64> = RefCell::new(2);
	pub static SESSION_CHANGED: RefCell<bool> = RefCell::new(false);
	pub static DISABLED: RefCell<bool> = RefCell::new(false);
	pub static BEFORE_SESSION_END_CALLED: RefCell<bool> = RefCell::new(false);
}

pub struct TestSessionHandler;
impl pallet_session::SessionHandler<sp_core::sr25519::Public> for TestSessionHandler {
	const KEY_TYPE_IDS: &'static [sp_runtime::KeyTypeId] = &[UintAuthorityId::ID];
	fn on_genesis_session<T: OpaqueKeys>(_validators: &[(sp_core::sr25519::Public, T)]) {}
	fn on_new_session<T: OpaqueKeys>(
		changed: bool,
		validators: &[(sp_core::sr25519::Public, T)],
		_queued_validators: &[(sp_core::sr25519::Public, T)],
	) {
		SESSION_CHANGED.with(|l| *l.borrow_mut() = changed);
		AUTHORITIES.with(|l| {
			*l.borrow_mut() = validators
				.iter()
				.map(|(_, id)| id.get::<UintAuthorityId>(DUMMY).unwrap_or_default())
				.collect()
		});
	}
	fn on_disabled(_validator_index: usize) {
		DISABLED.with(|l| *l.borrow_mut() = true)
	}
	fn on_before_session_ending() {
		BEFORE_SESSION_END_CALLED.with(|b| *b.borrow_mut() = true);
	}
}

pub struct TestShouldEndSession;
impl ShouldEndSession<u64> for TestShouldEndSession {
	fn should_end_session(now: u64) -> bool {
		let l = SESSION_LENGTH.with(|l| *l.borrow());
		now % l == 0 ||
			FORCE_SESSION_END.with(|l| {
				let r = *l.borrow();
				*l.borrow_mut() = false;
				r
			})
	}
}

pub fn authorities() -> Vec<UintAuthorityId> {
	AUTHORITIES.with(|l| l.borrow().to_vec())
}

parameter_types! {
	pub const MinimumPeriod: u64 = 5;
	pub const BlockHashCount: u64 = 250;
	pub BlockWeights: frame_system::limits::BlockWeights =
		frame_system::limits::BlockWeights::simple_max(1024);
}

impl frame_system::Config for Test {
	type BaseCallFilter = frame_support::traits::Everything;
	type BlockWeights = ();
	type BlockLength = ();
	type DbWeight = ();
	type Origin = Origin;
	type Index = u64;
	type BlockNumber = u64;
	type Call = Call;
	type Hash = H256;
	type Hashing = BlakeTwo256;
	type AccountId = sp_core::sr25519::Public;
	// type AccountId = u64;
	type Lookup = IdentityLookup<Self::AccountId>;
	type Header = Header;
	type Event = Event;
	type BlockHashCount = BlockHashCount;
	type Version = ();
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<u64>;
	// type AccountData = sp_core::sr25519::Public;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type SystemWeightInfo = ();
	type SS58Prefix = ();
	type OnSetCode = ();
}

// SS58Prefix
parameter_types! {
	pub const ExistentialDeposit: u64 = 1;
}

impl pallet_balances::Config for Test {
	type MaxLocks = ();
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	type Balance = u64;
	type DustRemoval = ();
	type Event = Event;
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type WeightInfo = ();
}

// implement assets pallet for iris_assets 
parameter_types! {
	pub const AssetDeposit: u64 = 1;
	pub const ApprovalDeposit: u64 = 1;
	pub const StringLimit: u32 = 50;
	pub const MetadataDepositBase: u64 = 1;
	pub const MetadataDepositPerByte: u64 = 1;
}

impl pallet_assets::Config for Test {
	type Event = Event;
	type Balance = u64;
	type AssetId = u32;
	type Currency = Balances;
	type ForceOrigin = frame_system::EnsureRoot<sp_core::sr25519::Public>;
	// type ForceOrigin = EnsureRoot<u64>;
	type AssetDeposit = AssetDeposit;
	type MetadataDepositBase = MetadataDepositBase;
	type MetadataDepositPerByte = MetadataDepositPerByte;
	type ApprovalDeposit = ApprovalDeposit;
	type StringLimit = StringLimit;
	type Freezer = ();
	type WeightInfo = ();
	type Extra = ();
}


/// configure the iris assets pallet
impl pallet_iris_assets::Config for Test {
	type Event = Event;
	type Call = Call;
	type Currency = Balances;
}

parameter_types! {
	pub const MinAuthorities: u32 = 2;
	pub const MaxDeadSession: u32 = 3;
}

impl Config for Test {
	// type AddRemoveOrigin = ();
	type AddRemoveOrigin = EnsureRoot<sp_core::sr25519::Public>;
	type Call = Call;
	type AuthorityId = pallet_iris_session::crypto::TestAuthId;
	type Event = Event;
	type MinAuthorities = MinAuthorities;
	type MaxDeadSession = MaxDeadSession;
}

parameter_types! {
	pub const DisabledValidatorsThreshold: Perbill = Perbill::from_percent(33);
}

impl pallet_session::Config for Test {
	type ValidatorId = <Self as frame_system::Config>::AccountId;
	type ValidatorIdOf = pallet_iris_session::ValidatorOf<Self>;
	type DisabledValidatorsThreshold = DisabledValidatorsThreshold;
	type ShouldEndSession = TestShouldEndSession;
	type NextSessionRotation = ();
	type SessionManager = IrisSession;
	type SessionHandler = TestSessionHandler;
	type Keys = MockSessionKeys;
	type WeightInfo = ();
	type Event = Event;
}

type Extrinsic = TestXt<Call, ()>;
type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

impl frame_system::offchain::SigningTypes for Test {
	type Public = <Signature as Verify>::Signer;
	type Signature = Signature;
}

impl<LocalCall> frame_system::offchain::SendTransactionTypes<LocalCall> for Test
where
	Call: From<LocalCall>,
{
	type OverarchingCall = Call;
	type Extrinsic = Extrinsic;
}

impl<LocalCall> frame_system::offchain::CreateSignedTransaction<LocalCall> for Test
where
	Call: From<LocalCall>,
{
	fn create_transaction<C: frame_system::offchain::AppCrypto<Self::Public, Self::Signature>>(
		call: Call,
		_public: <Signature as Verify>::Signer,
		_account: AccountId,
		nonce: u64,
	) -> Option<(Call, <Extrinsic as ExtrinsicT>::SignaturePayload)> {
		Some((call, (nonce, ())))
	}
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap();
	let keys: Vec<_> = NEXT_VALIDATORS
		.with(|l| l.borrow().iter().cloned().map(|i| (i.0, i.0, i.1.into())).collect());
	BasicExternalities::execute_with_storage(&mut t, || {
		for (ref k, ..) in &keys {
			frame_system::Pallet::<Test>::inc_providers(k);
		}
		// frame_system::Pallet::<Test>::inc_providers(&4);
		// frame_system::Pallet::<Test>::inc_providers(&69);
	});

	pallet_iris_session::GenesisConfig::<Test> {
		initial_validators: keys.iter().map(|x| x.0).collect::<Vec<_>>(),
	}
	.assimilate_storage(&mut t)
	.unwrap();
	
	pallet_session::GenesisConfig::<Test> { keys: keys.clone() }
		.assimilate_storage(&mut t)
		.unwrap();

	let (pair1, _) = sp_core::sr25519::Pair::generate();
	let (pair2, _) = sp_core::sr25519::Pair::generate();
	let (pair3, _) = sp_core::sr25519::Pair::generate();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(pair1.public(), 10), (pair2.public(), 20), (pair3.public(), 30)],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	sp_io::TestExternalities::new(t)
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext_funded(pair1_funded: sp_core::sr25519::Pair) -> sp_io::TestExternalities {
	let mut t = frame_system::GenesisConfig::default().build_storage::<Test>().unwrap();
	let keys: Vec<_> = NEXT_VALIDATORS
		.with(|l| l.borrow().iter().cloned().map(|i| (i.0, i.0, i.1.into())).collect());
	BasicExternalities::execute_with_storage(&mut t, || {
		for (ref k, ..) in &keys {
			frame_system::Pallet::<Test>::inc_providers(k);
		}
		// frame_system::Pallet::<Test>::inc_providers(&4);
		// frame_system::Pallet::<Test>::inc_providers(&69);
	});

	pallet_iris_session::GenesisConfig::<Test> {
		initial_validators: keys.iter().map(|x| x.0).collect::<Vec<_>>(),
	}
	.assimilate_storage(&mut t)
	.unwrap();
	
	pallet_session::GenesisConfig::<Test> { keys: keys.clone() }
		.assimilate_storage(&mut t)
		.unwrap();

	let (pair2, _) = sp_core::sr25519::Pair::generate();
	let (pair3, _) = sp_core::sr25519::Pair::generate();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(pair1_funded.public(), 10), (pair2.public(), 20), (pair3.public(), 30)],
	}
	.assimilate_storage(&mut t)
	.unwrap();

	sp_io::TestExternalities::new(t)
}
