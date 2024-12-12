use alloy_sol_types::{sol, SolCall};
use reth_primitives::Bytes;

use super::TestContract;

// KZGPointEvaluationContract wrapper.
sol! {
    #[sol(abi)]
    KZGPointEvaluation,
    "./src/evm/test_data/KZGPointEvaluation.abi"
}

/// KZGPointEvaluationContract wrapper.
pub struct KZGPointEvaluationContract {
    bytecode: Vec<u8>,
}

impl Default for KZGPointEvaluationContract {
    fn default() -> Self {
        let bytecode = {
            let bytecode_hex =
                include_str!("../../../evm/src/evm/test_data/KZGPointEvaluation.bin");
            hex::decode(bytecode_hex).unwrap()
        };

        Self { bytecode }
    }
}

impl TestContract for KZGPointEvaluationContract {
    fn byte_code(&self) -> Vec<u8> {
        self.byte_code()
    }
}

impl KZGPointEvaluationContract {
    /// KZGPointEvaluation bytecode.
    pub fn byte_code(&self) -> Vec<u8> {
        self.bytecode.clone()
    }

    /// Claims the gift.
    pub fn call_kzg_point_evaluation(
        &self,
        input: Bytes, // 192 bytes
    ) -> Vec<u8> {
        KZGPointEvaluation::verifyPointEvaluationCall { input }.abi_encode()
    }
}
