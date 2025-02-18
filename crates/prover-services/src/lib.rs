use citrea_stf::verifier::StateTransitionVerifier;
use sov_rollup_interface::services::da::DaService;
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::zk::ZkvmHost;

mod parallel;
pub use parallel::*;

pub enum ProofGenMode<Da, Vm, Stf>
where
    Da: DaService,
    Vm: ZkvmHost,
    Stf: StateTransitionFunction<Da::Spec>,
{
    /// Skips proving.
    Skip,
    /// The simulator runs the rollup verifier logic without even emulating the zkVM
    Simulate(StateTransitionVerifier<Stf, Da::Verifier, Vm::Guest>),
    /// The executor runs the rollup verification logic in the zkVM, but does not actually
    /// produce a zk proof
    Execute,
    /// The prover runs the rollup verification logic in the zkVM and produces a zk proof
    Prove,
}
