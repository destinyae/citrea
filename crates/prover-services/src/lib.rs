use std::sync::Arc;

use citrea_stf::verifier::StateTransitionVerifier;
use sov_rollup_interface::services::da::DaService;
use sov_rollup_interface::stf::StateTransitionFunction;
use sov_rollup_interface::zk::ZkvmHost;
use tokio::sync::Mutex;

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
    Simulate(Arc<Mutex<StateTransitionVerifier<Stf, Da::Verifier, Vm::Guest>>>),
    /// The executor runs the rollup verification logic in the zkVM, but does not actually
    /// produce a zk proof
    Execute,
    /// The prover runs the rollup verification logic in the zkVM and produces a zk proof
    Prove,
}

impl<Da, Vm, Stf> Clone for ProofGenMode<Da, Vm, Stf>
where
    Da: DaService,
    Vm: ZkvmHost,
    Stf: StateTransitionFunction<Da::Spec>,
{
    fn clone(&self) -> Self {
        match self {
            Self::Skip => Self::Skip,
            Self::Execute => Self::Execute,
            Self::Prove => Self::Prove,
            Self::Simulate(simulate) => Self::Simulate(Arc::clone(simulate)),
        }
    }
}
