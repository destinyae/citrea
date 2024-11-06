//! Defines traits and types used by the rollup to verify claims about the
//! soft confirmation

extern crate alloc;

use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt::Debug;

use borsh::{BorshDeserialize, BorshSerialize};
use digest::{Digest, Output};
use serde::{Deserialize, Serialize};

/// Contains raw transactions and information about the soft confirmation block
#[derive(Debug, PartialEq, BorshSerialize)]
pub struct UnsignedSoftConfirmation<'txs> {
    l2_height: u64,
    da_slot_height: u64,
    da_slot_hash: [u8; 32],
    da_slot_txs_commitment: [u8; 32],
    txs: &'txs [Vec<u8>],
    deposit_data: Vec<Vec<u8>>,
    l1_fee_rate: u128,
    timestamp: u64,
}

impl<'txs> UnsignedSoftConfirmation<'txs> {
    #[allow(clippy::too_many_arguments)]
    /// Creates a new unsigned soft confirmation batch
    pub fn new(
        l2_height: u64,
        da_slot_height: u64,
        da_slot_hash: [u8; 32],
        da_slot_txs_commitment: [u8; 32],
        txs: &'txs [Vec<u8>],
        deposit_data: Vec<Vec<u8>>,
        l1_fee_rate: u128,
        timestamp: u64,
    ) -> Self {
        Self {
            l2_height,
            da_slot_height,
            da_slot_hash,
            da_slot_txs_commitment,
            txs,
            deposit_data,
            l1_fee_rate,
            timestamp,
        }
    }
    /// L2 block height
    pub fn l2_height(&self) -> u64 {
        self.l2_height
    }
    /// DA block to build on
    pub fn da_slot_height(&self) -> u64 {
        self.da_slot_height
    }
    /// DA block hash
    pub fn da_slot_hash(&self) -> [u8; 32] {
        self.da_slot_hash
    }
    /// DA block transactions commitment
    pub fn da_slot_txs_commitment(&self) -> [u8; 32] {
        self.da_slot_txs_commitment
    }
    /// Raw transactions.
    pub fn txs(&self) -> &[Vec<u8>] {
        self.txs
    }
    /// Deposit data from L1 chain
    pub fn deposit_data(&self) -> Vec<Vec<u8>> {
        self.deposit_data.clone()
    }
    /// Base layer fee rate sats/wei etc. per byte.
    pub fn l1_fee_rate(&self) -> u128 {
        self.l1_fee_rate
    }
    /// Sequencer block timestamp
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }
    /// Compute digest for the whole UnsignedSoftConfirmation struct
    pub fn compute_digest<D: Digest>(&self) -> Output<D> {
        let mut hasher = D::new();
        hasher.update(self.l2_height.to_be_bytes());
        hasher.update(self.da_slot_height.to_be_bytes());
        hasher.update(self.da_slot_hash);
        hasher.update(self.da_slot_txs_commitment);
        for tx in self.txs {
            hasher.update(tx);
        }
        for deposit in &self.deposit_data {
            hasher.update(deposit);
        }
        hasher.update(self.l1_fee_rate.to_be_bytes());
        hasher.update(self.timestamp.to_be_bytes());
        hasher.finalize()
    }
    /// Old version of compute_digest
    // TODO: Remove derive(BorshSerialize) for UnsignedSoftConfirmation
    //   when removing this fn
    // FIXME: ^
    pub fn pre_fork1_hash<D: Digest>(&self) -> Output<D> {
        let raw = borsh::to_vec(&self).unwrap();
        D::digest(raw.as_slice())
    }
}

/// Signed version of the `UnsignedSoftConfirmation`
/// Contains the signature and public key of the sequencer
#[derive(Debug, PartialEq, BorshDeserialize, BorshSerialize, Serialize, Deserialize, Eq)]
pub struct SignedSoftConfirmation<'txs> {
    l2_height: u64,
    hash: [u8; 32],
    prev_hash: [u8; 32],
    da_slot_height: u64,
    da_slot_hash: [u8; 32],
    da_slot_txs_commitment: [u8; 32],
    l1_fee_rate: u128,
    txs: Cow<'txs, [Vec<u8>]>,
    signature: Vec<u8>,
    deposit_data: Vec<Vec<u8>>,
    pub_key: Vec<u8>,
    timestamp: u64,
}

impl<'txs> SignedSoftConfirmation<'txs> {
    /// Creates a signed soft confirmation batch
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        l2_height: u64,
        hash: [u8; 32],
        prev_hash: [u8; 32],
        da_slot_height: u64,
        da_slot_hash: [u8; 32],
        da_slot_txs_commitment: [u8; 32],
        l1_fee_rate: u128,
        txs: Cow<'txs, [Vec<u8>]>,
        deposit_data: Vec<Vec<u8>>,
        signature: Vec<u8>,
        pub_key: Vec<u8>,
        timestamp: u64,
    ) -> SignedSoftConfirmation {
        Self {
            l2_height,
            hash,
            prev_hash,
            da_slot_height,
            da_slot_hash,
            da_slot_txs_commitment,
            l1_fee_rate,
            txs,
            deposit_data,
            signature,
            pub_key,
            timestamp,
        }
    }

    /// L2 block height
    pub fn l2_height(&self) -> u64 {
        self.l2_height
    }

    /// Hash of the signed batch
    pub fn hash(&self) -> [u8; 32] {
        self.hash
    }

    /// Hash of the previous signed batch
    pub fn prev_hash(&self) -> [u8; 32] {
        self.prev_hash
    }

    /// DA block this soft confirmation was given for
    pub fn da_slot_height(&self) -> u64 {
        self.da_slot_height
    }

    /// DA block to build on
    pub fn da_slot_hash(&self) -> [u8; 32] {
        self.da_slot_hash
    }

    /// DA block transactions commitment
    pub fn da_slot_txs_commitment(&self) -> [u8; 32] {
        self.da_slot_txs_commitment
    }

    /// Public key of signer
    pub fn sequencer_pub_key(&self) -> &[u8] {
        self.pub_key.as_ref()
    }

    /// Txs of signed batch
    pub fn txs(&self) -> &[Vec<u8>] {
        &self.txs
    }

    /// Deposit data
    pub fn deposit_data(&self) -> &[Vec<u8>] {
        self.deposit_data.as_slice()
    }

    /// Signature of the sequencer
    pub fn signature(&self) -> &[u8] {
        self.signature.as_slice()
    }

    /// L1 fee rate
    pub fn l1_fee_rate(&self) -> u128 {
        self.l1_fee_rate
    }

    /// Public key of sequencer
    pub fn pub_key(&self) -> &[u8] {
        self.pub_key.as_slice()
    }

    /// Sets l1 fee rate
    pub fn set_l1_fee_rate(&mut self, l1_fee_rate: u128) {
        self.l1_fee_rate = l1_fee_rate;
    }

    /// Sets da slot hash
    pub fn set_da_slot_hash(&mut self, da_slot_hash: [u8; 32]) {
        self.da_slot_hash = da_slot_hash;
    }

    /// Sequencer block timestamp
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }
}
