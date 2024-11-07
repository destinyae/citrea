use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use anyhow::anyhow;
use borsh::{BorshDeserialize, BorshSerialize};
use citrea_common::cache::L1BlockCache;
use citrea_common::da::extract_sequencer_commitments;
use citrea_common::utils::{check_l2_range_exists, filter_out_proven_commitments};
use citrea_primitives::forks::FORKS;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sov_db::ledger_db::BatchProverLedgerOps;
use sov_db::schema::types::{BatchNumber, StoredBatchProof, StoredBatchProofOutput};
use sov_modules_api::{BlobReaderTrait, SlotData, SpecId, Zkvm};
use sov_rollup_interface::da::{BlockHeaderTrait, DaNamespace, DaSpec, SequencerCommitment};
use sov_rollup_interface::fork::fork_from_block_number;
use sov_rollup_interface::rpc::SoftConfirmationStatus;
use sov_rollup_interface::services::da::DaService;
use sov_rollup_interface::zk::{BatchProofCircuitInput, BatchProofCircuitOutput, Proof, ZkvmHost};
use sov_stf_runner::ProverService;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::da_block_handler::{
    break_sequencer_commitments_into_groups, get_batch_proof_circuit_input_from_commitments,
};
use crate::errors::L1ProcessingError;

pub(crate) async fn data_to_prove<Da, DB, StateRoot, Witness>(
    da_service: Arc<Da>,
    ledger: DB,
    sequencer_pub_key: Vec<u8>,
    sequencer_da_pub_key: Vec<u8>,
    l1_block_cache: Arc<Mutex<L1BlockCache<Da>>>,
    l1_block: <Da as DaService>::FilteredBlock,
    group_commitments: Option<bool>,
) -> Result<
    (
        Vec<SequencerCommitment>,
        Vec<BatchProofCircuitInput<'static, StateRoot, Witness, Da::Spec>>,
    ),
    L1ProcessingError,
>
where
    Da: DaService,
    DB: BatchProverLedgerOps + Clone + Send + Sync + 'static,
    StateRoot: BorshDeserialize
        + BorshSerialize
        + Serialize
        + DeserializeOwned
        + Clone
        + AsRef<[u8]>
        + Debug,
    Witness: Default + BorshDeserialize + Serialize + DeserializeOwned,
{
    let l1_height = l1_block.header().height();

    let (mut da_data, inclusion_proof, completeness_proof) =
        da_service.extract_relevant_blobs_with_proof(&l1_block, DaNamespace::ToBatchProver);

    // if we don't do this, the zk circuit can't read the sequencer commitments
    da_data.iter_mut().for_each(|blob| {
        blob.full_data();
    });

    let sequencer_commitments: Vec<SequencerCommitment> = extract_sequencer_commitments::<Da>(
        da_service.clone(),
        l1_block.clone(),
        &sequencer_da_pub_key,
    );

    if sequencer_commitments.is_empty() {
        return Err(L1ProcessingError::NoSeqCommitments {
            l1_height: l1_block.header().height(),
        });
    }

    // If the L2 range does not exist, we break off the local loop getting back to
    // the outer loop / select to make room for other tasks to run.
    // We retry the L1 block there as well.
    let start_block_number = sequencer_commitments[0].l2_start_block_number;
    let end_block_number =
        sequencer_commitments[sequencer_commitments.len() - 1].l2_end_block_number;

    // If range is not synced yet return error
    if !check_l2_range_exists(&ledger, start_block_number, end_block_number) {
        return Err(L1ProcessingError::L2RangeMissing {
            start_block_number,
            end_block_number,
        });
    }

    let (sequencer_commitments, preproven_commitments) =
        filter_out_proven_commitments(&ledger, &sequencer_commitments).map_err(|e| {
            L1ProcessingError::Other(format!("Error filtering out proven commitments: {}", e))
        })?;

    if sequencer_commitments.is_empty() {
        return Err(L1ProcessingError::DuplicateCommitments { l1_height });
    }

    let da_block_header_of_commitments: <<Da as DaService>::Spec as DaSpec>::BlockHeader =
        l1_block.header().clone();

    let ranges = match group_commitments {
        Some(true) => break_sequencer_commitments_into_groups(&ledger, &sequencer_commitments)
            .map_err(|e| {
                L1ProcessingError::Other(format!(
                    "Error breaking sequencer commitments into groups: {:?}",
                    e
                ))
            })?,
        _ => vec![(0..=sequencer_commitments.len() - 1)],
    };

    let mut batch_proof_circuit_inputs = vec![];

    for sequencer_commitments_range in ranges {
        let first_l2_height_of_l1 =
            sequencer_commitments[*sequencer_commitments_range.start()].l2_start_block_number;
        let last_l2_height_of_l1 =
            sequencer_commitments[*sequencer_commitments_range.end()].l2_end_block_number;
        let (
            state_transition_witnesses,
            soft_confirmations,
            da_block_headers_of_soft_confirmations,
        ) = get_batch_proof_circuit_input_from_commitments(
            &sequencer_commitments[sequencer_commitments_range.clone()],
            &da_service,
            &ledger,
            &l1_block_cache,
        )
        .await
        .map_err(|e| {
            L1ProcessingError::Other(format!(
                "Error getting state transition data from commitments: {:?}",
                e
            ))
        })?;
        let initial_state_root = ledger
            .get_l2_state_root::<StateRoot>(first_l2_height_of_l1 - 1)
            .map_err(|e| {
                L1ProcessingError::Other(format!("Error getting initial state root: {:?}", e))
            })?
            .expect("There should be a state root");
        let initial_batch_hash = ledger
            .get_soft_confirmation_by_number(&BatchNumber(first_l2_height_of_l1))
            .map_err(|e| {
                L1ProcessingError::Other(format!("Error getting initial batch hash: {:?}", e))
            })?
            .ok_or(L1ProcessingError::Other(format!(
                "Could not find soft batch at height {}",
                first_l2_height_of_l1
            )))?
            .prev_hash;

        let final_state_root = ledger
            .get_l2_state_root::<StateRoot>(last_l2_height_of_l1)
            .map_err(|e| {
                L1ProcessingError::Other(format!("Error getting final state root: {:?}", e))
            })?
            .expect("There should be a state root");

        let input: BatchProofCircuitInput<StateRoot, Witness, Da::Spec> = BatchProofCircuitInput {
            initial_state_root,
            final_state_root,
            prev_soft_confirmation_hash: initial_batch_hash,
            da_data: da_data.clone(),
            da_block_header_of_commitments: da_block_header_of_commitments.clone(),
            inclusion_proof: inclusion_proof.clone(),
            completeness_proof: completeness_proof.clone(),
            soft_confirmations,
            state_transition_witnesses,
            da_block_headers_of_soft_confirmations,
            preproven_commitments: preproven_commitments.to_vec(),
            sequencer_commitments_range: (
                *sequencer_commitments_range.start() as u32,
                *sequencer_commitments_range.end() as u32,
            ),
            sequencer_public_key: sequencer_pub_key.clone(),
            sequencer_da_public_key: sequencer_da_pub_key.clone(),
        };

        batch_proof_circuit_inputs.push(input);
    }

    Ok((sequencer_commitments, batch_proof_circuit_inputs))
}

pub(crate) async fn prove_l1<Da, Ps, Vm, DB, StateRoot, Witness>(
    prover_service: Arc<Ps>,
    ledger: DB,
    code_commitments_by_spec: HashMap<SpecId, Vm::CodeCommitment>,
    l1_block: Da::FilteredBlock,
    sequencer_commitments: Vec<SequencerCommitment>,
    inputs: Vec<BatchProofCircuitInput<'_, StateRoot, Witness, Da::Spec>>,
) -> anyhow::Result<()>
where
    Da: DaService,
    DB: BatchProverLedgerOps + Clone + Send + Sync + 'static,
    Vm: ZkvmHost + Zkvm,
    Ps: ProverService<DaService = Da>,
    StateRoot: BorshDeserialize
        + BorshSerialize
        + Serialize
        + DeserializeOwned
        + Clone
        + AsRef<[u8]>
        + Debug,
    Witness: Default + BorshSerialize + BorshDeserialize + Serialize + DeserializeOwned,
{
    let submitted_proofs = ledger
        .get_proofs_by_l1_height(l1_block.header().height())
        .map_err(|e| anyhow!("{e}"))?
        .unwrap_or(vec![]);

    // Add each non-proven proof's data to ProverService
    for input in inputs {
        if !state_transition_already_proven::<StateRoot, Witness, Da>(&input, &submitted_proofs) {
            prover_service
                .add_proof_data((borsh::to_vec(&input)?, vec![]))
                .await;
        }
    }

    // Prove all proofs in parallel
    let proofs = prover_service.prove().await?;

    let txs_and_proofs = prover_service.submit_proofs(proofs).await?;

    extract_and_store_proof::<DB, Da, Vm, StateRoot>(
        ledger.clone(),
        txs_and_proofs,
        code_commitments_by_spec.clone(),
    )
    .await
    .map_err(|e| anyhow!("{e}"))?;

    save_commitments(
        ledger.clone(),
        &sequencer_commitments,
        l1_block.header().height(),
    );

    Ok(())
}

pub(crate) fn state_transition_already_proven<StateRoot, Witness, Da>(
    input: &BatchProofCircuitInput<StateRoot, Witness, Da::Spec>,
    proofs: &Vec<StoredBatchProof>,
) -> bool
where
    Da: DaService,
    StateRoot: BorshDeserialize
        + BorshSerialize
        + Serialize
        + DeserializeOwned
        + Clone
        + AsRef<[u8]>
        + Debug,
    Witness: Default + BorshDeserialize + Serialize + DeserializeOwned,
{
    for proof in proofs {
        if proof.proof_output.initial_state_root == input.initial_state_root.as_ref()
            && proof.proof_output.final_state_root == input.final_state_root.as_ref()
            && proof.proof_output.sequencer_commitments_range == input.sequencer_commitments_range
        {
            return true;
        }
    }
    false
}

pub(crate) async fn extract_and_store_proof<DB, Da, Vm, StateRoot>(
    ledger_db: DB,
    txs_and_proofs: Vec<(<Da as DaService>::TransactionId, Proof)>,
    code_commitments_by_spec: HashMap<SpecId, Vm::CodeCommitment>,
) -> Result<(), anyhow::Error>
where
    Da: DaService,
    DB: BatchProverLedgerOps,
    Vm: ZkvmHost + Zkvm,
    StateRoot: BorshDeserialize
        + BorshSerialize
        + Serialize
        + DeserializeOwned
        + Clone
        + AsRef<[u8]>
        + Debug,
{
    for (tx_id, proof) in txs_and_proofs {
        let tx_id_u8 = tx_id.into();

        // l1_height => (tx_id, proof, transition_data)
        // save proof along with tx id to db, should be queryable by slot number or slot hash
        let transition_data = Vm::extract_output::<
            <Da as DaService>::Spec,
            BatchProofCircuitOutput<<Da as DaService>::Spec, StateRoot>,
        >(&proof)
        .expect("Proof should be deserializable");

        info!("Verifying proof!");

        let last_active_spec_id =
            fork_from_block_number(FORKS.to_vec(), transition_data.last_l2_height).spec_id;
        let code_commitment = code_commitments_by_spec
            .get(&last_active_spec_id)
            .expect("Proof public input must contain valid spec id");
        Vm::verify(proof.as_slice(), code_commitment)
            .map_err(|err| anyhow!("Failed to verify proof: {:?}. Skipping it...", err))?;

        debug!("transition data: {:?}", transition_data);

        let slot_hash = transition_data.da_slot_hash.into();

        let stored_batch_proof_output = StoredBatchProofOutput {
            initial_state_root: transition_data.initial_state_root.as_ref().to_vec(),
            final_state_root: transition_data.final_state_root.as_ref().to_vec(),
            state_diff: transition_data.state_diff,
            da_slot_hash: slot_hash,
            sequencer_commitments_range: transition_data.sequencer_commitments_range,
            sequencer_public_key: transition_data.sequencer_public_key,
            sequencer_da_public_key: transition_data.sequencer_da_public_key,
            preproven_commitments: transition_data.preproven_commitments,
            validity_condition: borsh::to_vec(&transition_data.validity_condition).unwrap(),
        };
        let l1_height = ledger_db
            .get_l1_height_of_l1_hash(slot_hash)?
            .expect("l1 height should exist");

        if let Err(e) = ledger_db.insert_batch_proof_data_by_l1_height(
            l1_height,
            tx_id_u8,
            proof,
            stored_batch_proof_output,
        ) {
            panic!("Failed to put proof data in the ledger db: {}", e);
        }
    }
    Ok(())
}

pub(crate) fn save_commitments<DB>(
    ledger_db: DB,
    sequencer_commitments: &[SequencerCommitment],
    l1_height: u64,
) where
    DB: BatchProverLedgerOps,
{
    for sequencer_commitment in sequencer_commitments.iter() {
        // Save commitments on prover ledger db
        ledger_db
            .update_commitments_on_da_slot(l1_height, sequencer_commitment.clone())
            .unwrap();

        let l2_start_height = sequencer_commitment.l2_start_block_number;
        let l2_end_height = sequencer_commitment.l2_end_block_number;
        for i in l2_start_height..=l2_end_height {
            ledger_db
                .put_soft_confirmation_status(BatchNumber(i), SoftConfirmationStatus::Proven)
                .unwrap_or_else(|_| {
                    panic!(
                        "Failed to put soft confirmation status in the ledger db {}",
                        i
                    )
                });
        }
    }
}
