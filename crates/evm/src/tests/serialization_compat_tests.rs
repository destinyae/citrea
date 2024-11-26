use alloy_consensus::serde_bincode_compat as alloy_serde_bincode_compat;
use reth_primitives::transaction::serde_bincode_compat as reth_tx_serde_bincode_compat;
use reth_primitives::{SealedHeader, TransactionSigned};
use reth_primitives_traits::serde_bincode_compat as reth_serde_bincode_compat;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

#[test]
fn test_sealed_header_serde_compat() {
    #[serde_as]
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Data {
        #[serde_as(as = "reth_serde_bincode_compat::SealedHeader")]
        pub b: SealedHeader,
    }
    let data = Data {
        b: SealedHeader::default(),
    };
    let encoded_data = bcs::to_bytes(&data).unwrap();
    let decoded_data: Data = bcs::from_bytes(&encoded_data).unwrap();

    assert_eq!(decoded_data, data);
}

#[test]
fn test_alloy_header_serde_compat() {
    #[serde_as]
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Data {
        #[serde_as(as = "alloy_serde_bincode_compat::Header")]
        pub b: alloy_consensus::Header,
    }
    let data = Data {
        b: alloy_consensus::Header::default(),
    };
    let encoded_data = bcs::to_bytes(&data).unwrap();
    let decoded_data: Data = bcs::from_bytes(&encoded_data).unwrap();

    assert_eq!(decoded_data, data);
}

#[test]
fn test_transaction_signed_serde_compat() {
    #[serde_as]
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Data {
        #[serde_as(as = "reth_tx_serde_bincode_compat::TransactionSigned")]
        pub b: TransactionSigned,
    }
    let data = Data {
        b: TransactionSigned::default(),
    };
    let encoded_data = bcs::to_bytes(&data).unwrap();
    let decoded_data: Data = bcs::from_bytes(&encoded_data).unwrap();

    assert_eq!(decoded_data, data);
}
