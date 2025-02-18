#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{mpsc, Arc, Mutex, RwLock};

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use sov_rollup_interface::zk::{Matches, Proof};

/// A mock commitment to a particular zkVM program.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct MockCodeCommitment(pub [u8; 32]);

impl Matches<MockCodeCommitment> for MockCodeCommitment {
    fn matches(&self, other: &MockCodeCommitment) -> bool {
        self.0 == other.0
    }
}

impl From<MockCodeCommitment> for [u32; 8] {
    fn from(val: MockCodeCommitment) -> Self {
        let mut output = [0u32; 8];
        for (i, chunk) in val.0.chunks(4).enumerate() {
            output[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        output
    }
}

impl From<[u32; 8]> for MockCodeCommitment {
    fn from(value: [u32; 8]) -> Self {
        let mut output = [0u8; 32];
        for (i, &val) in value.iter().enumerate() {
            output[i * 4..(i + 1) * 4].copy_from_slice(&val.to_le_bytes());
        }
        MockCodeCommitment(output)
    }
}

/// A mock proof generated by a zkVM.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub struct MockProof {
    /// The ID of the program this proof might be valid for.
    pub program_id: MockCodeCommitment,
    /// Whether the proof is valid.
    pub is_valid: bool,
    /// The tamper-proof outputs of the proof. Serialized Mock Journal
    pub log: Vec<u8>,
}

/// A mock journal generated by a zkVM.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
pub enum MockJournal {
    /// Represents the proof of this journal is verifiable
    Verifiable(Vec<u8>),
    /// Represents the proof of this journal is non-verifiable
    Unverifiable(Vec<u8>),
}

impl MockProof {
    /// Serializes a proof into a writer.
    pub fn encode(&self, mut writer: impl Write) {
        writer.write_all(&self.program_id.0).unwrap();
        let is_valid_byte = if self.is_valid { 1 } else { 0 };
        writer.write_all(&[is_valid_byte]).unwrap();
        writer.write_all(&self.log).unwrap();
    }

    /// Serializes a proof into a vector.
    pub fn encode_to_vec(&self) -> Vec<u8> {
        borsh::to_vec(&self).unwrap()
    }

    /// Tries to deserialize a proof from a byte slice.
    pub fn decode(input: &[u8]) -> Result<Self, anyhow::Error> {
        Ok(Self::try_from_slice(input).unwrap())
    }
}

/// MockZkvm is a mock struct which implements Zkvm and ZkvmHost traits.
/// It has the capability to behave as if multiple proofs are running
/// and exposes method `finish_next_proof` to emulate finishing behavior
/// of a single proof. It is useful for testing parallel proving.
#[derive(Clone)]
pub struct MockZkvm {
    waiting_tasks: Arc<Mutex<VecDeque<mpsc::Sender<()>>>>,
    committed_data: VecDeque<Vec<u8>>,
    is_valid: bool,
}

impl Default for MockZkvm {
    fn default() -> Self {
        Self::new()
    }
}

impl MockZkvm {
    /// Create new instance of `MockZkvm`
    pub fn new() -> Self {
        Self {
            waiting_tasks: Default::default(),
            committed_data: Default::default(),
            is_valid: Default::default(),
        }
    }

    /// Notifies the next proof in FIFO order to emulate finishing behavior.
    /// Returns whether there was any proof in the queue.
    pub fn finish_next_proof(&self) -> bool {
        let mut tasks = self.waiting_tasks.lock().unwrap();
        if let Some(chan) = tasks.pop_front() {
            chan.send(()).unwrap();
            true
        } else {
            false
        }
    }
}

impl sov_rollup_interface::zk::Zkvm for MockZkvm {
    type CodeCommitment = MockCodeCommitment;

    type Error = anyhow::Error;

    fn verify(
        serialized_proof: &[u8],
        code_commitment: &Self::CodeCommitment,
    ) -> Result<Vec<u8>, Self::Error> {
        let proof = MockProof::decode(serialized_proof)?;
        anyhow::ensure!(
            proof.program_id.matches(code_commitment),
            "Proof failed to verify against requested code commitment"
        );
        anyhow::ensure!(proof.is_valid, "Proof is not valid");
        Ok(serialized_proof[33..].to_vec())
    }

    fn extract_raw_output(serialized_proof: &[u8]) -> Result<Vec<u8>, Self::Error> {
        Ok(serialized_proof[33..].to_vec())
    }

    fn verify_and_extract_output<T: BorshDeserialize>(
        serialized_proof: &[u8],
        code_commitment: &Self::CodeCommitment,
    ) -> Result<T, Self::Error> {
        let output = Self::verify(serialized_proof, code_commitment)?;
        Ok(T::deserialize(&mut &*output)?)
    }
}

impl sov_rollup_interface::zk::ZkvmHost for MockZkvm {
    type Guest = MockZkGuest;

    fn add_hint(&mut self, item: Vec<u8>) {
        let proof_info = ProofInfo {
            hint: item,
            is_valid: self.is_valid,
        };

        let data = borsh::to_vec(&proof_info).unwrap();

        self.committed_data.push_back(data);
    }

    fn add_assumption(&mut self, _receipt_buf: Vec<u8>) {
        unimplemented!()
    }

    fn simulate_with_hints(&mut self) -> Self::Guest {
        MockZkGuest {
            input: vec![],
            output: RwLock::new(vec![]),
        }
    }

    fn run(
        &mut self,
        _elf: Vec<u8>,
        _with_proof: bool,
    ) -> Result<sov_rollup_interface::zk::Proof, anyhow::Error> {
        let (tx, rx) = mpsc::channel();

        let mut tasks = self.waiting_tasks.lock().unwrap();
        tasks.push_back(tx);
        drop(tasks);

        // Block until finish signal arrives
        rx.recv().unwrap();

        Ok(self.committed_data.pop_front().unwrap_or_default())
    }

    fn extract_output<Da: sov_rollup_interface::da::DaSpec, T: BorshDeserialize>(
        proof: &Proof,
    ) -> Result<T, Self::Error> {
        let data: ProofInfo = borsh::from_slice(proof)?;

        T::try_from_slice(&data.hint).map_err(Into::into)
    }

    fn recover_proving_sessions(&self) -> Result<Vec<Proof>, anyhow::Error> {
        unimplemented!()
    }
}

/// A mock implementing the Guest.
#[derive(Default)]
pub struct MockZkGuest {
    /// Input of the circuit
    pub input: Vec<u8>,
    /// Output of the circuit wrapped in RwLock for thread safe mutability
    pub output: RwLock<Vec<u8>>,
}

impl MockZkGuest {
    /// Constructs a new MockZk Guest
    pub fn new(input: Vec<u8>) -> MockZkGuest {
        MockZkGuest {
            input,
            output: RwLock::new(vec![]),
        }
    }
}

impl sov_rollup_interface::zk::Zkvm for MockZkGuest {
    type CodeCommitment = MockCodeCommitment;

    type Error = anyhow::Error;

    fn verify(
        journal: &[u8],
        _code_commitment: &Self::CodeCommitment,
    ) -> Result<Vec<u8>, Self::Error> {
        Ok(journal.to_vec())
    }

    fn extract_raw_output(serialized_proof: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let mock_proof = MockProof::decode(serialized_proof).unwrap();
        Ok(mock_proof.log)
    }

    fn verify_and_extract_output<T: BorshDeserialize>(
        journal: &[u8],
        _code_commitment: &Self::CodeCommitment,
    ) -> Result<T, Self::Error> {
        let mock_journal = MockJournal::try_from_slice(journal).unwrap();
        match mock_journal {
            MockJournal::Verifiable(journal) => Ok(T::try_from_slice(&journal)?),
            MockJournal::Unverifiable(_) => Err(anyhow::anyhow!("Journal is unverifiable")),
        }
    }
}

impl sov_rollup_interface::zk::ZkvmGuest for MockZkGuest {
    fn read_from_host<T: BorshDeserialize>(&self) -> T {
        T::try_from_slice(self.input.as_slice()).expect("Failed to deserialize input from host")
    }

    fn commit<T: BorshSerialize>(&self, item: &T) {
        let buf = borsh::to_vec(item).expect("Serialization to vec is infallible");
        // Mutate the `output` field using `borrow_mut`
        self.output.write().unwrap().extend_from_slice(&buf);
    }
}

#[derive(Debug, BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
struct ProofInfo {
    hint: Vec<u8>,
    is_valid: bool,
}

#[test]
fn test_mock_proof_round_trip() {
    let proof = MockProof {
        program_id: MockCodeCommitment([1; 32]),
        is_valid: true,
        log: vec![2; 50],
    };

    let encoded = proof.encode_to_vec();

    let decoded = MockProof::decode(&encoded).unwrap();
    assert_eq!(proof, decoded);
}
