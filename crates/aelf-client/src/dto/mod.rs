use aelf_proto::aelf::{ResourceTokenCharged, TransactionFeeCharged};
use base64::Engine;
use prost::Message;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Chain status returned by the blockchain status endpoint.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ChainStatusDto {
    pub chain_id: String,
    #[serde(default)]
    pub branches: HashMap<String, i64>,
    #[serde(default)]
    pub not_linked_blocks: HashMap<String, i64>,
    pub longest_chain_height: i64,
    pub longest_chain_hash: String,
    pub genesis_block_hash: String,
    pub genesis_contract_address: String,
    pub last_irreversible_block_hash: String,
    pub last_irreversible_block_height: i64,
    pub best_chain_hash: String,
    pub best_chain_height: i64,
}

/// Block payload returned by block lookup endpoints.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BlockDto {
    pub block_hash: String,
    pub header: BlockHeaderDto,
    pub body: BlockBodyDto,
}

/// Block header subset returned by REST block queries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BlockHeaderDto {
    pub height: i64,
    #[serde(default)]
    pub previous_block_hash: String,
}

/// Block body subset returned by REST block queries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BlockBodyDto {
    #[serde(default)]
    pub transactions: Vec<String>,
}

/// Transaction pool counters returned by pool status queries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TransactionPoolStatusOutput {
    #[serde(default)]
    pub queued: i64,
    #[serde(default)]
    pub validated: i64,
}

/// Input payload for node-side raw transaction generation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateRawTransactionInput {
    pub from: String,
    pub to: String,
    pub ref_block_number: i64,
    pub ref_block_hash: String,
    pub method_name: String,
    pub params: String,
}

/// Output payload returned by node-side raw transaction generation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateRawTransactionOutput {
    pub raw_transaction: String,
}

/// Signed raw transaction execution request.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ExecuteRawTransactionDto {
    pub raw_transaction: String,
    pub signature: String,
}

/// Input payload for broadcasting a raw transaction plus signature.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendRawTransactionInput {
    pub transaction: String,
    pub signature: String,
    pub return_transaction: bool,
}

/// Output payload returned by `sendRawTransaction`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendRawTransactionOutput {
    #[serde(alias = "TransactionID")]
    pub transaction_id: String,
    #[serde(default)]
    pub transaction: TransactionDto,
}

/// Input payload for broadcasting a fully signed raw transaction.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendTransactionInput {
    pub raw_transaction: String,
}

/// Output payload returned by `sendTransaction`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendTransactionOutput {
    #[serde(alias = "TransactionID")]
    pub transaction_id: String,
}

/// Input payload for batching multiple signed raw transactions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SendTransactionsInput {
    pub raw_transactions: String,
}

/// REST-serialized transaction representation returned by some node endpoints.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TransactionDto {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub ref_block_number: i64,
    #[serde(default)]
    pub ref_block_prefix: String,
    #[serde(default)]
    pub method_name: String,
    #[serde(default)]
    pub params: String,
    #[serde(default)]
    pub signature: String,
}

/// Log event emitted by a transaction execution result.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LogEventDto {
    pub address: String,
    pub name: String,
    #[serde(default, deserialize_with = "null_vec_as_default")]
    pub indexed: Vec<String>,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub non_indexed: String,
}

/// Transaction execution result returned by transaction result queries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TransactionResultDto {
    #[serde(alias = "TransactionID")]
    pub transaction_id: String,
    pub status: String,
    #[serde(default, deserialize_with = "null_vec_as_default")]
    pub logs: Vec<LogEventDto>,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub bloom: String,
    #[serde(default, deserialize_with = "null_json_as_default")]
    pub transaction: serde_json::Value,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub return_value: String,
    #[serde(default, deserialize_with = "null_i64_as_default")]
    pub block_number: i64,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub block_hash: String,
    #[serde(default, deserialize_with = "null_string_as_default")]
    pub error: String,
}

impl TransactionResultDto {
    /// Extracts ELF and resource token fees from protobuf-encoded execution logs.
    pub fn get_transaction_fees(&self) -> HashMap<String, i64> {
        let engine = base64::engine::general_purpose::STANDARD;
        let mut result = HashMap::new();

        for log in &self.logs {
            match log.name.as_str() {
                "TransactionFeeCharged" => {
                    let Ok(bytes) = engine.decode(&log.non_indexed) else {
                        continue;
                    };
                    let Ok(event) = TransactionFeeCharged::decode(bytes.as_slice()) else {
                        continue;
                    };
                    result.insert(event.symbol, event.amount);
                }
                "ResourceTokenCharged" => {
                    let Ok(bytes) = engine.decode(&log.non_indexed) else {
                        continue;
                    };
                    let Ok(event) = ResourceTokenCharged::decode(bytes.as_slice()) else {
                        continue;
                    };
                    result.insert(event.symbol, event.amount);
                }
                _ => {}
            }
        }

        result
    }
}

fn null_string_as_default<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}

fn null_vec_as_default<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let value = Option::<Vec<T>>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}

fn null_json_as_default<'de, D>(deserializer: D) -> Result<serde_json::Value, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.unwrap_or(serde_json::Value::Null))
}

fn null_i64_as_default<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<i64>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}

/// Merkle path node returned by transaction proof queries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MerklePathNodeDto {
    pub hash: String,
    pub is_left_child_node: bool,
}

/// Merkle path returned by transaction proof queries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MerklePathDto {
    #[serde(default)]
    pub merkle_path_nodes: Vec<MerklePathNodeDto>,
}

/// Peer description returned by the network peers endpoint.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PeerDto {
    #[serde(default)]
    pub ip_address: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Network metadata returned by the network info endpoint.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NetworkInfoOutput {
    #[serde(default)]
    pub version: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Task queue snapshot returned by the task queue endpoint.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TaskQueueInfoDto {
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Input payload for the transaction fee estimation endpoint.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CalculateTransactionFeeInput {
    pub raw_transaction: String,
}

/// Output payload returned by the transaction fee estimation endpoint.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CalculateTransactionFeeOutput {
    pub success: bool,
    #[serde(default)]
    pub transaction_fee: HashMap<String, f64>,
}

/// Standard WebApp error envelope returned by AElf node REST APIs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WebAppErrorResponse {
    pub error: WebAppError,
}

/// Error payload nested inside the WebApp error envelope.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WebAppError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{LogEventDto, TransactionResultDto};
    use aelf_proto::aelf::{Address, ResourceTokenCharged, TransactionFeeCharged};
    use base64::Engine;
    use prost::Message;

    #[test]
    fn parses_transaction_fee_logs() {
        let engine = base64::engine::general_purpose::STANDARD;
        let tx_fee = TransactionFeeCharged {
            symbol: "ELF".to_owned(),
            amount: 12_345,
        };
        let resource_fee = ResourceTokenCharged {
            symbol: "CPU".to_owned(),
            amount: 999,
            contract_address: Some(Address {
                value: vec![1_u8; 32],
            }),
        };
        let result = TransactionResultDto {
            transaction_id: "0x01".to_owned(),
            status: "MINED".to_owned(),
            logs: vec![
                LogEventDto {
                    address: "addr".to_owned(),
                    name: "TransactionFeeCharged".to_owned(),
                    indexed: Vec::new(),
                    non_indexed: engine.encode(tx_fee.encode_to_vec()),
                },
                LogEventDto {
                    address: "addr".to_owned(),
                    name: "ResourceTokenCharged".to_owned(),
                    indexed: Vec::new(),
                    non_indexed: engine.encode(resource_fee.encode_to_vec()),
                },
            ],
            bloom: String::new(),
            transaction: serde_json::Value::Null,
            return_value: String::new(),
            block_number: 1,
            block_hash: "0x02".to_owned(),
            error: String::new(),
        };

        let fees = result.get_transaction_fees();
        assert_eq!(fees.get("ELF"), Some(&12_345));
        assert_eq!(fees.get("CPU"), Some(&999));
    }
}
