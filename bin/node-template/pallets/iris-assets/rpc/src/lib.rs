pub use self::gen_client::Client as IrisClient;
use jsonrpc_core::{Error as RpcError, ErrorCode, Result};
use jsonrpc_derive::rpc;
pub use pallet_iris_rpc_runtime_api::IrisApi as IrisRuntimeApi;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_core::{
	Bytes,
	sr25519::{Signature, Public}
};
use sp_runtime::{
	generic::BlockId,
	traits::{Block as BlockT},
};
use std::sync::Arc;
use codec::{Codec, Decode, Encode};
use sp_std::vec::Vec;

#[rpc]
pub trait IrisApi<BlockHash> {
	#[rpc(name = "iris_retrieveBytes")]
	fn retrieve_bytes(
		&self,
		asset_id: u32,
		at: Option<BlockHash>,
	) -> Result<Bytes>;
}

/// A struct that implements IrisRpc
pub struct Iris<C, M> {
	client: Arc<C>,
	_marker: std::marker::PhantomData<M>,
}

impl<C, M> Iris<C, M> {
	/// create new 'Iris' instance with the given reference	to the client
	pub fn new(client: Arc<C>) -> Self {
		Self { client, _marker: Default::default() }
	}
}

pub enum Error {
	RuntimeError,
	DecodeError,
}

impl From<Error> for i64 {
	fn from(e: Error) -> i64 {
		match e {
			Error::RuntimeError => 1,
			Error::DecodeError => 2,
		}
	}
}

impl<C, Block> IrisApi<<Block as BlockT>::Hash>
	for Iris<C, Block>
where 
	Block: BlockT,
	C: Send + Sync + 'static,
	C: ProvideRuntimeApi<Block>,
	C: HeaderBackend<Block>,
	C::Api: IrisRuntimeApi<Block>,
{
	fn retrieve_bytes(
		&self,
		asset_id: u32,
		at: Option<<Block as BlockT>::Hash>,
	) -> Result<Bytes> {
		let api = self.client.runtime_api();
		let at = BlockId::hash(at.unwrap_or_else(||
			self.client.info().best_hash
		));
		let runtime_api_result = api.retrieve_bytes(&at, asset_id);
		runtime_api_result.map_err(|e| RpcError{
			code: ErrorCode::ServerError(Error::DecodeError.into()),
			message: "unable to query runtime api".into(),
			data: Some(format!("{:?}", e).into()),
		})
	}
}