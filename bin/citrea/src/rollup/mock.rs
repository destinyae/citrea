use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use citrea_common::rpc::register_healthcheck_rpc;
use citrea_common::tasks::manager::TaskManager;
use citrea_common::{BatchProverConfig, FullNodeConfig, LightClientProverConfig};
// use citrea_sp1::host::SP1Host;
use citrea_risc0_adapter::host::Risc0BonsaiHost;
use citrea_risc0_adapter::Digest;
use citrea_stf::genesis_config::StorageConfig;
use citrea_stf::runtime::Runtime;
use citrea_stf::verifier::StateTransitionVerifier;
use prover_services::{ParallelProverService, ProofGenMode};
use sov_db::ledger_db::LedgerDB;
use sov_mock_da::{MockDaConfig, MockDaService, MockDaSpec};
use sov_modules_api::default_context::{DefaultContext, ZkDefaultContext};
use sov_modules_api::{Address, Spec};
use sov_modules_rollup_blueprint::RollupBlueprint;
use sov_modules_stf_blueprint::StfBlueprint;
use sov_prover_storage_manager::ProverStorageManager;
use sov_rollup_interface::spec::SpecId;
use sov_rollup_interface::zk::Zkvm;
use sov_state::ZkStorage;
use sov_stf_runner::ProverGuestRunConfig;
use tokio::sync::{broadcast, Mutex};

use crate::CitreaRollupBlueprint;

/// Rollup with MockDa
pub struct MockDemoRollup {}

impl CitreaRollupBlueprint for MockDemoRollup {}

#[async_trait]
impl RollupBlueprint for MockDemoRollup {
    type DaService = MockDaService;
    type DaSpec = MockDaSpec;
    type DaConfig = MockDaConfig;
    type Vm = Risc0BonsaiHost<'static>;

    type ZkContext = ZkDefaultContext;
    type NativeContext = DefaultContext;

    type StorageManager = ProverStorageManager<MockDaSpec>;

    type ZkRuntime = Runtime<Self::ZkContext, Self::DaSpec>;
    type NativeRuntime = Runtime<Self::NativeContext, Self::DaSpec>;

    type ProverService = ParallelProverService<
        Self::DaService,
        Self::Vm,
        StfBlueprint<Self::ZkContext, Self::DaSpec, Self::ZkRuntime>,
    >;

    fn new() -> Self {
        Self {}
    }

    fn create_rpc_methods(
        &self,
        storage: &<Self::NativeContext as Spec>::Storage,
        ledger_db: &LedgerDB,
        da_service: &Arc<Self::DaService>,
        sequencer_client_url: Option<String>,
        soft_confirmation_rx: Option<broadcast::Receiver<u64>>,
    ) -> Result<jsonrpsee::RpcModule<()>, anyhow::Error> {
        // TODO set the sequencer address
        let sequencer = Address::new([0; 32]);

        let mut rpc_methods = sov_modules_rollup_blueprint::register_rpc::<
            Self::NativeRuntime,
            Self::NativeContext,
            Self::DaService,
        >(storage, ledger_db, da_service, sequencer)?;

        crate::eth::register_ethereum::<Self::DaService>(
            da_service.clone(),
            storage.clone(),
            ledger_db.clone(),
            &mut rpc_methods,
            sequencer_client_url,
            soft_confirmation_rx,
        )?;

        register_healthcheck_rpc(&mut rpc_methods, ledger_db.clone())?;

        Ok(rpc_methods)
    }

    fn get_batch_prover_code_commitments_by_spec(
        &self,
    ) -> HashMap<SpecId, <Self::Vm as Zkvm>::CodeCommitment> {
        let mut map = HashMap::new();
        map.insert(
            SpecId::Genesis,
            Digest::new(citrea_risc0::BATCH_PROOF_MOCK_ID),
        );
        map
    }

    fn get_light_client_prover_code_commitment(&self) -> <Self::Vm as Zkvm>::CodeCommitment {
        Digest::new(citrea_risc0::LIGHT_CLIENT_PROOF_MOCK_ID)
    }

    async fn create_da_service(
        &self,
        rollup_config: &FullNodeConfig<Self::DaConfig>,
        _require_wallet_check: bool,
        _task_manager: &mut TaskManager<()>,
    ) -> Result<Arc<Self::DaService>, anyhow::Error> {
        Ok(Arc::new(MockDaService::new(
            rollup_config.da.sender_address,
            &rollup_config.da.db_path,
        )))
    }

    async fn create_batch_prover_service(
        &self,
        prover_config: BatchProverConfig,
        _rollup_config: &FullNodeConfig<Self::DaConfig>,
        da_service: &Arc<Self::DaService>,
        ledger_db: LedgerDB,
    ) -> Self::ProverService {
        let vm = Risc0BonsaiHost::new(citrea_risc0::BATCH_PROOF_MOCK_ELF, ledger_db.clone());

        let zk_stf = StfBlueprint::new();
        let zk_storage = ZkStorage::new();
        let da_verifier = Default::default();

        let proof_mode = match prover_config.proving_mode {
            ProverGuestRunConfig::Skip => ProofGenMode::Skip,
            ProverGuestRunConfig::Simulate => {
                let stf_verifier = StateTransitionVerifier::new(zk_stf, da_verifier);
                ProofGenMode::Simulate(Arc::new(Mutex::new(stf_verifier)))
            }
            ProverGuestRunConfig::Execute => ProofGenMode::Execute,
            ProverGuestRunConfig::Prove => ProofGenMode::Prove,
        };

        ParallelProverService::new(da_service.clone(), vm, proof_mode, zk_storage, 1, ledger_db)
            .expect("Should be able to instantiate prover service")
    }

    async fn create_light_client_prover_service(
        &self,
        prover_config: LightClientProverConfig,
        _rollup_config: &FullNodeConfig<Self::DaConfig>,
        da_service: &Arc<Self::DaService>,
        ledger_db: LedgerDB,
    ) -> Self::ProverService {
        let vm = Risc0BonsaiHost::new(citrea_risc0::LIGHT_CLIENT_PROOF_MOCK_ELF, ledger_db.clone());
        let zk_stf = StfBlueprint::new();
        let zk_storage = ZkStorage::new();
        let da_verifier = Default::default();

        let proof_mode = match prover_config.proving_mode {
            ProverGuestRunConfig::Skip => ProofGenMode::Skip,
            ProverGuestRunConfig::Simulate => {
                let stf_verifier = StateTransitionVerifier::new(zk_stf, da_verifier);
                ProofGenMode::Simulate(Arc::new(Mutex::new(stf_verifier)))
            }
            ProverGuestRunConfig::Execute => ProofGenMode::Execute,
            ProverGuestRunConfig::Prove => ProofGenMode::Prove,
        };

        ParallelProverService::new(da_service.clone(), vm, proof_mode, zk_storage, 1, ledger_db)
            .expect("Should be able to instantiate prover service")
    }

    fn create_storage_manager(
        &self,
        rollup_config: &FullNodeConfig<Self::DaConfig>,
    ) -> anyhow::Result<Self::StorageManager> {
        let storage_config = StorageConfig {
            path: rollup_config.storage.path.clone(),
            db_max_open_files: rollup_config.storage.db_max_open_files,
        };
        ProverStorageManager::new(storage_config)
    }
}
