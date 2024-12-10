use std::sync::{Arc, Mutex};

#[cfg(feature = "local")]
use citrea_evm::DevSigner;
use citrea_evm::{DbAccount, Evm};
use jsonrpsee::types::ErrorObjectOwned;
use reth_primitives::{Address, Bytes, KECCAK_EMPTY, U256};
use reth_rpc_eth_types::EthApiError;
use reth_rpc_types::trace::geth::GethTrace;
use reth_rpc_types::{BlockId, EIP1186StorageProof, JsonStorageKey};
use rustc_version_runtime::version;
use schnellru::{ByLength, LruMap};
use sequencer_client::SequencerClient;
use sov_db::ledger_db::LedgerDB;
use sov_modules_api::{StateMapAccessor, WorkingSet};
use sov_rollup_interface::services::da::DaService;
use sov_rollup_interface::CITREA_VERSION;
use sov_state::storage::NativeStorage;
use tokio::sync::broadcast;
use tracing::instrument;

use crate::gas_price::fee_history::FeeHistoryCacheConfig;
use crate::gas_price::gas_oracle::{GasPriceOracle, GasPriceOracleConfig};
use crate::subscription::SubscriptionManager;

const MAX_TRACE_BLOCK: u32 = 1000;
const DEFAULT_PRIORITY_FEE: U256 = U256::from_limbs([100, 0, 0, 0]);

#[derive(Clone)]
pub struct EthRpcConfig {
    pub gas_price_oracle_config: GasPriceOracleConfig,
    pub fee_history_cache_config: FeeHistoryCacheConfig,
    #[cfg(feature = "local")]
    pub eth_signer: DevSigner,
}

pub struct Ethereum<C: sov_modules_api::Context, Da: DaService> {
    #[allow(dead_code)]
    pub(crate) da_service: Arc<Da>,
    pub(crate) gas_price_oracle: GasPriceOracle<C>,
    #[cfg(feature = "local")]
    pub(crate) eth_signer: DevSigner,
    pub(crate) storage: C::Storage,
    pub(crate) ledger_db: LedgerDB,
    pub(crate) sequencer_client: Option<SequencerClient>,
    pub(crate) web3_client_version: String,
    pub(crate) trace_cache: Mutex<LruMap<u64, Vec<GethTrace>, ByLength>>,
    pub(crate) subscription_manager: Option<SubscriptionManager>,
}

impl<C: sov_modules_api::Context, Da: DaService> Ethereum<C, Da>
where
    C::Storage: NativeStorage,
{
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        da_service: Arc<Da>,
        gas_price_oracle_config: GasPriceOracleConfig,
        fee_history_cache_config: FeeHistoryCacheConfig,
        #[cfg(feature = "local")] eth_signer: DevSigner,
        storage: C::Storage,
        ledger_db: LedgerDB,
        sequencer_client: Option<SequencerClient>,
        soft_confirmation_rx: Option<broadcast::Receiver<u64>>,
    ) -> Self {
        let evm = Evm::<C>::default();
        let gas_price_oracle =
            GasPriceOracle::new(evm, gas_price_oracle_config, fee_history_cache_config);

        let rollup = "citrea";
        let arch = std::env::consts::ARCH;
        let rustc_v = version();

        let current_version = format!("{}/{}/{}/rust-{}", rollup, CITREA_VERSION, arch, rustc_v);

        let trace_cache = Mutex::new(LruMap::new(ByLength::new(MAX_TRACE_BLOCK)));

        let subscription_manager =
            soft_confirmation_rx.map(|rx| SubscriptionManager::new::<C>(storage.clone(), rx));

        Self {
            da_service,
            gas_price_oracle,
            #[cfg(feature = "local")]
            eth_signer,
            storage,
            ledger_db,
            sequencer_client,
            web3_client_version: current_version,
            trace_cache,
            subscription_manager,
        }
    }

    #[instrument(level = "trace", skip_all)]
    pub(crate) fn max_fee_per_gas(&self, working_set: &mut WorkingSet<C::Storage>) -> (U256, U256) {
        let evm = Evm::<C>::default();
        let base_fee = evm
            .get_block_by_number(None, None, working_set)
            .unwrap()
            .unwrap()
            .header
            .base_fee_per_gas
            .unwrap_or_default();

        // We return a default priority of 100 wei. Small enough to
        // not make a difference to price, but also allows bumping tip
        // of EIP-1559 transactions in times of congestion.
        (U256::from(base_fee), DEFAULT_PRIORITY_FEE)
    }

    pub fn get_proof(
        &self,
        address: Address,
        keys: Vec<U256>,
        block_id: Option<BlockId>,
        working_set: &mut WorkingSet<C::Storage>,
    ) -> Result<reth_rpc_types::EIP1186AccountProofResponse, ErrorObjectOwned> {
        use sov_state::storage::{StateCodec, StorageKey};

        let evm = Evm::<C>::default();
        evm.set_state_to_end_of_evm_block_by_block_id(block_id, working_set)?;
        let block_id: u64 = evm
            .block_number(working_set)?
            .try_into()
            .map_err(|_| EthApiError::UnknownBlockNumber)?;

        let root_hash = working_set
            .get_root_hash(block_id)
            .map_err(|_| EthApiError::UnknownBlockNumber)?;

        let account = evm.accounts.get(&address, working_set).unwrap_or_default();
        let balance = account.balance;
        let nonce = account.nonce;
        let code_hash = account.code_hash.unwrap_or(KECCAK_EMPTY);

        let account_key = StorageKey::new(
            evm.accounts.prefix(),
            &address,
            evm.accounts.codec().key_codec(),
        );
        let account_proof = working_set.get_with_proof(account_key);
        let account_proof = borsh::to_vec(&account_proof).expect("Serialization shouldn't fail");
        let account_proof = Bytes::from(account_proof);

        let db_account = DbAccount::new(address);
        let mut storage_proof = vec![];
        for key in keys {
            let storage_key = StorageKey::new(
                db_account.storage.prefix(),
                &key,
                db_account.storage.codec().key_codec(),
            );
            let value = db_account.storage.get(&key, working_set);
            let proof = working_set.get_with_proof(storage_key);
            let value_proof = borsh::to_vec(&proof.proof).expect("Serialization shouldn't fail");
            let value_proof = Bytes::from(value_proof);
            storage_proof.push(EIP1186StorageProof {
                key: JsonStorageKey(key.into()),
                value: value.unwrap_or_default(),
                proof: vec![value_proof],
            });
        }

        Ok(reth_rpc_types::EIP1186AccountProofResponse {
            address,
            balance,
            nonce,
            code_hash,
            storage_hash: root_hash.0.into(),
            account_proof: vec![account_proof],
            storage_proof,
        })
    }

    //     fn make_raw_tx(
    //         &self,
    //         raw_tx: RlpEvmTransaction,
    //     ) -> Result<(B256, Vec<u8>), jsonrpsee::core::RegisterMethodError> {
    //         let signed_transaction: RethTransactionSignedNoHash = raw_tx.clone().try_into()?;

    //         let tx_hash = signed_transaction.hash();

    //         let tx = CallMessage { txs: vec![raw_tx] };
    //         let message = <Runtime<C, Da::Spec> as EncodeCall<citrea_evm::Evm<C>>>::encode_call(tx);

    //         Ok((B256::from(tx_hash), message))
    //     }
}
