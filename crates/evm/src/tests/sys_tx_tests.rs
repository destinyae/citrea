use std::collections::HashMap;
use std::str::FromStr;

use alloy_primitives::LogData;
use reth_primitives::constants::ETHEREUM_BLOCK_GAS_LIMIT;
use reth_primitives::{address, b256, hex, BlockNumberOrTag, Log, TxKind};
use reth_rpc_types::trace::geth::{GethDebugBuiltInTracerType, GethDebugTracerType, GethDebugTracingOptions};
use reth_rpc_types::{Block, BlockId, TransactionInput, TransactionRequest};
use revm::primitives::bitvec::ptr::hash;
use revm::primitives::{Bytes, KECCAK_EMPTY, U256};
use secp256k1::SecretKey;
use sov_modules_api::default_context::DefaultContext;
use sov_modules_api::hooks::HookSoftConfirmationInfo;
use sov_modules_api::utils::generate_address;
use sov_modules_api::{Context, Module, StateMapAccessor, StateVecAccessor};

use std::env;

use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use crate::call::CallMessage;
use crate::evm::primitive_types::Receipt;
use crate::evm::system_contracts::BitcoinLightClient;
use crate::smart_contracts::{BlockHashContract, LogsContract, SelfDestructorContract};
use crate::system_contracts::BitcoinLightClientContract::abi::contract;
use crate::system_contracts::Bridge;
use crate::tests::call_tests::{
    create_contract_message, create_contract_message_with_fee, get_evm_config_starting_base_fee,
    publish_event_message,
};
use crate::tests::utils::get_evm;
use crate::{AccountData, EvmConfig, SYSTEM_SIGNER};

use super::test_signer::TestSigner;

type C = DefaultContext;

#[test]
fn test_sys_bitcoin_light_client() {
    let (mut config, dev_signer, _) =
        get_evm_config_starting_base_fee(U256::from_str("1000000").unwrap(), None, 1);

    config_push_contracts(&mut config);

    let (evm, mut working_set) = get_evm(&config);

    assert_eq!(
        evm.receipts
            .iter(&mut working_set.accessory_state())
            .collect::<Vec<_>>(),
        [
            Receipt { // BitcoinLightClient::initializeBlockNumber(U256)
                receipt: reth_primitives::Receipt {
                    tx_type: reth_primitives::TxType::Eip1559,
                    success: true,
                    cumulative_gas_used: 48522,
                    logs: vec![]
                },
                gas_used: 48522,
                log_index_start: 0,
                diff_size: 168,
            },
            Receipt { // BitcoinLightClient::setBlockInfo(U256, U256)
                receipt: reth_primitives::Receipt {
                    tx_type: reth_primitives::TxType::Eip1559,
                    success: true,
                    cumulative_gas_used: 127013,
                    logs: vec![
                        Log {
                            address: BitcoinLightClient::address(),
                            data: LogData::new(
                                vec![b256!("32eff959e2e8d1609edc4b39ccf75900aa6c1da5719f8432752963fdf008234f")],
                                Bytes::from_static(&hex!("000000000000000000000000000000000000000000000000000000000000000201010101010101010101010101010101010101010101010101010101010101010202020202020202020202020202020202020202020202020202020202020202")),
                            ).unwrap(),
                        }
                    ]
                },
                gas_used: 78491,
                log_index_start: 0,
                diff_size: 296,
            },
            Receipt {
                receipt: reth_primitives::Receipt {
                    tx_type: reth_primitives::TxType::Eip1559,
                    success: true,
                    cumulative_gas_used: 385984,
                    logs: vec![
                        Log {
                            address: Bridge::address(),
                            data: LogData::new(
                                vec![b256!("fbe5b6cbafb274f445d7fed869dc77a838d8243a22c460de156560e8857cad03")],
                                Bytes::from_static(&hex!("0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000deaddeaddeaddeaddeaddeaddeaddeaddeaddead")),
                            ).unwrap(),
                        },
                        Log {
                            address: Bridge::address(),
                            data: LogData::new(
                                vec![b256!("89ed79f38bee253aee2fb8d52df0d71b4aaf0843800d093a499a55eeca455c34")],
                                Bytes::from_static(&hex!("00000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000140000000000000000000000000000000000000000000000000000000000000000500000000000000000000000000000000000000000000000000000000000000b5d2205daf577048c5e5a9a75d0a924ed03e226c3304f4a2f01c65ca1dab73522e6b8bad206228eba653cf1819bcfc1bc858630e5ae373eec1a9924322a5fe8445c5e76027ad201521d65f64be3f71b71ca462220f13c77b251027f6ca443a483353a96fbce222ad200fabeed269694ee83d9b3343a571202e68af65d05feda61dbed0c4bdb256a6eaad2000326d6f721c03dc5f1d8817d8f8ee890a95a2eeda0d4d9a01b1cc9b7b1b724dac006306636974726561140000000000000000000000000000000000000000000000000000000000000000000000000000000000000a0800000000000f42406800000000000000000000000000000000000000000000"))
                            ).unwrap(),
                        }
                    ]
                },
                gas_used: 258971,
                log_index_start: 1,
                diff_size: 744,
            }
        ]
    );

    let l1_fee_rate = 1;

    let system_account = evm.accounts.get(&SYSTEM_SIGNER, &mut working_set).unwrap();
    // The system caller balance is unchanged(if exists)/or should be 0
    assert_eq!(system_account.info.balance, U256::from(0));
    assert_eq!(system_account.info.nonce, 3);

    let hash = evm
        .get_call(
            TransactionRequest {
                to: Some(TxKind::Call(BitcoinLightClient::address())),
                input: TransactionInput::new(BitcoinLightClient::get_block_hash(1)),
                ..Default::default()
            },
            None,
            None,
            None,
            &mut working_set,
        )
        .unwrap();

    let merkle_root = evm
        .get_call(
            TransactionRequest {
                to: Some(TxKind::Call(BitcoinLightClient::address())),
                input: TransactionInput::new(BitcoinLightClient::get_witness_root_by_number(1)),
                ..Default::default()
            },
            None,
            None,
            None,
            &mut working_set,
        )
        .unwrap();

    assert_eq!(hash.as_ref(), &[1u8; 32]);
    assert_eq!(merkle_root.as_ref(), &[2u8; 32]);

    // New L1 block №2
    evm.begin_soft_confirmation_hook(
        &HookSoftConfirmationInfo {
            da_slot_hash: [2u8; 32],
            da_slot_height: 2,
            da_slot_txs_commitment: [3u8; 32],
            pre_state_root: [10u8; 32].to_vec(),
            pub_key: vec![],
            deposit_data: vec![],
            l1_fee_rate,
            timestamp: 42,
        },
        &mut working_set,
    );
    {
        let sender_address = generate_address::<C>("sender");
        let sequencer_address = generate_address::<C>("sequencer");
        let context = C::new(sender_address, sequencer_address, 1);

        let deploy_message =
            create_contract_message_with_fee(&dev_signer, 0, BlockHashContract::default(), 1);

        evm.call(
            CallMessage {
                txs: vec![deploy_message],
            },
            &context,
            &mut working_set,
        )
        .unwrap();
    }
    evm.end_soft_confirmation_hook(&mut working_set);
    evm.finalize_hook(&[99u8; 32].into(), &mut working_set.accessory_state());

    let system_account = evm.accounts.get(&SYSTEM_SIGNER, &mut working_set).unwrap();
    // The system caller balance is unchanged(if exists)/or should be 0
    assert_eq!(system_account.info.balance, U256::from(0));
    assert_eq!(system_account.info.nonce, 4);

    let receipts: Vec<_> = evm
        .receipts
        .iter(&mut working_set.accessory_state())
        .collect();
    assert_eq!(receipts.len(), 5); // 3 from first L2 block + 2 from second L2 block
    let receipts = receipts[3..].to_vec();

    assert_eq!(receipts,
        [
            Receipt { // BitcoinLightClient::setBlockInfo(U256, U256)
                receipt: reth_primitives::Receipt {
                    tx_type: reth_primitives::TxType::Eip1559,
                    success: true,
                    cumulative_gas_used: 78491,
                    logs: vec![
                        Log {
                            address: BitcoinLightClient::address(),
                            data: LogData::new(
                                vec![b256!("32eff959e2e8d1609edc4b39ccf75900aa6c1da5719f8432752963fdf008234f")],
                                Bytes::from_static(&hex!("000000000000000000000000000000000000000000000000000000000000000302020202020202020202020202020202020202020202020202020202020202020303030303030303030303030303030303030303030303030303030303030303")),
                            ).unwrap(),
                        }
                    ]
                },
                gas_used: 78491,
                log_index_start: 0,
                diff_size: 296,
            },
            Receipt {
                receipt: reth_primitives::Receipt {
                    tx_type: reth_primitives::TxType::Eip1559,
                    success: true,
                    cumulative_gas_used: 192726,
                    logs: vec![]
                },
                gas_used: 114235,
                log_index_start: 1,
                diff_size: 477,
            },
        ]
    );

    let coinbase_account = evm
        .accounts
        .get(&config.coinbase, &mut working_set)
        .unwrap();
    assert_eq!(coinbase_account.info.balance, U256::from(114235 + 477));

    let hash = evm
        .get_call(
            TransactionRequest {
                to: Some(TxKind::Call(BitcoinLightClient::address())),
                input: TransactionInput::new(BitcoinLightClient::get_block_hash(2)),
                ..Default::default()
            },
            None,
            None,
            None,
            &mut working_set,
        )
        .unwrap();

    let merkle_root = evm
        .get_call(
            TransactionRequest {
                to: Some(TxKind::Call(BitcoinLightClient::address())),
                input: TransactionInput::new(BitcoinLightClient::get_witness_root_by_number(2)),
                ..Default::default()
            },
            None,
            None,
            None,
            &mut working_set,
        )
        .unwrap();

    assert_eq!(hash.as_ref(), &[2u8; 32]);
    assert_eq!(merkle_root.as_ref(), &[3u8; 32]);
}

#[test]
fn test_sys_tx_gas_usage_effect_on_block_gas_limit() {
    // This test also tests evm checking gas usage and not just the tx gas limit when including txs in block after checking available block limit
    // For example txs below have 1_000_000 gas limit, the block used to stuck at 29_030_000 gas usage but now can utilize the whole block gas limit
    let (mut config, dev_signer, contract_addr) = get_evm_config_starting_base_fee(
        U256::from_str("100000000000000000000").unwrap(),
        Some(ETHEREUM_BLOCK_GAS_LIMIT),
        1,
    );

    config_push_contracts(&mut config);

    let (evm, mut working_set) = get_evm(&config);
    let l1_fee_rate = 0;

    let sender_address = generate_address::<C>("sender");
    let sequencer_address = generate_address::<C>("sequencer");
    let context = C::new(sender_address, sequencer_address, 1);

    evm.begin_soft_confirmation_hook(
        &HookSoftConfirmationInfo {
            da_slot_hash: [5u8; 32],
            da_slot_height: 1,
            da_slot_txs_commitment: [42u8; 32],
            pre_state_root: [10u8; 32].to_vec(),
            pub_key: vec![],
            deposit_data: vec![],
            l1_fee_rate: 1,
            timestamp: 0,
        },
        &mut working_set,
    );
    {
        // deploy logs contract
        evm.call(
            CallMessage {
                txs: vec![create_contract_message(
                    &dev_signer,
                    0,
                    LogsContract::default(),
                )],
            },
            &context,
            &mut working_set,
        )
        .unwrap();
    }
    evm.end_soft_confirmation_hook(&mut working_set);
    evm.finalize_hook(&[99u8; 32].into(), &mut working_set.accessory_state());

    evm.begin_soft_confirmation_hook(
        &HookSoftConfirmationInfo {
            da_slot_hash: [10u8; 32],
            da_slot_height: 2,
            da_slot_txs_commitment: [43u8; 32],
            pre_state_root: [10u8; 32].to_vec(),
            pub_key: vec![],
            deposit_data: vec![],
            l1_fee_rate,
            timestamp: 0,
        },
        &mut working_set,
    );
    {
        let context = C::new(sender_address, sequencer_address, 2);

        let sys_tx_gas_usage = evm.get_pending_txs_cumulative_gas_used(&mut working_set);
        assert_eq!(sys_tx_gas_usage, 78491);

        let mut rlp_transactions = Vec::new();

        // Check: Given now we also push bridge contract, is the following calculation correct?

        // the amount of gas left is 30_000_000 - 78491 = 29_921_509
        // send barely enough gas to reach the limit
        // one publish event message is 26388 gas
        // 29921509 / 26388 = 1133.09
        // so there cannot be more than 1133 messages
        for i in 0..11350 {
            rlp_transactions.push(publish_event_message(
                contract_addr,
                &dev_signer,
                i + 1,
                "hello".to_string(),
            ));
        }

        evm.call(
            CallMessage {
                txs: rlp_transactions,
            },
            &context,
            &mut working_set,
        )
        .unwrap();
    }
    evm.end_soft_confirmation_hook(&mut working_set);
    evm.finalize_hook(&[99u8; 32].into(), &mut working_set.accessory_state());

    let block = evm
        .get_block_by_number(Some(BlockNumberOrTag::Latest), None, &mut working_set)
        .unwrap()
        .unwrap();

    assert_eq!(block.header.gas_limit, ETHEREUM_BLOCK_GAS_LIMIT as _);
    assert!(block.header.gas_used <= block.header.gas_limit);

    // In total there should only be 1134 transactions 1 is system tx others are contract calls
    assert!(
        block.transactions.hashes().len() == 1134,
        "Some transactions should be dropped because of gas limit"
    );
}

#[test]
fn test_bridge() {
    let (mut config, _, _) =
        get_evm_config_starting_base_fee(U256::from_str("1000000").unwrap(), None, 1);

    config_push_contracts(&mut config);

    let (evm, mut working_set) = get_evm(&config);

    evm.begin_soft_confirmation_hook(
        &HookSoftConfirmationInfo {
            da_slot_height: 2,
            da_slot_hash: [2u8; 32],
            da_slot_txs_commitment: [
                136, 147, 225, 201, 35, 145, 64, 167, 182, 140, 185, 55, 22, 224, 150, 42, 51, 86,
                214, 251, 181, 122, 169, 246, 188, 29, 186, 32, 227, 33, 199, 38,
            ],
            pre_state_root: [1u8; 32].to_vec(),
            pub_key: vec![],
            deposit_data: vec![[
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 32, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 128, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 4, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 42, 1, 196, 196, 205, 156,
                93, 62, 54, 134, 133, 188, 6, 17, 153, 42, 62, 155, 138, 8, 111, 222, 48, 192, 86,
                41, 210, 202, 111, 100, 49, 6, 36, 123, 0, 0, 0, 0, 0, 253, 255, 255, 255, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 87, 2, 197, 63,
                15, 0, 0, 0, 0, 0, 34, 81, 32, 225, 85, 228, 181, 8, 114, 26, 130, 4, 159, 125,
                249, 18, 119, 121, 134, 147, 142, 99, 173, 85, 230, 58, 42, 39, 210, 102, 158, 156,
                54, 47, 183, 74, 1, 0, 0, 0, 0, 0, 0, 34, 0, 32, 74, 232, 21, 114, 240, 110, 27,
                136, 253, 92, 237, 122, 26, 0, 9, 69, 67, 46, 131, 225, 85, 30, 111, 114, 30, 233,
                192, 11, 140, 195, 50, 96, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 91, 7, 64, 85, 100,
                226, 121, 160, 231, 130, 160, 201, 56, 39, 35, 161, 143, 216, 21, 211, 206, 127,
                229, 78, 29, 6, 86, 241, 85, 191, 62, 174, 148, 71, 7, 97, 25, 170, 78, 173, 238,
                251, 184, 7, 3, 139, 103, 184, 9, 84, 28, 37, 39, 39, 91, 248, 166, 240, 149, 245,
                51, 48, 45, 10, 151, 90, 134, 64, 58, 4, 251, 18, 243, 51, 241, 78, 218, 137, 248,
                84, 193, 73, 6, 249, 29, 144, 62, 120, 43, 235, 170, 173, 3, 241, 236, 171, 253,
                71, 17, 237, 81, 214, 38, 47, 206, 119, 2, 116, 56, 203, 107, 84, 255, 102, 133,
                42, 245, 35, 173, 250, 41, 110, 193, 18, 121, 214, 157, 81, 81, 115, 91, 237, 64,
                21, 17, 223, 104, 155, 182, 45, 200, 209, 237, 114, 78, 88, 157, 251, 106, 70, 76,
                150, 27, 223, 254, 87, 62, 121, 250, 18, 141, 166, 53, 181, 63, 41, 28, 81, 51, 20,
                84, 115, 122, 154, 139, 187, 182, 208, 212, 16, 122, 183, 103, 149, 223, 86, 216,
                191, 246, 117, 102, 59, 111, 120, 22, 223, 62, 64, 253, 145, 239, 196, 249, 255,
                135, 5, 208, 64, 144, 150, 213, 166, 66, 98, 4, 23, 151, 165, 220, 201, 209, 179,
                201, 162, 185, 98, 0, 228, 44, 29, 230, 117, 232, 11, 123, 162, 71, 201, 73, 125,
                209, 236, 189, 139, 56, 160, 205, 48, 238, 29, 185, 43, 229, 103, 117, 247, 252,
                85, 166, 29, 59, 232, 64, 189, 1, 191, 87, 25, 32, 77, 193, 98, 33, 84, 159, 168,
                209, 181, 157, 80, 130, 164, 59, 101, 196, 190, 247, 124, 131, 53, 156, 111, 105,
                196, 18, 8, 177, 1, 118, 217, 178, 150, 165, 172, 205, 126, 106, 54, 246, 54, 95,
                47, 16, 155, 156, 123, 135, 135, 4, 44, 241, 144, 188, 76, 181, 157, 173, 210, 32,
                93, 175, 87, 112, 72, 197, 229, 169, 167, 93, 10, 146, 78, 208, 62, 34, 108, 51, 4,
                244, 162, 240, 28, 101, 202, 29, 171, 115, 82, 46, 107, 139, 173, 32, 98, 40, 235,
                166, 83, 207, 24, 25, 188, 252, 27, 200, 88, 99, 14, 90, 227, 115, 238, 193, 169,
                146, 67, 34, 165, 254, 132, 69, 197, 231, 96, 39, 173, 32, 21, 33, 214, 95, 100,
                190, 63, 113, 183, 28, 164, 98, 34, 15, 19, 199, 123, 37, 16, 39, 246, 202, 68, 58,
                72, 51, 83, 169, 111, 188, 226, 34, 173, 32, 15, 171, 238, 210, 105, 105, 78, 232,
                61, 155, 51, 67, 165, 113, 32, 46, 104, 175, 101, 208, 95, 237, 166, 29, 190, 208,
                196, 189, 178, 86, 166, 234, 173, 32, 0, 50, 109, 111, 114, 28, 3, 220, 95, 29,
                136, 23, 216, 248, 238, 137, 10, 149, 162, 238, 218, 13, 77, 154, 1, 177, 204, 155,
                123, 27, 114, 77, 172, 0, 99, 6, 99, 105, 116, 114, 101, 97, 20, 1, 1, 1, 1, 1, 1,
                1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 8, 0, 0, 0, 0, 0, 15, 66, 64, 104, 65,
                193, 147, 199, 55, 141, 150, 81, 138, 117, 68, 136, 33, 196, 247, 200, 244, 186,
                231, 206, 96, 248, 4, 208, 61, 31, 6, 40, 221, 93, 208, 245, 222, 81, 15, 41, 81,
                255, 251, 84, 130, 89, 213, 171, 185, 243, 81, 190, 143, 148, 3, 28, 156, 232, 140,
                232, 56, 180, 13, 124, 236, 124, 96, 110, 12, 122, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0,
            ]
            .to_vec()],
            l1_fee_rate: 1,
            timestamp: 0,
        },
        &mut working_set,
    );
    evm.end_soft_confirmation_hook(&mut working_set);
    evm.finalize_hook(&[99u8; 32].into(), &mut working_set.accessory_state());

    let recipient_address = address!("0101010101010101010101010101010101010101");
    let recipient_account = evm
        .accounts
        .get(&recipient_address, &mut working_set)
        .unwrap();

    assert_eq!(
        recipient_account.info.balance,
        U256::from_str("0x2386f26fc10000").unwrap(),
    );
}

#[test]
fn test_upgrade_light_client() {
    initialize_logging(tracing::Level::INFO);
    let (mut config, _, _) =
        get_evm_config_starting_base_fee(U256::from_str("1000000000000000000000").unwrap(), None, 1);

    config_push_contracts(&mut config);

    // False bitcoin light client implementation, returns dead address on block hash query
    config.data.push(AccountData::new(
        address!("deAD00000000000000000000000000000000dEAd"),
        U256::ZERO,
        Bytes::from_static(&hex!("6080604052600436106101145760003560e01c8063715018a6116100a0578063d269a03e11610064578063d269a03e14610332578063d761753e14610352578063e30c39781461037a578063ee82ac5e1461038f578063f2fde38b146103cf57600080fd5b8063715018a61461027057806379ba5097146102855780638da5cb5b1461029a578063a91d8b3d146102c7578063ad3cb1cc146102f457600080fd5b80634f1ef286116100e75780634f1ef286146101c85780634ffd344a146101db57806352d1902d1461020b57806357e871e71461022057806361b207e21461023657600080fd5b80630466efc4146101195780630e27bc11146101595780631f5783331461017b57806334cdf78d1461019b575b600080fd5b34801561012557600080fd5b50610146610134366004610cec565b60009081526002602052604090205490565b6040519081526020015b60405180910390f35b34801561016557600080fd5b50610179610174366004610d05565b6103ef565b005b34801561018757600080fd5b50610179610196366004610cec565b610518565b3480156101a757600080fd5b506101466101b6366004610cec565b60016020526000908152604090205481565b6101796101d6366004610d59565b6105c6565b3480156101e757600080fd5b506101fb6101f6366004610e64565b6105dd565b6040519015158152602001610150565b34801561021757600080fd5b50610146610603565b34801561022c57600080fd5b5061014660005481565b34801561024257600080fd5b50610146610251366004610cec565b6000908152600160209081526040808320548352600290915290205490565b34801561027c57600080fd5b50610179610632565b34801561029157600080fd5b50610179610646565b3480156102a657600080fd5b506102af61068e565b6040516001600160a01b039091168152602001610150565b3480156102d357600080fd5b506101466102e2366004610cec565b60026020526000908152604090205481565b34801561030057600080fd5b50610325604051806040016040528060058152602001640352e302e360dc1b81525081565b6040516101509190610ee3565b34801561033e57600080fd5b506101fb61034d366004610e64565b6106c3565b34801561035e57600080fd5b506102af73deaddeaddeaddeaddeaddeaddeaddeaddeaddead81565b34801561038657600080fd5b506102af6106d2565b34801561039b57600080fd5b506101466103aa366004610cec565b507fdeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddeaddead90565b3480156103db57600080fd5b506101796103ea366004610f16565b6106fb565b3373deaddeaddeaddeaddeaddeaddeaddeaddeaddead146104575760405162461bcd60e51b815260206004820152601f60248201527f63616c6c6572206973206e6f74207468652073797374656d2063616c6c65720060448201526064015b60405180910390fd5b600080549081900361049d5760405162461bcd60e51b815260206004820152600f60248201526e139bdd081a5b9a5d1a585b1a5e9959608a1b604482015260640161044e565b60008181526001602081905260409091208490556104bc908290610f31565b6000908155838152600260209081526040808320859055915482519081529081018590529081018390527f32eff959e2e8d1609edc4b39ccf75900aa6c1da5719f8432752963fdf008234f9060600160405180910390a1505050565b3373deaddeaddeaddeaddeaddeaddeaddeaddeaddead1461057b5760405162461bcd60e51b815260206004820152601f60248201527f63616c6c6572206973206e6f74207468652073797374656d2063616c6c657200604482015260640161044e565b600054156105c15760405162461bcd60e51b8152602060048201526013602482015272105b1c9958591e481a5b9a5d1a585b1a5e9959606a1b604482015260640161044e565b600055565b6105cf82610780565b6105d98282610788565b5050565b6000858152600160205260408120546105f9908686868661085c565b9695505050505050565b600061060d6108ba565b507f360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc90565b61063a610903565b6106446000610935565b565b33806106506106d2565b6001600160a01b0316146106825760405163118cdaa760e01b81526001600160a01b038216600482015260240161044e565b61068b81610935565b50565b6000807f9016d09d72d40fdae2fd8ceac6b6234c7706214fd39c1cd1e609a0528c1993005b546001600160a01b031692915050565b60006105f9868686868661085c565b6000807f237e158222e3e6968b72b9db0d8043aacf074ad9f650f0d1606b4d82ee432c006106b3565b610703610903565b7f237e158222e3e6968b72b9db0d8043aacf074ad9f650f0d1606b4d82ee432c0080546001600160a01b0319166001600160a01b038316908117825561074761068e565b6001600160a01b03167f38d16b8cac22d99fc7c124b9cd0de2d3fa1faef420bfe791d8c362d765e2270060405160405180910390a35050565b61068b610903565b816001600160a01b03166352d1902d6040518163ffffffff1660e01b8152600401602060405180830381865afa9250505080156107e2575060408051601f3d908101601f191682019092526107df91810190610f52565b60015b61080a57604051634c9c8ce360e01b81526001600160a01b038316600482015260240161044e565b7f360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc811461084d57604051632a87526960e21b81526004810182905260240161044e565b610857838361096d565b505050565b6000858152600260209081526040808320548151601f8701849004840281018401909252858252916108af91889184919089908990819084018382808284376000920191909152508992506109c3915050565b979650505050505050565b306001600160a01b037f000000000000000000000000000000000000000000000000000000000000000016146106445760405163703e46dd60e11b815260040160405180910390fd5b3361090c61068e565b6001600160a01b0316146106445760405163118cdaa760e01b815233600482015260240161044e565b7f237e158222e3e6968b72b9db0d8043aacf074ad9f650f0d1606b4d82ee432c0080546001600160a01b03191681556105d982610a01565b61097682610a72565b6040516001600160a01b038316907fbc7cd75a20ee27fd9adebab32041f755214dbc6bffa90cc0225b39da2e5c2d3b90600090a28051156109bb576108578282610ae9565b6105d9610b61565b600083851480156109d2575081155b80156109dd57508251155b156109ea575060016109f9565b6109f685848685610b80565b90505b949350505050565b7f9016d09d72d40fdae2fd8ceac6b6234c7706214fd39c1cd1e609a0528c19930080546001600160a01b031981166001600160a01b03848116918217845560405192169182907f8be0079c531659141344cd1fd0a4f28419497f9722a3daafe3b4186f6b6457e090600090a3505050565b806001600160a01b03163b600003610aa857604051634c9c8ce360e01b81526001600160a01b038216600482015260240161044e565b7f360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc80546001600160a01b0319166001600160a01b0392909216919091179055565b6060600080846001600160a01b031684604051610b069190610f6b565b600060405180830381855af49150503d8060008114610b41576040519150601f19603f3d011682016040523d82523d6000602084013e610b46565b606091505b5091509150610b56858383610c2a565b925050505b92915050565b34156106445760405163b398979f60e01b815260040160405180910390fd5b600060208451610b909190610f87565b15610b9d575060006109f9565b8351600003610bae575060006109f9565b818560005b8651811015610c1d57610bc7600284610f87565b600103610beb57610be4610bde8883016020015190565b83610c89565b9150610c04565b610c0182610bfc8984016020015190565b610c89565b91505b60019290921c91610c16602082610f31565b9050610bb3565b5090931495945050505050565b606082610c3f57610c3a82610c95565b610c82565b8151158015610c5657506001600160a01b0384163b155b15610c7f57604051639996b31560e01b81526001600160a01b038516600482015260240161044e565b50805b9392505050565b6000610c828383610cbe565b805115610ca55780518082602001fd5b604051630a12f52160e11b815260040160405180910390fd5b60008260005281602052602060006040600060025afa50602060006020600060025afa505060005192915050565b600060208284031215610cfe57600080fd5b5035919050565b60008060408385031215610d1857600080fd5b50508035926020909101359150565b80356001600160a01b0381168114610d3e57600080fd5b919050565b634e487b7160e01b600052604160045260246000fd5b60008060408385031215610d6c57600080fd5b610d7583610d27565b9150602083013567ffffffffffffffff80821115610d9257600080fd5b818501915085601f830112610da657600080fd5b813581811115610db857610db8610d43565b604051601f8201601f19908116603f01168101908382118183101715610de057610de0610d43565b81604052828152886020848701011115610df957600080fd5b8260208601602083013760006020848301015280955050505050509250929050565b60008083601f840112610e2d57600080fd5b50813567ffffffffffffffff811115610e4557600080fd5b602083019150836020828501011115610e5d57600080fd5b9250929050565b600080600080600060808688031215610e7c57600080fd5b8535945060208601359350604086013567ffffffffffffffff811115610ea157600080fd5b610ead88828901610e1b565b96999598509660600135949350505050565b60005b83811015610eda578181015183820152602001610ec2565b50506000910152565b6020815260008251806020840152610f02816040850160208701610ebf565b601f01601f19169190910160400192915050565b600060208284031215610f2857600080fd5b610c8282610d27565b80820180821115610b5b57634e487b7160e01b600052601160045260246000fd5b600060208284031215610f6457600080fd5b5051919050565b60008251610f7d818460208701610ebf565b9190910192915050565b600082610fa457634e487b7160e01b600052601260045260246000fd5b50069056fea2646970667358221220cb22b346a23078243cb869a68fb68e5704b567765a15214f1d3d3d7cadb59a9764736f6c63430008190033")),
        0,
        HashMap::new()
    ));

    // secret key is 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
    let contract_owner = TestSigner::new(secp256k1::SecretKey::from_slice(&[
        0xac, 0x09, 0x74, 0xbe, 0xc3, 0x9a, 0x17, 0xe3, 0x6b, 0xa4, 0xa6, 0xb4, 0xd2, 0x38,
        0xff, 0x94, 0x4b, 0xac, 0xb4, 0x78, 0xcb, 0xed, 0x5e, 0xfc, 0xae, 0x78, 0x4d, 0x7b,
        0xf4, 0xf2, 0xff, 0x80,
    ]).unwrap());

    config.data.push(AccountData {
        address: contract_owner.address(),
        balance: U256::from_str("1000000000000000000000").unwrap(),
        code_hash: KECCAK_EMPTY,
        code: Bytes::default(),
        nonce: 0,
        storage: Default::default(),
    });

    let (evm, mut working_set) = get_evm(&config);

    let sender_address = generate_address::<C>("sender");
    let sequencer_address = generate_address::<C>("sequencer");
    let context = C::new(sender_address, sequencer_address, 1);

    evm.begin_soft_confirmation_hook(
        &HookSoftConfirmationInfo {
            da_slot_hash: [5u8; 32],
            da_slot_height: 1,
            da_slot_txs_commitment: [42u8; 32],
            pre_state_root: [10u8; 32].to_vec(),
            pub_key: vec![],
            deposit_data: vec![],
            l1_fee_rate: 1,
            timestamp: 0,
        },
        &mut working_set,
    );

    let upgrade_tx = contract_owner.sign_default_transaction(TxKind::Call(BitcoinLightClient::address()), BitcoinLightClient::upgrade_to_and_call(address!("deAD00000000000000000000000000000000dEAd"), Bytes::default()).to_vec(), 0, 0).unwrap();
    evm.call(
        CallMessage {
            txs: vec![upgrade_tx],
        },
        &context,
        &mut working_set,
    ).unwrap();

    evm.end_soft_confirmation_hook(&mut working_set);
    evm.finalize_hook(&[99u8; 32].into(), &mut working_set.accessory_state());

    let trace =
        evm.trace_block_transactions_by_number(
            2u64,
            Some(GethDebugTracingOptions::default().with_tracer(
                GethDebugTracerType::BuiltInTracer(GethDebugBuiltInTracerType::CallTracer),
            )),
            None,
            &mut working_set,
        );

    println!("{:?}", trace);

    // let new_block = evm.get_block_receipts(BlockId::Number(BlockNumberOrTag::Latest), &mut working_set);
    // let new_receipts: Vec<_> = new_block.unwrap().unwrap().into_iter().collect();
    // println!("{:?}", new_receipts);

    // let code = evm.get_code(address!("dead00000000000000000000000000000000dead"), Some(BlockNumberOrTag::Latest), &mut working_set).unwrap();
    let hash = evm
        .get_call(
            TransactionRequest {
                to: Some(TxKind::Call(BitcoinLightClient::address())),
                input: TransactionInput::new(BitcoinLightClient::get_block_hash(0)),
                ..Default::default()
            },
            None,
            None,
            None,
            &mut working_set,
        )
        .unwrap();

    println!("{:?}", hash);
    }

pub fn initialize_logging(level: Level) {
    let env_filter = EnvFilter::from_str(&env::var("RUST_LOG").unwrap_or_else(|_| {
        let debug_components = vec![
            level.as_str().to_owned(),
            "jmt=info".to_owned(),
            "hyper=info".to_owned(),
            // Limit output as much as possible, use WARN.
            "risc0_zkvm=warn".to_owned(),
            "guest_execution=info".to_owned(),
            "jsonrpsee-server=info".to_owned(),
            "reqwest=info".to_owned(),
            "sov_schema_db=info".to_owned(),
            "sov_prover_storage_manager=info".to_owned(),
            // Limit output as much as possible, use WARN.
            "tokio_postgres=warn".to_owned(),
        ];
        debug_components.join(",")
    }))
    .unwrap();
    if std::env::var("JSON_LOGS").is_ok() {
        tracing_subscriber::registry()
            .with(fmt::layer().json())
            .with(env_filter)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(fmt::layer())
            .with(env_filter)
            .init();
    }

    log_panics::init();
}

fn config_push_contracts(config: &mut EvmConfig) {
    config.data.push(AccountData::new(
        BitcoinLightClient::address(),
        U256::ZERO,
        Bytes::from_static(&hex!("6080604052600a600c565b005b60186014601a565b6050565b565b5f604b7f360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc546001600160a01b031690565b905090565b365f80375f80365f845af43d5f803e8080156069573d5ff35b3d5ffdfea26469706673582212201698835cd7a9e8303f44009c3f144a4dbbfa3ab8ec0bca6489bb06bb1bda401164736f6c63430008180033")),
        0,
        [
            (U256::from_be_slice(&hex!("360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc")), U256::from_be_slice(&hex!("3200000000000000000000000000000000000001"))),
            (U256::from_be_slice(&hex!("9016d09d72d40fdae2fd8ceac6b6234c7706214fd39c1cd1e609a0528c199300")), U256::from_be_slice(&hex!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"))),
        ].into_iter().collect(),
    ));

    config.data.push(AccountData::new(
        address!("3200000000000000000000000000000000000001"),
        U256::ZERO,
        Bytes::from_static(&hex!("6080604052600436106101145760003560e01c8063715018a6116100a0578063d269a03e11610064578063d269a03e14610332578063d761753e14610352578063e30c39781461037a578063ee82ac5e1461038f578063f2fde38b146103bc57600080fd5b8063715018a61461027057806379ba5097146102855780638da5cb5b1461029a578063a91d8b3d146102c7578063ad3cb1cc146102f457600080fd5b80634f1ef286116100e75780634f1ef286146101c85780634ffd344a146101db57806352d1902d1461020b57806357e871e71461022057806361b207e21461023657600080fd5b80630466efc4146101195780630e27bc11146101595780631f5783331461017b57806334cdf78d1461019b575b600080fd5b34801561012557600080fd5b50610146610134366004610d50565b60009081526002602052604090205490565b6040519081526020015b60405180910390f35b34801561016557600080fd5b50610179610174366004610d69565b6103dc565b005b34801561018757600080fd5b50610179610196366004610d50565b610505565b3480156101a757600080fd5b506101466101b6366004610d50565b60016020526000908152604090205481565b6101796101d6366004610dbd565b6105b3565b3480156101e757600080fd5b506101fb6101f6366004610ec8565b6105d2565b6040519015158152602001610150565b34801561021757600080fd5b506101466105f8565b34801561022c57600080fd5b5061014660005481565b34801561024257600080fd5b50610146610251366004610d50565b6000908152600160209081526040808320548352600290915290205490565b34801561027c57600080fd5b50610179610615565b34801561029157600080fd5b50610179610629565b3480156102a657600080fd5b506102af610671565b6040516001600160a01b039091168152602001610150565b3480156102d357600080fd5b506101466102e2366004610d50565b60026020526000908152604090205481565b34801561030057600080fd5b50610325604051806040016040528060058152602001640352e302e360dc1b81525081565b6040516101509190610f47565b34801561033e57600080fd5b506101fb61034d366004610ec8565b6106a6565b34801561035e57600080fd5b506102af73deaddeaddeaddeaddeaddeaddeaddeaddeaddead81565b34801561038657600080fd5b506102af6106b5565b34801561039b57600080fd5b506101466103aa366004610d50565b60009081526001602052604090205490565b3480156103c857600080fd5b506101796103d7366004610f7a565b6106de565b3373deaddeaddeaddeaddeaddeaddeaddeaddeaddead146104445760405162461bcd60e51b815260206004820152601f60248201527f63616c6c6572206973206e6f74207468652073797374656d2063616c6c65720060448201526064015b60405180910390fd5b600080549081900361048a5760405162461bcd60e51b815260206004820152600f60248201526e139bdd081a5b9a5d1a585b1a5e9959608a1b604482015260640161043b565b60008181526001602081905260409091208490556104a9908290610f95565b6000908155838152600260209081526040808320859055915482519081529081018590529081018390527f32eff959e2e8d1609edc4b39ccf75900aa6c1da5719f8432752963fdf008234f9060600160405180910390a1505050565b3373deaddeaddeaddeaddeaddeaddeaddeaddeaddead146105685760405162461bcd60e51b815260206004820152601f60248201527f63616c6c6572206973206e6f74207468652073797374656d2063616c6c657200604482015260640161043b565b600054156105ae5760405162461bcd60e51b8152602060048201526013602482015272105b1c9958591e481a5b9a5d1a585b1a5e9959606a1b604482015260640161043b565b600055565b6105bb610763565b6105c482610808565b6105ce8282610810565b5050565b6000858152600160205260408120546105ee90868686866108d2565b9695505050505050565b6000610602610930565b5060008051602061100e83398151915290565b61061d610979565b61062760006109ab565b565b33806106336106b5565b6001600160a01b0316146106655760405163118cdaa760e01b81526001600160a01b038216600482015260240161043b565b61066e816109ab565b50565b6000807f9016d09d72d40fdae2fd8ceac6b6234c7706214fd39c1cd1e609a0528c1993005b546001600160a01b031692915050565b60006105ee86868686866108d2565b6000807f237e158222e3e6968b72b9db0d8043aacf074ad9f650f0d1606b4d82ee432c00610696565b6106e6610979565b7f237e158222e3e6968b72b9db0d8043aacf074ad9f650f0d1606b4d82ee432c0080546001600160a01b0319166001600160a01b038316908117825561072a610671565b6001600160a01b03167f38d16b8cac22d99fc7c124b9cd0de2d3fa1faef420bfe791d8c362d765e2270060405160405180910390a35050565b306001600160a01b037f00000000000000000000000000000000000000000000000000000000000000001614806107ea57507f00000000000000000000000000000000000000000000000000000000000000006001600160a01b03166107de60008051602061100e833981519152546001600160a01b031690565b6001600160a01b031614155b156106275760405163703e46dd60e11b815260040160405180910390fd5b61066e610979565b816001600160a01b03166352d1902d6040518163ffffffff1660e01b8152600401602060405180830381865afa92505050801561086a575060408051601f3d908101601f1916820190925261086791810190610fb6565b60015b61089257604051634c9c8ce360e01b81526001600160a01b038316600482015260240161043b565b60008051602061100e83398151915281146108c357604051632a87526960e21b81526004810182905260240161043b565b6108cd83836109e3565b505050565b6000858152600260209081526040808320548151601f8701849004840281018401909252858252916109259188918491908990899081908401838280828437600092019190915250899250610a39915050565b979650505050505050565b306001600160a01b037f000000000000000000000000000000000000000000000000000000000000000016146106275760405163703e46dd60e11b815260040160405180910390fd5b33610982610671565b6001600160a01b0316146106275760405163118cdaa760e01b815233600482015260240161043b565b7f237e158222e3e6968b72b9db0d8043aacf074ad9f650f0d1606b4d82ee432c0080546001600160a01b03191681556105ce82610a77565b6109ec82610ae8565b6040516001600160a01b038316907fbc7cd75a20ee27fd9adebab32041f755214dbc6bffa90cc0225b39da2e5c2d3b90600090a2805115610a31576108cd8282610b4d565b6105ce610bc5565b60008385148015610a48575081155b8015610a5357508251155b15610a6057506001610a6f565b610a6c85848685610be4565b90505b949350505050565b7f9016d09d72d40fdae2fd8ceac6b6234c7706214fd39c1cd1e609a0528c19930080546001600160a01b031981166001600160a01b03848116918217845560405192169182907f8be0079c531659141344cd1fd0a4f28419497f9722a3daafe3b4186f6b6457e090600090a3505050565b806001600160a01b03163b600003610b1e57604051634c9c8ce360e01b81526001600160a01b038216600482015260240161043b565b60008051602061100e83398151915280546001600160a01b0319166001600160a01b0392909216919091179055565b6060600080846001600160a01b031684604051610b6a9190610fcf565b600060405180830381855af49150503d8060008114610ba5576040519150601f19603f3d011682016040523d82523d6000602084013e610baa565b606091505b5091509150610bba858383610c8e565b925050505b92915050565b34156106275760405163b398979f60e01b815260040160405180910390fd5b600060208451610bf49190610feb565b15610c0157506000610a6f565b8351600003610c1257506000610a6f565b818560005b8651811015610c8157610c2b600284610feb565b600103610c4f57610c48610c428883016020015190565b83610ced565b9150610c68565b610c6582610c608984016020015190565b610ced565b91505b60019290921c91610c7a602082610f95565b9050610c17565b5090931495945050505050565b606082610ca357610c9e82610cf9565b610ce6565b8151158015610cba57506001600160a01b0384163b155b15610ce357604051639996b31560e01b81526001600160a01b038516600482015260240161043b565b50805b9392505050565b6000610ce68383610d22565b805115610d095780518082602001fd5b604051630a12f52160e11b815260040160405180910390fd5b60008260005281602052602060006040600060025afa50602060006020600060025afa505060005192915050565b600060208284031215610d6257600080fd5b5035919050565b60008060408385031215610d7c57600080fd5b50508035926020909101359150565b80356001600160a01b0381168114610da257600080fd5b919050565b634e487b7160e01b600052604160045260246000fd5b60008060408385031215610dd057600080fd5b610dd983610d8b565b9150602083013567ffffffffffffffff80821115610df657600080fd5b818501915085601f830112610e0a57600080fd5b813581811115610e1c57610e1c610da7565b604051601f8201601f19908116603f01168101908382118183101715610e4457610e44610da7565b81604052828152886020848701011115610e5d57600080fd5b8260208601602083013760006020848301015280955050505050509250929050565b60008083601f840112610e9157600080fd5b50813567ffffffffffffffff811115610ea957600080fd5b602083019150836020828501011115610ec157600080fd5b9250929050565b600080600080600060808688031215610ee057600080fd5b8535945060208601359350604086013567ffffffffffffffff811115610f0557600080fd5b610f1188828901610e7f565b96999598509660600135949350505050565b60005b83811015610f3e578181015183820152602001610f26565b50506000910152565b6020815260008251806020840152610f66816040850160208701610f23565b601f01601f19169190910160400192915050565b600060208284031215610f8c57600080fd5b610ce682610d8b565b80820180821115610bbf57634e487b7160e01b600052601160045260246000fd5b600060208284031215610fc857600080fd5b5051919050565b60008251610fe1818460208701610f23565b9190910192915050565b60008261100857634e487b7160e01b600052601260045260246000fd5b50069056fe360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbca26469706673582212208a59ff38af63c8a0ca256bb007b725d98ab1c290599e8cdf87bcbf2a98add93164736f6c63430008190033")),
        0,
        HashMap::new()
    ));

    config.data.push(AccountData::new(
        Bridge::address(),
        U256::from_str("0x115EEC47F6CF7E35000000").unwrap(),
        Bytes::from_static(&hex!("6080604052600a600c565b005b60186014601a565b6050565b565b5f604b7f360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc546001600160a01b031690565b905090565b365f80375f80365f845af43d5f803e8080156069573d5ff35b3d5ffdfea26469706673582212201698835cd7a9e8303f44009c3f144a4dbbfa3ab8ec0bca6489bb06bb1bda401164736f6c63430008180033")),
        0,
        [
            (U256::from_be_slice(&hex!("360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc")), U256::from_be_slice(&hex!("3200000000000000000000000000000000000002"))),
            (U256::from_be_slice(&hex!("9016d09d72d40fdae2fd8ceac6b6234c7706214fd39c1cd1e609a0528c199300")), U256::from_be_slice(&hex!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"))),
        ].into_iter().collect(),
    ));

    config.data.push(AccountData::new(
        address!("3200000000000000000000000000000000000002"),
        U256::ZERO,
        Bytes::from_static(&hex!("6080604052600436106101665760003560e01c80638da5cb5b116100d1578063d1c444561161008a578063e30c397811610064578063e30c3978146103ff578063e613ae0014610414578063ec6925a71461042f578063f2fde38b1461044a57600080fd5b8063d1c4445614610397578063d761753e146103b7578063dd95c7c6146103df57600080fd5b80638da5cb5b146102e95780638e19899e146102fe5780639f963f5914610311578063ad3cb1cc14610331578063b3ab15fb14610362578063b93780f61461038257600080fd5b806359c19cee1161012357806359c19cee146102455780635e0e5b3e14610258578063715018a614610288578063781952a81461029d57806379ba5097146102b257806387f8bf56146102c757600080fd5b8063158ef93e1461016b578063412601371461019a57806343e31687146101bc5780634f1ef286146101e057806352d1902d146101f3578063570ca73514610208575b600080fd5b34801561017757600080fd5b506000546101859060ff1681565b60405190151581526020015b60405180910390f35b3480156101a657600080fd5b506101ba6101b53660046122e2565b61046a565b005b3480156101c857600080fd5b506101d260015481565b604051908152602001610191565b6101ba6101ee366004612388565b6106ae565b3480156101ff57600080fd5b506101d26106cd565b34801561021457600080fd5b5060005461022d9061010090046001600160a01b031681565b6040516001600160a01b039091168152602001610191565b6101ba61025336600461244a565b6106ea565b34801561026457600080fd5b506101856102733660046124bf565b60046020526000908152604090205460ff1681565b34801561029457600080fd5b506101ba6107fa565b3480156102a957600080fd5b506005546101d2565b3480156102be57600080fd5b506101ba61080e565b3480156102d357600080fd5b506102dc610856565b6040516101919190612528565b3480156102f557600080fd5b5061022d6108e4565b6101ba61030c3660046124bf565b610919565b34801561031d57600080fd5b506101ba61032c3660046122e2565b6109e2565b34801561033d57600080fd5b506102dc604051806040016040528060058152602001640352e302e360dc1b81525081565b34801561036e57600080fd5b506101ba61037d36600461253b565b610ae4565b34801561038e57600080fd5b506102dc610b55565b3480156103a357600080fd5b506101d26103b23660046124bf565b610b62565b3480156103c357600080fd5b5061022d73deaddeaddeaddeaddeaddeaddeaddeaddeaddead81565b3480156103eb57600080fd5b506101ba6103fa366004612556565b610b83565b34801561040b57600080fd5b5061022d61132d565b34801561042057600080fd5b5061022d6001603160981b0181565b34801561043b57600080fd5b506101d2662386f26fc1000081565b34801561045657600080fd5b506101ba61046536600461253b565b611356565b3373deaddeaddeaddeaddeaddeaddeaddeaddeaddead146104d25760405162461bcd60e51b815260206004820152601f60248201527f63616c6c6572206973206e6f74207468652073797374656d2063616c6c65720060448201526064015b60405180910390fd5b60005460ff16156105255760405162461bcd60e51b815260206004820152601f60248201527f436f6e747261637420697320616c726561647920696e697469616c697a65640060448201526064016104c9565b806000036105755760405162461bcd60e51b815260206004820152601a60248201527f566572696669657220636f756e742063616e6e6f74206265203000000000000060448201526064016104c9565b60008490036105c65760405162461bcd60e51b815260206004820152601e60248201527f4465706f736974207363726970742063616e6e6f7420626520656d707479000060448201526064016104c9565b6000805460ff1916600117905560026105e085878361261c565b5060036105ee83858361261c565b50600181905560008054610100600160a81b03191674deaddeaddeaddeaddeaddeaddeaddeaddeaddead001781556040805191825273deaddeaddeaddeaddeaddeaddeaddeaddeaddead60208301527ffbe5b6cbafb274f445d7fed869dc77a838d8243a22c460de156560e8857cad03910160405180910390a17f89ed79f38bee253aee2fb8d52df0d71b4aaf0843800d093a499a55eeca455c34858585858560405161069f959493929190612706565b60405180910390a15050505050565b6106b66113db565b6106bf82611480565b6106c98282611488565b5050565b60006106d761154a565b5060008051602061298b83398151915290565b6106fb81662386f26fc10000612756565b34146107435760405162461bcd60e51b8152602060048201526017602482015276125b9d985b1a59081dda5d1a191c985dc8185b5bdd5b9d604a1b60448201526064016104c9565b60055460005b828110156107f45760058484838181106107655761076561276d565b835460018101855560009485526020948590209190940292909201359190920155507fc96d1af655ee5eb07357bb1097f3b2f247ea0c4e3cf5f9a5c8449c4f8b64fb6b8484838181106107ba576107ba61276d565b9050602002013582846107cd9190612783565b604080519283526020830191909152429082015260600160405180910390a1600101610749565b50505050565b610802611593565b61080c60006115c5565b565b338061081861132d565b6001600160a01b03161461084a5760405163118cdaa760e01b81526001600160a01b03821660048201526024016104c9565b610853816115c5565b50565b6003805461086390612592565b80601f016020809104026020016040519081016040528092919081815260200182805461088f90612592565b80156108dc5780601f106108b1576101008083540402835291602001916108dc565b820191906000526020600020905b8154815290600101906020018083116108bf57829003601f168201915b505050505081565b6000807f9016d09d72d40fdae2fd8ceac6b6234c7706214fd39c1cd1e609a0528c1993005b546001600160a01b031692915050565b662386f26fc1000034146109695760405162461bcd60e51b8152602060048201526017602482015276125b9d985b1a59081dda5d1a191c985dc8185b5bdd5b9d604a1b60448201526064016104c9565b600580546001810182556000919091527f036b6384b5eca791c62761152d0c79bb0604c104a5fb6f4eb0703f3154bb3db081018290556040805183815260208101839052428183015290517fc96d1af655ee5eb07357bb1097f3b2f247ea0c4e3cf5f9a5c8449c4f8b64fb6b9181900360600190a15050565b6109ea611593565b80600003610a3a5760405162461bcd60e51b815260206004820152601a60248201527f566572696669657220636f756e742063616e6e6f74206265203000000000000060448201526064016104c9565b6000849003610a8b5760405162461bcd60e51b815260206004820152601e60248201527f4465706f736974207363726970742063616e6e6f7420626520656d707479000060448201526064016104c9565b6002610a9885878361261c565b506003610aa683858361261c565b5060018190556040517f89ed79f38bee253aee2fb8d52df0d71b4aaf0843800d093a499a55eeca455c349061069f9087908790879087908790612706565b610aec611593565b60008054610100600160a81b0319166101006001600160a01b038481168281029390931793849055604080519290940416815260208101919091527ffbe5b6cbafb274f445d7fed869dc77a838d8243a22c460de156560e8857cad03910160405180910390a150565b6002805461086390612592565b60058181548110610b7257600080fd5b600091825260209091200154905081565b60005461010090046001600160a01b03163314610be25760405162461bcd60e51b815260206004820152601a60248201527f63616c6c6572206973206e6f7420746865206f70657261746f7200000000000060448201526064016104c9565b6000610c40610bf46020840184612796565b610c0460408501602086016127c0565b610c1160408601866127ea565b610c1e60608801886127ea565b610c2b60808a018a6127ea565b610c3b60c08c0160a08d01612796565b6115fd565b60008181526004602052604090205490915060ff1615610c985760405162461bcd60e51b81526020600482015260136024820152721ddd1e125908185b1c9958591e481cdc195b9d606a1b60448201526064016104c9565b60008181526004602052604090819020805460ff19166001179055610cfd90610cc3908401846127ea565b8080601f01602080910402602001604051908101604052809392919081815260200183838082843760009201919091525061164592505050565b610d495760405162461bcd60e51b815260206004820152601d60248201527f56696e206973206e6f742070726f7065726c7920666f726d617474656400000060448201526064016104c9565b610d93610d5960608401846127ea565b8080601f0160208091040260200160405190810160405280939291908181526020018383808284376000920191909152506116e992505050565b610ddf5760405162461bcd60e51b815260206004820152601e60248201527f566f7574206973206e6f742070726f7065726c7920666f726d6174746564000060448201526064016104c9565b6000610e2b610df160408501856127ea565b8080601f01602080910402602001604051908101604052809392919081815260200183838082843760009201919091525061178092505050565b91505080600114610e775760405162461bcd60e51b815260206004820152601660248201527513db9b1e481bdb99481a5b9c1d5d08185b1b1bddd95960521b60448201526064016104c9565b610ec3610e8760808501856127ea565b8080601f016020809104026020016040519081016040528093929190818152602001838380828437600092019190915250859250611797915050565b610f195760405162461bcd60e51b815260206004820152602160248201527f5769746e657373206973206e6f742070726f7065726c7920666f726d617474656044820152601960fa1b60648201526084016104c9565b6001603160981b01634ffd344a60e085013584610f3960c08801886127ea565b8861010001356040518663ffffffff1660e01b8152600401610f5f959493929190612831565b602060405180830381865afa158015610f7c573d6000803e3d6000fd5b505050506040513d601f19601f82011682018060405250810190610fa09190612863565b610fec5760405162461bcd60e51b815260206004820152601b60248201527f5472616e73616374696f6e206973206e6f7420696e20626c6f636b000000000060448201526064016104c9565b6000611038610ffe60808601866127ea565b8080601f0160208091040260200160405190810160405280939291908181526020018383808284376000920182905250925061180d915050565b9050600061104582611780565b91505060015460026110579190612783565b811461109d5760405162461bcd60e51b8152602060048201526015602482015274496e76616c6964207769746e657373206974656d7360581b60448201526064016104c9565b60006110ab836001546118f0565b90506000600280546110bc90612592565b9150600090506110cd838284611aba565b905061116381600280546110e090612592565b80601f016020809104026020016040519081016040528092919081815260200182805461110c90612592565b80156111595780601f1061112e57610100808354040283529160200191611159565b820191906000526020600020905b81548152906001019060200180831161113c57829003601f168201915b5050505050611b7e565b6111a85760405162461bcd60e51b8152602060048201526016602482015275125b9d985b1a590819195c1bdcda5d081cd8dc9a5c1d60521b60448201526064016104c9565b60006111d76111b8846014612783565b6111c3856014612783565b86516111cf9190612885565b869190611aba565b90506111ea81600380546110e090612592565b61122e5760405162461bcd60e51b8152602060048201526015602482015274092dcecc2d8d2c840e6c6e4d2e0e840e6eaccccd2f605b1b60448201526064016104c9565b600061123985611c40565b604080518b81526001600160a01b0383166020820152428183015290519192507f182fa52899142d44ff5c45a6354d3b3e868d5b07db6a65580b39bd321bdaf8ac919081900360600190a16000816001600160a01b0316662386f26fc1000060405160006040518083038185875af1925050503d80600081146112d8576040519150601f19603f3d011682016040523d82523d6000602084013e6112dd565b606091505b50509050806113205760405162461bcd60e51b815260206004820152600f60248201526e151c985b9cd9995c8819985a5b1959608a1b60448201526064016104c9565b5050505050505050505050565b6000807f237e158222e3e6968b72b9db0d8043aacf074ad9f650f0d1606b4d82ee432c00610909565b61135e611593565b7f237e158222e3e6968b72b9db0d8043aacf074ad9f650f0d1606b4d82ee432c0080546001600160a01b0319166001600160a01b03831690811782556113a26108e4565b6001600160a01b03167f38d16b8cac22d99fc7c124b9cd0de2d3fa1faef420bfe791d8c362d765e2270060405160405180910390a35050565b306001600160a01b037f000000000000000000000000000000000000000000000000000000000000000016148061146257507f00000000000000000000000000000000000000000000000000000000000000006001600160a01b031661145660008051602061298b833981519152546001600160a01b031690565b6001600160a01b031614155b1561080c5760405163703e46dd60e11b815260040160405180910390fd5b610853611593565b816001600160a01b03166352d1902d6040518163ffffffff1660e01b8152600401602060405180830381865afa9250505080156114e2575060408051601f3d908101601f191682019092526114df91810190612898565b60015b61150a57604051634c9c8ce360e01b81526001600160a01b03831660048201526024016104c9565b60008051602061298b833981519152811461153b57604051632a87526960e21b8152600481018290526024016104c9565b6115458383611c76565b505050565b306001600160a01b037f0000000000000000000000000000000000000000000000000000000000000000161461080c5760405163703e46dd60e11b815260040160405180910390fd5b3361159c6108e4565b6001600160a01b03161461080c5760405163118cdaa760e01b81523360048201526024016104c9565b7f237e158222e3e6968b72b9db0d8043aacf074ad9f650f0d1606b4d82ee432c0080546001600160a01b03191681556106c982611ccc565b60006116378a8a8a8a8a8a8a8a8a604051602001611623999897969594939291906128b1565b604051602081830303815290604052611d3d565b9a9950505050505050505050565b600080600061165384611780565b9092509050801580611666575060001982145b15611675575060009392505050565b6000611682836001612783565b905060005b828110156116dc57855182106116a35750600095945050505050565b60006116af8784611d64565b905060001981036116c7575060009695505050505050565b6116d18184612783565b925050600101611687565b5093519093149392505050565b60008060006116f784611780565b909250905080158061170a575060001982145b15611719575060009392505050565b6000611726836001612783565b905060005b828110156116dc57855182106117475750600095945050505050565b60006117538784611dad565b9050600019810361176b575060009695505050505050565b6117758184612783565b92505060010161172b565b60008061178e836000611e11565b91509150915091565b6000816000036117a957506000611807565b6000805b8381101561180057845182106117c857600092505050611807565b60006117d48684611fb5565b905060001981036117eb5760009350505050611807565b6117f58184612783565b9250506001016117ad565b5083511490505b92915050565b606060008060005b84811015611886576118278683611fb5565b925060001983036118725760405162461bcd60e51b815260206004820152601560248201527442616420566172496e7420696e207769746e65737360581b60448201526064016104c9565b61187c8383612783565b9150600101611815565b506118918582611fb5565b915060001982036118dc5760405162461bcd60e51b815260206004820152601560248201527442616420566172496e7420696e207769746e65737360581b60448201526064016104c9565b6118e7858284611aba565b95945050505050565b60606000806118fe85611780565b90925090506001820161195e5760405162461bcd60e51b815260206004820152602260248201527f52656164206f76657272756e20647572696e6720566172496e742070617273696044820152616e6760f01b60648201526084016104c9565b8084106119a05760405162461bcd60e51b815260206004820152601060248201526f2b34b7103932b0b21037bb32b9393ab760811b60448201526064016104c9565b6000806119ae846001612783565b905060005b86811015611a39576119c58883611e11565b909550925060018301611a0f5760405162461bcd60e51b815260206004820152601260248201527142616420566172496e7420696e206974656d60701b60448201526064016104c9565b82611a1b866001612783565b611a259190612783565b611a2f9083612783565b91506001016119b3565b50611a448782611e11565b909450915060018201611a8e5760405162461bcd60e51b815260206004820152601260248201527142616420566172496e7420696e206974656d60701b60448201526064016104c9565b611aaf81611a9c8685612783565b611aa7906001612783565b899190611aba565b979650505050505050565b606081600003611ad95750604080516020810190915260008152611b77565b6000611ae58385612783565b90508381118015611af7575080855110155b611b395760405162461bcd60e51b8152602060048201526013602482015272536c696365206f7574206f6620626f756e647360681b60448201526064016104c9565b604051915082604083010160405282825283850182038460208701018481015b80821015611b7257815183830152602082019150611b59565b505050505b9392505050565b60008151835114611bc85760405162461bcd60e51b8152602060048201526014602482015273098cadccee8d0e640c8de40dcdee840dac2e8c6d60631b60448201526064016104c9565b825160005b81811015611c3557838181518110611be757611be761276d565b602001015160f81c60f81b6001600160f81b031916858281518110611c0e57611c0e61276d565b01602001516001600160f81b03191614611c2d57600092505050611807565b600101611bcd565b506001949350505050565b60008060028054611c5090612592565b915060009050611c6284836014611aba565b611c6b90612919565b60601c949350505050565b611c7f82612053565b6040516001600160a01b038316907fbc7cd75a20ee27fd9adebab32041f755214dbc6bffa90cc0225b39da2e5c2d3b90600090a2805115611cc45761154582826120b8565b6106c9612125565b7f9016d09d72d40fdae2fd8ceac6b6234c7706214fd39c1cd1e609a0528c19930080546001600160a01b031981166001600160a01b03848116918217845560405192169182907f8be0079c531659141344cd1fd0a4f28419497f9722a3daafe3b4186f6b6457e090600090a3505050565b60006020600083516020850160025afa50602060006020600060025afa5050600051919050565b6000806000611d738585612144565b909250905060018201611d8c5760001992505050611807565b80611d98836025612783565b611da29190612783565b6118e7906004612783565b6000611dba826009612783565b83511015611dcb5750600019611807565b600080611de285611ddd866008612783565b611e11565b909250905060018201611dfb5760001992505050611807565b80611e07836009612783565b6118e79190612783565b6000806000611e208585612186565b90508060ff16600003611e55576000858581518110611e4157611e4161276d565b016020015190935060f81c9150611fae9050565b83611e61826001612955565b60ff16611e6e9190612783565b85511015611e855760001960009250925050611fae565b60008160ff16600203611ec957611ebe611eaa611ea3876001612783565b889061220c565b62ffff0060e882901c1660f89190911c1790565b61ffff169050611fa4565b8160ff16600403611f1857611f0b611ee5611ea3876001612783565b60d881901c63ff00ff001662ff00ff60e89290921c9190911617601081811b91901c1790565b63ffffffff169050611fa4565b8160ff16600803611fa457611f97611f34611ea3876001612783565b60c01c64ff000000ff600882811c91821665ff000000ff009390911b92831617601090811b67ffffffffffffffff1666ff00ff00ff00ff9290921667ff00ff00ff00ff009093169290921790911c65ffff0000ffff1617602081811c91901b1790565b67ffffffffffffffff1690505b60ff909116925090505b9250929050565b6000806000611fc48585611e11565b909250905060018201611fdd5760001992505050611807565b600080611feb846001612783565b905060005b83811015612048576120028883611e11565b90955092506001830161201e5760001995505050505050611807565b8261202a866001612783565b6120349190612783565b61203e9083612783565b9150600101611ff0565b509695505050505050565b806001600160a01b03163b60000361208957604051634c9c8ce360e01b81526001600160a01b03821660048201526024016104c9565b60008051602061298b83398151915280546001600160a01b0319166001600160a01b0392909216919091179055565b6060600080846001600160a01b0316846040516120d5919061296e565b600060405180830381855af49150503d8060008114612110576040519150601f19603f3d011682016040523d82523d6000602084013e612115565b606091505b50915091506118e785838361221b565b341561080c5760405163b398979f60e01b815260040160405180910390fd5b600080612152836025612783565b84511015612167575060001990506000611fae565b60008061217986611ddd876024612783565b9097909650945050505050565b600082828151811061219a5761219a61276d565b016020015160f81c60ff036121b157506008611807565b8282815181106121c3576121c361276d565b016020015160f81c60fe036121da57506004611807565b8282815181106121ec576121ec61276d565b016020015160f81c60fd0361220357506002611807565b50600092915050565b6000611b778383016020015190565b6060826122305761222b82612277565b611b77565b815115801561224757506001600160a01b0384163b155b1561227057604051639996b31560e01b81526001600160a01b03851660048201526024016104c9565b5080611b77565b8051156122875780518082602001fd5b604051630a12f52160e11b815260040160405180910390fd5b60008083601f8401126122b257600080fd5b50813567ffffffffffffffff8111156122ca57600080fd5b602083019150836020828501011115611fae57600080fd5b6000806000806000606086880312156122fa57600080fd5b853567ffffffffffffffff8082111561231257600080fd5b61231e89838a016122a0565b9097509550602088013591508082111561233757600080fd5b50612344888289016122a0565b96999598509660400135949350505050565b80356001600160a01b038116811461236d57600080fd5b919050565b634e487b7160e01b600052604160045260246000fd5b6000806040838503121561239b57600080fd5b6123a483612356565b9150602083013567ffffffffffffffff808211156123c157600080fd5b818501915085601f8301126123d557600080fd5b8135818111156123e7576123e7612372565b604051601f8201601f19908116603f0116810190838211818310171561240f5761240f612372565b8160405282815288602084870101111561242857600080fd5b8260208601602083013760006020848301015280955050505050509250929050565b6000806020838503121561245d57600080fd5b823567ffffffffffffffff8082111561247557600080fd5b818501915085601f83011261248957600080fd5b81358181111561249857600080fd5b8660208260051b85010111156124ad57600080fd5b60209290920196919550909350505050565b6000602082840312156124d157600080fd5b5035919050565b60005b838110156124f35781810151838201526020016124db565b50506000910152565b600081518084526125148160208601602086016124d8565b601f01601f19169290920160200192915050565b602081526000611b7760208301846124fc565b60006020828403121561254d57600080fd5b611b7782612356565b60006020828403121561256857600080fd5b813567ffffffffffffffff81111561257f57600080fd5b82016101208185031215611b7757600080fd5b600181811c908216806125a657607f821691505b6020821081036125c657634e487b7160e01b600052602260045260246000fd5b50919050565b601f821115611545576000816000526020600020601f850160051c810160208610156125f55750805b601f850160051c820191505b8181101561261457828155600101612601565b505050505050565b67ffffffffffffffff83111561263457612634612372565b612648836126428354612592565b836125cc565b6000601f84116001811461267c57600085156126645750838201355b600019600387901b1c1916600186901b1783556126d6565b600083815260209020601f19861690835b828110156126ad578685013582556020948501946001909201910161268d565b50868210156126ca5760001960f88860031b161c19848701351681555b505060018560011b0183555b5050505050565b81835281816020850137506000828201602090810191909152601f909101601f19169091010190565b60608152600061271a6060830187896126dd565b828103602084015261272d8186886126dd565b9150508260408301529695505050505050565b634e487b7160e01b600052601160045260246000fd5b808202811582820484141761180757611807612740565b634e487b7160e01b600052603260045260246000fd5b8082018082111561180757611807612740565b6000602082840312156127a857600080fd5b81356001600160e01b031981168114611b7757600080fd5b6000602082840312156127d257600080fd5b81356001600160f01b031981168114611b7757600080fd5b6000808335601e1984360301811261280157600080fd5b83018035915067ffffffffffffffff82111561281c57600080fd5b602001915036819003821315611fae57600080fd5b8581528460208201526080604082015260006128516080830185876126dd565b90508260608301529695505050505050565b60006020828403121561287557600080fd5b81518015158114611b7757600080fd5b8181038181111561180757611807612740565b6000602082840312156128aa57600080fd5b5051919050565b6001600160e01b03198a811682526001600160f01b03198a166004830152600090888a60068501378883016006810160008152888a823750878101905060068101600081528688823750931692909301600681019290925250600a0198975050505050505050565b805160208201516bffffffffffffffffffffffff19808216929190601483101561294d5780818460140360031b1b83161693505b505050919050565b60ff818116838216019081111561180757611807612740565b600082516129808184602087016124d8565b919091019291505056fe360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbca2646970667358221220182f4d2f08ca4c4ec067af41dedeedc7f5d8b29e5b5a7e0810b80d2efbad778064736f6c63430008190033")),
        0,
        HashMap::new()
    ));
}
