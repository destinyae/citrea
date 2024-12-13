#![no_main]
use citrea_stf::runtime::Runtime;
use citrea_stf::StfVerifier;
use sov_mock_da::MockDaVerifier;
use sov_modules_api::default_context::ZkDefaultContext;
use sov_modules_stf_blueprint::StfBlueprint;
use citrea_risc0_adapter::guest::Risc0Guest;
use sov_state::ZkStorage;
use sov_rollup_interface::zk::ZkvmGuest;

risc0_zkvm::guest::entry!(main);

pub fn main() {
    let guest = Risc0Guest::new();
    let storage = ZkStorage::new();
    let stf = StfBlueprint::new();

    let mut stf_verifier: StfVerifier<_, ZkDefaultContext, Runtime<_, _>> = StfVerifier::new(
        stf,
        MockDaVerifier {}
    );

    let data = guest.read_from_host();

    let out = stf_verifier
        .run_sequencer_commitments_in_da_slot(data, storage)
        .expect("Prover must be honest");

    guest.commit(&out);
}
