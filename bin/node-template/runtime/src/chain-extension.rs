// use codec::Encode;
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
//         match func_id {
//             1 => {
//                 let mut env = env.buf_in_buf_out();
//                 let arg: [u8; 32] = env.read_as()?;
//                 // call to transfer assets should go here
//                 // crate::IrisAssets::trandsfer(&arg);
//                 // let random_slice = random_seed.encode();
//                 trace!(
//                     target: "runtime",
//                     "[ChainExtension]|call|func_id:{:}",
//                     func_id
//                 );
//                 // env.write(&random_slice, false, None).map_err(|_| {
//                 //     DispatchError::Other("ChainExtension failed to call transfer_assets")
//                 // })?;
//             }
//             _ => {
//                 error!("Called an unregistered `func_id`: {:}", func_id);
//                 return Err(DispatchError::Other("Unimplemented func_id"))
//             }
//         }
//         Ok(RetVal::Converging(0))
//     }

//     fn enabled() -> bool {
//         true
//     }
// }
