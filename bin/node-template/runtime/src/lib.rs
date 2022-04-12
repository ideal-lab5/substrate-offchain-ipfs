#![cfg_attr(not(feature = "std"), no_std)]
// `construct_runtime!` does a lot of recursion and requires us to increase the limit to 256.
#![recursion_limit = "256"]

// Make the WASM binary available.
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

use pallet_grandpa::{
	fg_primitives, AuthorityId as GrandpaId, AuthorityList as GrandpaAuthorityList,
};
use sp_api::impl_runtime_apis;
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::{crypto::KeyTypeId, OpaqueMetadata, Encode, Bytes};
use sp_runtime::{
	create_runtime_str, generic, impl_opaque_keys,
	traits::{
		AccountIdLookup,
		BlakeTwo256,
		Block as BlockT, 
		IdentifyAccount,
		NumberFor,
		Verify,
		SaturatedConversion,
		OpaqueKeys,
	 },
	transaction_validity::{TransactionSource, TransactionValidity, TransactionPriority},
	ApplyExtrinsicResult, MultiSignature,
};
use sp_std::prelude::*;
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;
use frame_system::{
	limits::{BlockLength, BlockWeights},
	EnsureRoot,
};
use pallet_im_online::sr25519::AuthorityId as ImOnlineId;
// use pallet_contracts::weights::WeightInfo;
use frame_support::traits::Nothing;

// A few exports that help ease life for downstream crates.
pub use frame_support::{
	construct_runtime, parameter_types,
	traits::{ConstU128, ConstU32, ConstU8, KeyOwnerProofSystem, Randomness, StorageInfo},
	debug,
	weights::{
		constants::{BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight, WEIGHT_PER_SECOND},
		IdentityFee, Weight, DispatchClass,
	},
	StorageValue,
};
pub use frame_system::Call as SystemCall;
pub use pallet_balances::Call as BalancesCall;
pub use pallet_timestamp::Call as TimestampCall;
pub use pallet_assets::Call as AssetsCall;
use pallet_transaction_payment::CurrencyAdapter;
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;
pub use sp_runtime::{Perbill, Permill};


/// Import the template pallet.
pub use pallet_iris_assets;
pub use pallet_iris_session;
pub use pallet_iris_ledger;

/// An index to a block.
pub type BlockNumber = u32;

/// Alias to 512-bit hash when used in the context of a transaction signature on the chain.
pub type Signature = MultiSignature;

/// Some way of identifying an account on the chain. We intentionally make it equivalent
/// to the public key of our transaction signing scheme.
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

/// Balance of an account.
// pub type Balance = u128;

/// Index of a transaction in the chain.
pub type Index = u32;

/// A hash of some data used by the chain.
pub type Hash = sp_core::H256;

pub const MILLICENTS: Balance = 1_000_000_000;
pub const CENTS: Balance = 1_000 * MILLICENTS; // assume this is worth about a cent.
pub const DOLLARS: Balance = 100 * CENTS;

/// Opaque types. These are used by the CLI to instantiate machinery that don't need to know
/// the specifics of the runtime. They can then be made to be agnostic over specific formats
/// of data like extrinsics, allowing for them to continue syncing the network through upgrades
/// to even the core data structures.
pub mod opaque {
	use super::*;

	pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;

	/// Opaque block header type.
	pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
	/// Opaque block type.
	pub type Block = generic::Block<Header, UncheckedExtrinsic>;
	/// Opaque block identifier type.
	pub type BlockId = generic::BlockId<Block>;

	impl_opaque_keys! {
		pub struct SessionKeys {
			pub aura: Aura,
			pub grandpa: Grandpa,
			pub im_online: ImOnline,
		}
	}
}

use node_primitives::Balance;

pub const fn deposit(items: u32, bytes: u32) -> Balance {
	items as Balance * 15 * CENTS + (bytes as Balance) * 6 * CENTS
}

// To learn more about runtime versioning and what each of the following value means:
//   https://docs.substrate.io/v3/runtime/upgrades#runtime-versioning
#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
	spec_name: create_runtime_str!("node-template"),
	impl_name: create_runtime_str!("node-template"),
	authoring_version: 1,
	// The version of the runtime specification. A full node will not attempt to use its native
	//   runtime in substitute for the on-chain Wasm runtime unless all of `spec_name`,
	//   `spec_version`, and `authoring_version` are the same between Wasm and native.
	// This value is set to 100 to notify Polkadot-JS App (https://polkadot.js.org/apps) to use
	//   the compatible custom types.
	spec_version: 100,
	impl_version: 1,
	apis: RUNTIME_API_VERSIONS,
	transaction_version: 1,
	state_version: 1,
};

/// This determines the average expected block time that we are targeting.
/// Blocks will be produced at a minimum duration defined by `SLOT_DURATION`.
/// `SLOT_DURATION` is picked up by `pallet_timestamp` which is in turn picked
/// up by `pallet_aura` to implement `fn slot_duration()`.
///
/// Change this to adjust the block time.
pub const MILLISECS_PER_BLOCK: u64 = 6000;

// NOTE: Currently it is not possible to change the slot duration after the chain has started.
//       Attempting to do so will brick block production.
pub const SLOT_DURATION: u64 = MILLISECS_PER_BLOCK;

// Time is measured by number of blocks.
pub const MINUTES: BlockNumber = 60_000 / (MILLISECS_PER_BLOCK as BlockNumber);
pub const HOURS: BlockNumber = MINUTES * 60;
pub const DAYS: BlockNumber = HOURS * 24;

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
	NativeVersion { runtime_version: VERSION, can_author_with: Default::default() }
}


/// We assume that ~10% of the block weight is consumed by `on_initialize` handlers.
/// This is used to limit the maximal weight of a single extrinsic.
const AVERAGE_ON_INITIALIZE_RATIO: Perbill = Perbill::from_percent(10);
/// We allow `Normal` extrinsics to fill up the block up to 75%, the rest can be used
/// by  Operational  extrinsics.
const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);
/// We allow for 2 seconds of compute with a 6 second average block time.
const MAXIMUM_BLOCK_WEIGHT: Weight = 2 * WEIGHT_PER_SECOND;

parameter_types! {
	pub const BlockHashCount: BlockNumber = 2400;
	pub const Version: RuntimeVersion = VERSION;
	pub RuntimeBlockLength: BlockLength =
		BlockLength::max_with_normal_ratio(5 * 1024 * 1024, NORMAL_DISPATCH_RATIO);
	pub RuntimeBlockWeights: BlockWeights = BlockWeights::builder()
		.base_block(BlockExecutionWeight::get())
		.for_class(DispatchClass::all(), |weights| {
			weights.base_extrinsic = ExtrinsicBaseWeight::get();
		})
		.for_class(DispatchClass::Normal, |weights| {
			weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT);
		})
		.for_class(DispatchClass::Operational, |weights| {
			weights.max_total = Some(MAXIMUM_BLOCK_WEIGHT);
			// Operational transactions have some extra reserved space, so that they
			// are included even if block reached `MAXIMUM_BLOCK_WEIGHT`.
			weights.reserved = Some(
				MAXIMUM_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT
			);
		})
		.avg_block_initialization(AVERAGE_ON_INITIALIZE_RATIO)
		.build_or_panic();
	pub const SS58Prefix: u16 = 42;
}

// Configure FRAME pallets to include in runtime.

impl frame_system::Config for Runtime {
	/// The basic call filter to use in dispatchable.
	type BaseCallFilter = frame_support::traits::Everything;
	/// Block & extrinsics weights: base values and limits.
	type BlockWeights = RuntimeBlockWeights;
	/// The maximum length of a block (in bytes).
	type BlockLength = RuntimeBlockLength;
	/// The identifier used to distinguish between accounts.
	type AccountId = AccountId;
	// type AccountId = From<[u8; 32]>;
	/// The aggregated dispatch type that is available for extrinsics.
	type Call = Call;
	/// The lookup mechanism to get account ID from whatever is passed in dispatchers.
	type Lookup = AccountIdLookup<AccountId, ()>;
	/// The index type for storing how many extrinsics an account has signed.
	type Index = Index;
	/// The index type for blocks.
	type BlockNumber = BlockNumber;
	/// The type for hashing blocks and tries.
	type Hash = Hash;
	/// The hashing algorithm used.
	type Hashing = BlakeTwo256;
	/// The header type.
	type Header = generic::Header<BlockNumber, BlakeTwo256>;
	/// The ubiquitous event type.
	type Event = Event;
	/// The ubiquitous origin type.
	type Origin = Origin;
	/// Maximum number of block number to block hash mappings to keep (oldest pruned first).
	type BlockHashCount = BlockHashCount;
	/// The weight of database operations that the runtime can invoke.
	type DbWeight = RocksDbWeight;
	/// Version of the runtime.
	type Version = Version;
	/// Converts a module to the index of the module in `construct_runtime!`.
	///
	/// This type is being generated by `construct_runtime!`.
	type PalletInfo = PalletInfo;
	/// What to do if a new account is created.
	type OnNewAccount = ();
	/// What to do if an account is fully reaped from the system.
	type OnKilledAccount = ();
	/// The data to be stored in an account.
	type AccountData = pallet_balances::AccountData<Balance>;
	/// Weight information for the extrinsics of this pallet.
	type SystemWeightInfo = ();
	/// This is used as an identifier of the chain. 42 is the generic substrate prefix.
	type SS58Prefix = SS58Prefix;
	/// The set code logic, just the default since we're not a parachain.
	type OnSetCode = ();
	type MaxConsumers = frame_support::traits::ConstU32<16>;
}

impl pallet_randomness_collective_flip::Config for Runtime {}

// parameter_types! {
// 	pub ContractDeposit: Balance = deposit(
// 		1,
// 		<pallet_contracts::Pallet<Runtime>>::contract_info_size(),
// 	);
// 	pub const MaxValueSize: u32 = 16 * 1024;
// 	// The lazy deletion runs inside on_initialize.
// 	pub DeletionWeightLimit: Weight = AVERAGE_ON_INITIALIZE_RATIO *
// 		RuntimeBlockWeights::get().max_block;
// 	// The weight needed for decoding the queue should be less or equal than a fifth
// 	// of the overall weight dedicated to the lazy deletion.
// 	pub DeletionQueueDepth: u32 = ((DeletionWeightLimit::get() / (
// 			<Runtime as pallet_contracts::Config>::WeightInfo::on_initialize_per_queue_item(1) -
// 			<Runtime as pallet_contracts::Config>::WeightInfo::on_initialize_per_queue_item(0)
// 		)) / 5) as u32;
// 	pub Schedule: pallet_contracts::Schedule<Runtime> = Default::default();
// 	pub const DEPOSIT_PER_ITEM = 1;
// }

// impl pallet_contracts::Config for Runtime {
// 	type Time = Timestamp;
// 	type Randomness = RandomnessCollectiveFlip;
// 	type Currency = Balances;
// 	type Event = Event;
// 	type Call = Call;
// 	/// The safest default is to allow no calls at all.
// 	///
// 	/// Runtimes should whitelist dispatchables that are allowed to be called from contracts
// 	/// and make sure they are stable. Dispatchables exposed to contracts are not allowed to
// 	/// change because that would break already deployed contracts. The `Call` structure itself
// 	/// is not allowed to change the indices of existing pallets, too.
// 	type CallFilter = Nothing;
// 	// type ContractDeposit = ContractDeposit;
// 	type CallStack = [pallet_contracts::Frame<Self>; 31];
// 	type WeightPrice = pallet_transaction_payment::Pallet<Self>;
// 	type WeightInfo = pallet_contracts::weights::SubstrateWeight<Self>;
// 	type ChainExtension = IrisExtension;
// 	type DeletionQueueDepth = DeletionQueueDepth;
// 	type DeletionWeightLimit = DeletionWeightLimit;
// 	type Schedule = Schedule;
// 	type DepositPerByte = ContractDeposit;
// 	type DepositPerItem = DEPOSIT_PER_ITEM;
// 	type AddressGenerator = ();
// }

parameter_types! {
	// TODO: Increase this when done testing
	pub const MinAuthorities: u32 = 1;
	pub const MaxDeadSession: u32 = 3;
}

impl pallet_iris_session::Config for Runtime {
	type Event = Event;
	type Call = Call;
	type AddRemoveOrigin = EnsureRoot<AccountId>;
	type MinAuthorities = MinAuthorities;
	type MaxDeadSession = MaxDeadSession;
	type AuthorityId = pallet_iris_session::crypto::TestAuthId;
}

parameter_types! {
	pub const Period: u32 = 1 * MINUTES;
	pub const Offset: u32 = 0;
}

impl pallet_session::Config for Runtime {
	type ValidatorId = <Self as frame_system::Config>::AccountId;
	type ValidatorIdOf = pallet_iris_session::ValidatorOf<Self>;
	type ShouldEndSession = pallet_session::PeriodicSessions<Period, Offset>;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type SessionManager = IrisSession;
	type SessionHandler = <opaque::SessionKeys as OpaqueKeys>::KeyTypeIdProviders;
	type Keys = opaque::SessionKeys;
	type WeightInfo = pallet_session::weights::SubstrateWeight<Runtime>;
	// type DisabledValidatorsThreshold = ();
	type Event = Event;
}

parameter_types! {
	pub const ImOnlineUnsignedPriority: TransactionPriority = TransactionPriority::max_value();
	pub const MaxKeys: u32 = 10_000;
	pub const MaxPeerInHeartbeats: u32 = 10_000;
	pub const MaxPeerDataEncodingSize: u32 = 1_000;
}

impl pallet_im_online::Config for Runtime {
	type AuthorityId = ImOnlineId;
	type Event = Event;
	type NextSessionRotation = pallet_session::PeriodicSessions<Period, Offset>;
	type ValidatorSet = IrisSession;
	type ReportUnresponsiveness = IrisSession;
	type UnsignedPriority = ImOnlineUnsignedPriority;
	type WeightInfo = pallet_im_online::weights::SubstrateWeight<Runtime>;
	type MaxKeys = MaxKeys;
	type MaxPeerInHeartbeats = MaxPeerInHeartbeats;
	type MaxPeerDataEncodingSize = MaxPeerDataEncodingSize;
}

parameter_types! {
	pub const MaxAuthorities: u32 = 32;
}

impl pallet_aura::Config for Runtime {
	type AuthorityId = AuraId;
	type DisabledValidators = ();
	type MaxAuthorities = ConstU32<32>;
}

impl pallet_grandpa::Config for Runtime {
	type Event = Event;
	type Call = Call;

	type KeyOwnerProofSystem = ();

	type KeyOwnerProof =
		<Self::KeyOwnerProofSystem as KeyOwnerProofSystem<(KeyTypeId, GrandpaId)>>::Proof;

	type KeyOwnerIdentification = <Self::KeyOwnerProofSystem as KeyOwnerProofSystem<(
		KeyTypeId,
		GrandpaId,
	)>>::IdentificationTuple;

	type HandleEquivocation = ();

	type WeightInfo = ();
	type MaxAuthorities = ConstU32<32>;
}

parameter_types! {
	pub const MinimumPeriod: u64 = SLOT_DURATION / 2;
}

impl pallet_timestamp::Config for Runtime {
	/// A timestamp: milliseconds since the unix epoch.
	type Moment = u64;
	type OnTimestampSet = Aura;
	type MinimumPeriod = MinimumPeriod;
	type WeightInfo = ();
}

impl pallet_balances::Config for Runtime {
	type MaxLocks = ConstU32<50>;
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	/// The type for recording an account's balance.
	type Balance = Balance;
	/// The ubiquitous event type.
	type Event = Event;
	type DustRemoval = ();
	type ExistentialDeposit = ConstU128<500>;
	type AccountStore = System;
	type WeightInfo = pallet_balances::weights::SubstrateWeight<Runtime>;
}



parameter_types! {
	pub const TransactionByteFee: Balance = 1;
}

impl pallet_transaction_payment::Config for Runtime {
	type OnChargeTransaction = CurrencyAdapter<Balances, ()>;
	type OperationalFeeMultiplier = ConstU8<5>;
	type WeightToFee = IdentityFee<Balance>;
	type LengthToFee = IdentityFee<Balance>;
	type FeeMultiplierUpdate = ();
}

parameter_types! {
	pub const AssetDeposit: Balance = 100 * DOLLARS;
	// TODO: What is the proper amount for this?
	pub const AssetAccountDeposit: Balance = 1 * DOLLARS;
	pub const ApprovalDeposit: Balance = 1 * DOLLARS;
	pub const StringLimit: u32 = 50;
	pub const MetadataDepositBase: Balance = 10 * DOLLARS;
	pub const MetadataDepositPerByte: Balance = 1 * DOLLARS;
}

impl pallet_sudo::Config for Runtime {
	type Event = Event;
	type Call = Call;
}

impl pallet_assets::Config for Runtime {
	type Event = Event;
	type Balance = u64;
	type AssetId = u32;
	type Currency = Balances;
	type ForceOrigin = EnsureRoot<AccountId>;
	type AssetDeposit = AssetDeposit;
	type AssetAccountDeposit = AssetAccountDeposit;
	type MetadataDepositBase = MetadataDepositBase;
	type MetadataDepositPerByte = MetadataDepositPerByte;
	type ApprovalDeposit = ApprovalDeposit;
	type StringLimit = StringLimit;
	type Freezer = ();
	type Extra = ();
	type WeightInfo = pallet_assets::weights::SubstrateWeight<Runtime>;
}

/// Payload data to be signed when making signed transaction from off-chain workers,
///   inside `create_transaction` function.
pub type SignedPayload = generic::SignedPayload<Call, SignedExtra>;

/// configure the iris assets pallet
impl pallet_iris_assets::Config for Runtime {
	type Event = Event;
	type Call = Call;
	type Currency = Balances;
}

impl pallet_iris_ledger::Config for Runtime {
	type Event = Event;
	type Call = Call;
	// type Balance = u64;
	type IrisCurrency = Balances;
}

impl<LocalCall> frame_system::offchain::CreateSignedTransaction<LocalCall> for Runtime
where
	Call: From<LocalCall>,
{
	fn create_transaction<C: frame_system::offchain::AppCrypto<Self::Public, Self::Signature>>(
		call: Call,
		public: <Signature as sp_runtime::traits::Verify>::Signer,
		account: AccountId,
		index: Index,
	) -> Option<(
		Call,
		<UncheckedExtrinsic as sp_runtime::traits::Extrinsic>::SignaturePayload,
	)> {
		let period = BlockHashCount::get() as u64;
		let current_block = System::block_number()
			.saturated_into::<u64>()
			.saturating_sub(1);
		let tip = 0;
		let extra: SignedExtra = (
			frame_system::CheckNonZeroSender::<Runtime>::new(),
			frame_system::CheckSpecVersion::<Runtime>::new(),
			frame_system::CheckTxVersion::<Runtime>::new(),
			frame_system::CheckGenesis::<Runtime>::new(),
			frame_system::CheckEra::<Runtime>::from(generic::Era::mortal(period, current_block)),
			frame_system::CheckNonce::<Runtime>::from(index),
			frame_system::CheckWeight::<Runtime>::new(),
			pallet_transaction_payment::ChargeTransactionPayment::<Runtime>::from(tip),
		);

		#[cfg_attr(not(feature = "std"), allow(unused_variables))]
		let raw_payload = SignedPayload::new(call, extra)
			.map_err(|e| {
				log::warn!("SignedPayload error: {:?}", e);
			})
			.ok()?;

		let signature = raw_payload.using_encoded(|payload| C::sign(payload, public))?;
		let address = account;
		let (call, extra, _) = raw_payload.deconstruct();
		Some((call, (sp_runtime::MultiAddress::Id(address), signature, extra)))
	}
}

impl frame_system::offchain::SigningTypes for Runtime {
	type Public = <Signature as sp_runtime::traits::Verify>::Signer;
	type Signature = Signature;
}

impl<C> frame_system::offchain::SendTransactionTypes<C> for Runtime
where
	Call: From<C>,
{
	type OverarchingCall = Call;
	type Extrinsic = UncheckedExtrinsic;
}

// Create the runtime by composing the FRAME pallets that were previously configured.
construct_runtime!(
	pub enum Runtime where
		Block = Block,
		NodeBlock = opaque::Block,
		UncheckedExtrinsic = UncheckedExtrinsic
	{
		System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
		Timestamp: pallet_timestamp::{Pallet, Call, Storage, Inherent},
		Balances: pallet_balances::{Pallet, Call, Storage, Config<T>, Event<T>},
		TransactionPayment: pallet_transaction_payment::{Pallet, Storage},
		Sudo: pallet_sudo::{Pallet, Call, Config<T>, Storage, Event<T>},
		IrisAssets: pallet_iris_assets::{Pallet, Call, Storage, Event<T>},
		IrisSession: pallet_iris_session::{Pallet, Call, Storage, Event<T>, Config<T>, ValidateUnsigned},
		IrisLedger: pallet_iris_ledger::{Pallet, Call, Storage, Event<T>},
		Session: pallet_session::{Pallet, Call, Storage, Event, Config<T>},
		Assets: pallet_assets::{Pallet, Storage, Event<T>},
		ImOnline: pallet_im_online::{Pallet, Call, Storage, Event<T>, ValidateUnsigned, Config<T>},
		RandomnessCollectiveFlip: pallet_randomness_collective_flip::{Pallet, Storage},
		// Contracts: pallet_contracts::{Pallet, Call, Storage, Event<T>},
		Aura: pallet_aura::{Pallet, Config<T>},
		Grandpa: pallet_grandpa::{Pallet, Call, Storage, Config, Event},
	}
);

/// The address format for describing accounts.
pub type Address = sp_runtime::MultiAddress<AccountId, ()>;
/// Block header type as expected by this runtime.
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;
/// The SignedExtension to the basic transaction logic.
pub type SignedExtra = (
	frame_system::CheckNonZeroSender<Runtime>,
	frame_system::CheckSpecVersion<Runtime>,
	frame_system::CheckTxVersion<Runtime>,
	frame_system::CheckGenesis<Runtime>,
	frame_system::CheckEra<Runtime>,
	frame_system::CheckNonce<Runtime>,
	frame_system::CheckWeight<Runtime>,
	pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
);
/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic = generic::UncheckedExtrinsic<Address, Call, Signature, SignedExtra>;
/// The payload being signed in transactions.
// pub type SignedPayload = generic::SignedPayload<Call, SignedExtra>;
/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
	Runtime,
	Block,
	frame_system::ChainContext<Runtime>,
	Runtime,
	AllPalletsWithSystem,
>;

#[cfg(feature = "runtime-benchmarks")]
#[macro_use]
extern crate frame_benchmarking;

#[cfg(feature = "runtime-benchmarks")]
mod benches {
	define_benchmarks!(
		[frame_benchmarking, BaselineBench::<Runtime>]
		[frame_system, SystemBench::<Runtime>]
		[pallet_balances, Balances]
		[pallet_timestamp, Timestamp]
		[pallet_template, TemplateModule]
	);
}

impl_runtime_apis! {
	impl sp_api::Core<Block> for Runtime {
		fn version() -> RuntimeVersion {
			VERSION
		}

		fn execute_block(block: Block) {
			Executive::execute_block(block);
		}

		fn initialize_block(header: &<Block as BlockT>::Header) {
			Executive::initialize_block(header)
		}
	}

	impl sp_api::Metadata<Block> for Runtime {
		fn metadata() -> OpaqueMetadata {
			OpaqueMetadata::new(Runtime::metadata().into())
		}
	}

	impl sp_block_builder::BlockBuilder<Block> for Runtime {
		fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
			Executive::apply_extrinsic(extrinsic)
		}

		fn finalize_block() -> <Block as BlockT>::Header {
			Executive::finalize_block()
		}

		fn inherent_extrinsics(data: sp_inherents::InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
			data.create_extrinsics()
		}

		fn check_inherents(
			block: Block,
			data: sp_inherents::InherentData,
		) -> sp_inherents::CheckInherentsResult {
			data.check_extrinsics(&block)
		}
	}

	impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
		fn validate_transaction(
			source: TransactionSource,
			tx: <Block as BlockT>::Extrinsic,
			block_hash: <Block as BlockT>::Hash,
		) -> TransactionValidity {
			Executive::validate_transaction(source, tx, block_hash)
		}
	}

	impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
		fn offchain_worker(header: &<Block as BlockT>::Header) {
			Executive::offchain_worker(header)
		}
	}

	impl sp_consensus_aura::AuraApi<Block, AuraId> for Runtime {
		fn slot_duration() -> sp_consensus_aura::SlotDuration {
			sp_consensus_aura::SlotDuration::from_millis(Aura::slot_duration())
		}

		fn authorities() -> Vec<AuraId> {
			Aura::authorities().into_inner()
		}
	}

	impl sp_session::SessionKeys<Block> for Runtime {
		fn generate_session_keys(seed: Option<Vec<u8>>) -> Vec<u8> {
			opaque::SessionKeys::generate(seed)
		}

		fn decode_session_keys(
			encoded: Vec<u8>,
		) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
			opaque::SessionKeys::decode_into_raw_public_keys(&encoded)
		}
	}

	impl fg_primitives::GrandpaApi<Block> for Runtime {
		fn grandpa_authorities() -> GrandpaAuthorityList {
			Grandpa::grandpa_authorities()
		}

		fn current_set_id() -> fg_primitives::SetId {
			Grandpa::current_set_id()
		}

		fn submit_report_equivocation_unsigned_extrinsic(
			_equivocation_proof: fg_primitives::EquivocationProof<
				<Block as BlockT>::Hash,
				NumberFor<Block>,
			>,
			_key_owner_proof: fg_primitives::OpaqueKeyOwnershipProof,
		) -> Option<()> {
			None
		}

		fn generate_key_ownership_proof(
			_set_id: fg_primitives::SetId,
			_authority_id: GrandpaId,
		) -> Option<fg_primitives::OpaqueKeyOwnershipProof> {
			// NOTE: this is the only implementation possible since we've
			// defined our key owner proof type as a bottom type (i.e. a type
			// with no values).
			None
		}
	}

	impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Index> for Runtime {
		fn account_nonce(account: AccountId) -> Index {
			System::account_nonce(account)
		}
	}

	
	// impl pallet_contracts_rpc_runtime_api::ContractsApi<
	// 	Block, AccountId, Balance, BlockNumber, Hash,
	// >
	// 	for Runtime
	// {
	// 	fn call(
	// 		origin: AccountId,
	// 		dest: AccountId,
	// 		value: Balance,
	// 		gas_limit: u64,
	// 		storage_deposit_limit: Option<Balance>,
	// 		input_data: Vec<u8>,
	// 	) -> pallet_contracts_primitives::ContractExecResult<Balance> {
	// 		Contracts::bare_call(origin, dest, value, gas_limit, storage_deposit_limit, input_data, true)
	// 	}

	// 	fn instantiate(
	// 		origin: AccountId,
	// 		endowment: Balance,
	// 		gas_limit: u64,
	// 		code: pallet_contracts_primitives::Code<Hash>,
	// 		data: Vec<u8>,
	// 		salt: Vec<u8>,
	// 	) -> pallet_contracts_primitives::ContractInstantiateResult<AccountId, Balance>
	// 	{
	// 		Contracts::bare_instantiate(origin, endowment, gas_limit, code, data, salt, true)
	// 	}

	// 	fn get_storage(
	// 		address: AccountId,
	// 		key: [u8; 32],
	// 	) -> pallet_contracts_primitives::GetStorageResult {
	// 		Contracts::get_storage(address, key)
	// 	}
	// }

	impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance> for Runtime {
		fn query_info(
			uxt: <Block as BlockT>::Extrinsic,
			len: u32,
		) -> pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo<Balance> {
			TransactionPayment::query_info(uxt, len)
		}
		fn query_fee_details(
			uxt: <Block as BlockT>::Extrinsic,
			len: u32,
		) -> pallet_transaction_payment::FeeDetails<Balance> {
			TransactionPayment::query_fee_details(uxt, len)
		}
	}

	/*
		the iris RPC runtime api
	*/
	impl pallet_iris_rpc_runtime_api::IrisApi<Block> for Runtime {
		fn retrieve_bytes(
		asset_id: u32,
		) -> Bytes {
			IrisAssets::retrieve_bytes(asset_id)
		}
	}

	#[cfg(feature = "runtime-benchmarks")]
	impl frame_benchmarking::Benchmark<Block> for Runtime {
		fn benchmark_metadata(extra: bool) -> (
			Vec<frame_benchmarking::BenchmarkList>,
			Vec<frame_support::traits::StorageInfo>,
		) {
			use frame_benchmarking::{baseline, Benchmarking, BenchmarkList};
			use frame_support::traits::StorageInfoTrait;
			use frame_system_benchmarking::Pallet as SystemBench;
			use baseline::Pallet as BaselineBench;

			let mut list = Vec::<BenchmarkList>::new();

			list_benchmark!(list, extra, pallet_assets, Assets);
			list_benchmark!(list, extra, frame_system, SystemBench::<Runtime>);
			// list_benchmark!(list, extra, pallet_contracts, Contracts);
			list_benchmark!(list, extra, pallet_balances, Balances);
			list_benchmark!(list, extra, pallet_timestamp, Timestamp);
			list_benchmark!(list, extra, pallet_iris, Iris);

			let storage_info = AllPalletsWithSystem::storage_info();

			return (list, storage_info)
		}

		fn dispatch_benchmark(
			config: frame_benchmarking::BenchmarkConfig
		) -> Result<Vec<frame_benchmarking::BenchmarkBatch>, sp_runtime::RuntimeString> {
			use frame_benchmarking::{baseline, Benchmarking, BenchmarkBatch, TrackedStorageKey};

			use frame_system_benchmarking::Pallet as SystemBench;
			use baseline::Pallet as BaselineBench;

			impl frame_system_benchmarking::Config for Runtime {}
			impl baseline::Config for Runtime {}

			let whitelist: Vec<TrackedStorageKey> = vec![
				// Block Number
				hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef702a5c1b19ab7a04f536c519aca4983ac").to_vec().into(),
				// Total Issuance
				hex_literal::hex!("c2261276cc9d1f8598ea4b6a74b15c2f57c875e4cff74148e4628f264b974c80").to_vec().into(),
				// Execution Phase
				hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef7ff553b5a9862a516939d82b3d3d8661a").to_vec().into(),
				// Event Count
				hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef70a98fdbe9ce6c55837576c60c7af3850").to_vec().into(),
				// System Events
				hex_literal::hex!("26aa394eea5630e07c48ae0c9558cef780d41e5e16056765bc8461851072c9d7").to_vec().into(),
			];

			let mut batches = Vec::<BenchmarkBatch>::new();
			let params = (&config, &whitelist);
			add_benchmarks!(params, batches);

			add_benchmark!(params, batches, pallet_assets, Assets);
			add_benchmark!(params, batches, frame_system, SystemBench::<Runtime>);
			// add_benchmark!(params, batches, pallet_contracts, Contracts);
			add_benchmark!(params, batches, pallet_balances, Balances);
			add_benchmark!(params, batches, pallet_timestamp, Timestamp);
			add_benchmark!(params, batches, pallet_iris_assets, Iris);

			if batches.is_empty() { return Err("Benchmark not found for this pallet.".into()) }
			Ok(batches)
		}
	}

	#[cfg(feature = "try-runtime")]
	impl frame_try_runtime::TryRuntime<Block> for Runtime {
		fn on_runtime_upgrade() -> (Weight, Weight) {
			// NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
			// have a backtrace here. If any of the pre/post migration checks fail, we shall stop
			// right here and right now.
			let weight = Executive::try_runtime_upgrade().unwrap();
			(weight, BlockWeights::get().max_block)
		}

		fn execute_block_no_check(block: Block) -> Weight {
			Executive::execute_block_no_check(block)
		}
	}
}

// /**
//  * 
//  * CHAIN EXTENSION
//  * 
//  * Here we expose functionality that can be used by smart contracts to hook into the iris runtime
//  * 
//  * in general, this exposes iris functionality that doesn't rely on the underlying IPFS network
//  * 
//  * We expose functionality to:
//  * - transfer owned assets
//  * - mint assets from an owned asset class
//  * 
//  */
// use frame_support::log::{
//     error,
//     trace,
// };
// use pallet_contracts::chain_extension::{
//     ChainExtension,
//     Environment,
//     Ext,
//     InitState,
//     RetVal,
//     SysConfig,
//     UncheckedFrom,
// };
// use sp_runtime::DispatchError;
// use frame_system::{
// 	self as system,
// };

// pub struct IrisExtension;

// impl ChainExtension<Runtime> for IrisExtension {
	
//     fn call<E: Ext>(
//         func_id: u32,
//         env: Environment<E, InitState>,
//     ) -> Result<RetVal, DispatchError>
//     where
//         <E::T as SysConfig>::AccountId:
//             UncheckedFrom<<E::T as SysConfig>::Hash> + AsRef<[u8]>,
//     {
// 		trace!(
// 			target: "runtime",
// 			"[ChainExtension]|call|func_id:{:}",
// 			func_id
// 		);
//         match func_id {	
// 			// IrisAssets::transfer_asset
//             0 => {
//                 let mut env = env.buf_in_buf_out();
// 				let (caller_account, target, asset_id, amount): (AccountId, AccountId, u32, u64) = env.read_as()?;
// 				let origin: Origin = system::RawOrigin::Signed(caller_account).into();

//                 crate::IrisAssets::transfer_asset(
// 					origin, sp_runtime::MultiAddress::Id(target), asset_id, amount,
// 				)?;
// 				Ok(RetVal::Converging(func_id))
//             },
// 			// IrisAssets::mint
// 			1 => {
// 				let mut env = env.buf_in_buf_out();
// 				let (caller_account, target, asset_id, amount): (AccountId, AccountId, u32, u64) = env.read_as()?;
// 				let origin: Origin = system::RawOrigin::Signed(caller_account).into();

//                 crate::IrisAssets::mint(
// 					origin, sp_runtime::MultiAddress::Id(target), asset_id, amount,
// 				)?;
// 				Ok(RetVal::Converging(func_id))
// 			},
// 			// IrisLedger::lock_currrency
// 			2 => {
// 				let mut env = env.buf_in_buf_out();
// 				let (caller_account, amount): (AccountId, u64) = env.read_as()?;
// 				let origin: Origin = system::RawOrigin::Signed(caller_account).into();

// 				crate::IrisLedger::lock_currency(
// 					origin, amount.into(),
// 				)?;
// 				Ok(RetVal::Converging(func_id))
// 			},
// 			// IrisLedger::unlock_currency_and_transfer
// 			3 => {
// 				let mut env = env.buf_in_buf_out();
// 				let (caller_account, target): (AccountId, AccountId) = env.read_as()?;
// 				let origin: Origin = system::RawOrigin::Signed(caller_account).into();

// 				crate::IrisLedger::unlock_currency_and_transfer(
// 					origin, target,
// 				)?;
// 				Ok(RetVal::Converging(func_id))
// 			},
//             _ => {
// 				// env.write(&random_slice, false, None).map_err(|_| {
//                 //     DispatchError::Other("ChainExtension failed to call transfer_assets")
//                 // })?;
//                 error!("Called an unregistered `func_id`: {:}", func_id);
//                 return Err(DispatchError::Other("Unimplemented func_id"))
//             }
//         }
//     }

//     fn enabled() -> bool {
//         true
//     }
// }
