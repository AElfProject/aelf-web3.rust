//! Typed and dynamic contract bindings for the AElf Rust SDK.

#![forbid(unsafe_code)]

use aelf_client::dto::SendTransactionOutput;
use aelf_client::protobuf::RawBytesMessage;
use aelf_client::{AElfClient, AElfError};
use aelf_crypto::{address_to_pb, hash_to_pb, pb_to_address, Wallet};
use aelf_proto::{aedpos, cross_chain, election, token, vote};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, Kind, MessageDescriptor, MethodDescriptor};
use serde::de::IntoDeserializer;
use serde_json::{Map, Value};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::OnceCell;

/// Errors returned by typed and dynamic contract operations.
#[derive(Debug, Error)]
pub enum ContractError {
    #[error("client error: {0}")]
    Client(Box<AElfError>),
    #[error("method not found: {0}")]
    MethodNotFound(String),
    #[error("descriptor error: {0}")]
    Descriptor(#[from] prost_reflect::DescriptorError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("crypto error: {0}")]
    Crypto(#[from] aelf_crypto::CryptoError),
    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("protobuf decode error: {0}")]
    ProtobufDecode(#[from] prost::DecodeError),
}

impl From<AElfError> for ContractError {
    fn from(value: AElfError) -> Self {
        Self::Client(Box::new(value))
    }
}

/// Contract handle backed by a descriptor pool fetched from chain.
#[derive(Clone)]
pub struct DynamicContract {
    client: AElfClient,
    address: String,
    wallet: Wallet,
    pool: DescriptorPool,
}

impl DynamicContract {
    /// Loads a contract descriptor from chain and returns a dynamic contract handle.
    pub async fn at(
        client: AElfClient,
        address: impl Into<String>,
        wallet: Wallet,
    ) -> Result<Self, ContractError> {
        let address = address.into();
        let bytes = client
            .chain()
            .get_contract_file_descriptor_set(&address)
            .await?;
        let pool = DescriptorPool::decode(bytes.as_slice())?;

        Ok(Self {
            client,
            address,
            wallet,
            pool,
        })
    }

    /// Returns the contract address associated with this handle.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Resolves a method descriptor by name.
    pub fn method(&self, name: &str) -> Result<DynamicContractMethod, ContractError> {
        let mut found = None;
        for service in self.pool.services() {
            if let Some(method) = service.methods().find(|method| method.name() == name) {
                found = Some(method);
                break;
            }
        }

        let method = found.ok_or_else(|| ContractError::MethodNotFound(name.to_owned()))?;
        Ok(DynamicContractMethod {
            client: self.client.clone(),
            address: self.address.clone(),
            wallet: self.wallet.clone(),
            method,
        })
    }

    /// Calls a contract method with protobuf input and decodes the protobuf output.
    pub async fn call_typed<MIn, MOut>(
        &self,
        method_name: &str,
        input: &MIn,
    ) -> Result<MOut, ContractError>
    where
        MIn: Message,
        MOut: Message + Default,
    {
        let bytes = self
            .method(method_name)?
            .call_bytes(&input.encode_to_vec())
            .await?;
        Ok(MOut::decode(bytes.as_slice())?)
    }

    /// Sends a transaction with protobuf input and returns the broadcast response.
    pub async fn send_typed<MIn>(
        &self,
        method_name: &str,
        input: &MIn,
    ) -> Result<SendTransactionOutput, ContractError>
    where
        MIn: Message,
    {
        self.method(method_name)?
            .send_bytes(&input.encode_to_vec())
            .await
    }

    /// Calls a contract method using protobuf JSON input and returns protobuf JSON output.
    pub async fn call_json(&self, method_name: &str, input: Value) -> Result<Value, ContractError> {
        self.method(method_name)?.call_json(input).await
    }

    /// Sends a transaction using protobuf JSON input.
    pub async fn send_json(
        &self,
        method_name: &str,
        input: Value,
    ) -> Result<SendTransactionOutput, ContractError> {
        self.method(method_name)?.send_json(input).await
    }
}

/// Bound dynamic contract method descriptor.
#[derive(Clone)]
pub struct DynamicContractMethod {
    client: AElfClient,
    address: String,
    wallet: Wallet,
    method: MethodDescriptor,
}

impl DynamicContractMethod {
    /// Returns the underlying reflected method descriptor.
    pub fn descriptor(&self) -> &MethodDescriptor {
        &self.method
    }

    /// Calls a method with already encoded protobuf bytes.
    pub async fn call_bytes(&self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        let raw = build_signed_raw(
            &self.client,
            &self.wallet,
            &self.address,
            self.method.name(),
            input,
        )
        .await?;
        let response = self.client.tx().execute_transaction(&raw).await?;
        Ok(hex::decode(response.trim_matches('"'))?)
    }

    /// Sends a transaction with already encoded protobuf bytes.
    pub async fn send_bytes(&self, input: &[u8]) -> Result<SendTransactionOutput, ContractError> {
        let raw = build_signed_raw(
            &self.client,
            &self.wallet,
            &self.address,
            self.method.name(),
            input,
        )
        .await?;
        self.client
            .tx()
            .send_transaction(&raw)
            .await
            .map_err(Into::into)
    }

    /// Calls a method with protobuf JSON input and returns protobuf JSON output.
    pub async fn call_json(&self, input: Value) -> Result<Value, ContractError> {
        let input = normalize_json_input(&self.method.input(), input)?;
        let message = DynamicMessage::deserialize(self.method.input(), input.into_deserializer())?;
        let output = self.call_bytes(&message.encode_to_vec()).await?;
        let output = DynamicMessage::decode(self.method.output(), output.as_slice())?;
        let output = serde_json::to_value(output)?;
        normalize_json_output(&self.method.output(), output)
    }

    /// Sends a transaction with protobuf JSON input.
    pub async fn send_json(&self, input: Value) -> Result<SendTransactionOutput, ContractError> {
        let input = normalize_json_input(&self.method.input(), input)?;
        let message = DynamicMessage::deserialize(self.method.input(), input.into_deserializer())?;
        self.send_bytes(&message.encode_to_vec()).await
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsonDirection {
    Input,
    Output,
}

fn normalize_json_input(desc: &MessageDescriptor, value: Value) -> Result<Value, ContractError> {
    normalize_json_message(desc, value, JsonDirection::Input)
}

fn normalize_json_output(desc: &MessageDescriptor, value: Value) -> Result<Value, ContractError> {
    normalize_json_message(desc, value, JsonDirection::Output)
}

fn normalize_json_message(
    desc: &MessageDescriptor,
    value: Value,
    direction: JsonDirection,
) -> Result<Value, ContractError> {
    match (desc.full_name(), direction) {
        ("aelf.Address", JsonDirection::Input) => {
            return match value {
                Value::String(address) => Ok(serde_json::to_value(address_to_pb(&address)?)?),
                other => Ok(other),
            };
        }
        ("aelf.Address", JsonDirection::Output) => {
            return match value {
                Value::Object(_) => {
                    let address: aelf_proto::aelf::Address = serde_json::from_value(value)?;
                    Ok(Value::String(pb_to_address(&address)))
                }
                other => Ok(other),
            };
        }
        ("aelf.Hash", JsonDirection::Input) => {
            return match value {
                Value::String(hash) => {
                    let bytes = hex::decode(hash)?;
                    Ok(serde_json::to_value(hash_to_pb(bytes))?)
                }
                other => Ok(other),
            };
        }
        ("aelf.Hash", JsonDirection::Output) => {
            return match value {
                Value::Object(_) => {
                    let hash: aelf_proto::aelf::Hash = serde_json::from_value(value)?;
                    Ok(Value::String(hex::encode(hash.value)))
                }
                other => Ok(other),
            };
        }
        _ => {}
    }

    let Value::Object(object) = value else {
        return Ok(value);
    };

    let mut normalized = Map::with_capacity(object.len());
    for (key, value) in object {
        let field = desc
            .get_field_by_json_name(&key)
            .or_else(|| desc.get_field_by_name(&key));
        let value = match field {
            Some(field) => normalize_json_field(&field, value, direction)?,
            None => value,
        };
        normalized.insert(key, value);
    }

    Ok(Value::Object(normalized))
}
fn normalize_json_field(
    field: &prost_reflect::FieldDescriptor,
    value: Value,
    direction: JsonDirection,
) -> Result<Value, ContractError> {
    if field.is_list() {
        return match (field.kind(), value) {
            (Kind::Message(desc), Value::Array(items)) => Ok(Value::Array(
                items
                    .into_iter()
                    .map(|item| normalize_json_message(&desc, item, direction))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            (_, other) => Ok(other),
        };
    }

    if field.is_map() {
        return match (field.kind(), value) {
            (Kind::Message(entry), Value::Object(entries)) => {
                let value_field = entry.map_entry_value_field();
                match value_field.kind() {
                    Kind::Message(desc) => {
                        let mut normalized = Map::with_capacity(entries.len());
                        for (key, value) in entries {
                            normalized
                                .insert(key, normalize_json_message(&desc, value, direction)?);
                        }
                        Ok(Value::Object(normalized))
                    }
                    _ => Ok(Value::Object(entries)),
                }
            }
            (_, other) => Ok(other),
        };
    }

    match field.kind() {
        Kind::Message(desc) => normalize_json_message(&desc, value, direction),
        _ => Ok(value),
    }
}

async fn build_signed_raw(
    client: &AElfClient,
    wallet: &Wallet,
    address: &str,
    method_name: &str,
    params: &[u8],
) -> Result<String, ContractError> {
    let transaction = client
        .transaction_builder()
        .with_wallet(wallet.clone())
        .with_contract(address.to_owned())
        .with_method(method_name.to_owned())
        .with_message(&RawBytesMessage::new(params.to_vec()))
        .build_signed()
        .await?;
    Ok(hex::encode(transaction.encode_to_vec()))
}

#[derive(Clone)]
struct LazyDynamicContract {
    client: AElfClient,
    wallet: Wallet,
    address: String,
    dynamic: Arc<OnceCell<DynamicContract>>,
}

impl LazyDynamicContract {
    fn new(client: AElfClient, wallet: Wallet, address: impl Into<String>) -> Self {
        Self {
            client,
            wallet,
            address: address.into(),
            dynamic: Arc::new(OnceCell::new()),
        }
    }

    async fn get(&self) -> Result<DynamicContract, ContractError> {
        let contract = self
            .dynamic
            .get_or_try_init(|| async {
                DynamicContract::at(
                    self.client.clone(),
                    self.address.clone(),
                    self.wallet.clone(),
                )
                .await
            })
            .await?;
        Ok(contract.clone())
    }
}

/// Typed wrapper for the genesis zero contract.
#[derive(Clone)]
pub struct ZeroContract {
    client: AElfClient,
    wallet: Wallet,
    address: String,
}

impl ZeroContract {
    /// Creates a typed zero contract wrapper.
    pub fn new(client: AElfClient, wallet: Wallet, address: impl Into<String>) -> Self {
        Self {
            client,
            wallet,
            address: address.into(),
        }
    }

    /// Resolves a system contract address by its canonical contract name.
    pub async fn get_contract_address_by_name(
        &self,
        contract_name: &str,
    ) -> Result<String, ContractError> {
        let input = aelf_proto::aelf::Hash {
            value: aelf_crypto::sha256_bytes(contract_name.as_bytes()).to_vec(),
        };
        let raw = build_signed_raw(
            &self.client,
            &self.wallet,
            &self.address,
            "GetContractAddressByName",
            &input.encode_to_vec(),
        )
        .await?;
        let response = self.client.tx().execute_transaction(&raw).await?;
        let output =
            aelf_proto::aelf::Address::decode(hex::decode(response.trim_matches('"'))?.as_slice())?;
        Ok(pb_to_address(&output))
    }
}

/// Typed wrapper for the token contract.
#[derive(Clone)]
pub struct TokenContract {
    dynamic: LazyDynamicContract,
}

impl TokenContract {
    /// Creates a typed token contract wrapper.
    pub fn new(client: AElfClient, wallet: Wallet, address: impl Into<String>) -> Self {
        Self {
            dynamic: LazyDynamicContract::new(client, wallet, address),
        }
    }

    /// Returns the token balance for a given owner and symbol.
    pub async fn get_balance(
        &self,
        input: &token::GetBalanceInput,
    ) -> Result<token::GetBalanceOutput, ContractError> {
        self.dynamic().await?.call_typed("GetBalance", input).await
    }

    /// Returns token metadata for a specific symbol.
    pub async fn get_token_info(
        &self,
        input: &token::GetTokenInfoInput,
    ) -> Result<token::TokenInfo, ContractError> {
        self.dynamic()
            .await?
            .call_typed("GetTokenInfo", input)
            .await
    }

    /// Returns metadata for the chain's native token.
    pub async fn get_native_token_info(&self) -> Result<token::TokenInfo, ContractError> {
        self.dynamic()
            .await?
            .call_typed("GetNativeTokenInfo", &pbjson_types::Empty {})
            .await
    }

    /// Returns the primary token symbol used by the chain.
    pub async fn get_primary_token_symbol(&self) -> Result<String, ContractError> {
        let output: pbjson_types::StringValue = self
            .dynamic()
            .await?
            .call_typed("GetPrimaryTokenSymbol", &pbjson_types::Empty {})
            .await?;
        Ok(output.value)
    }

    /// Transfers tokens to another address.
    pub async fn transfer(
        &self,
        input: &token::TransferInput,
    ) -> Result<SendTransactionOutput, ContractError> {
        self.dynamic().await?.send_typed("Transfer", input).await
    }

    /// Sends tokens to another chain through the token contract.
    pub async fn cross_chain_transfer(
        &self,
        input: &token::CrossChainTransferInput,
    ) -> Result<SendTransactionOutput, ContractError> {
        self.dynamic()
            .await?
            .send_typed("CrossChainTransfer", input)
            .await
    }

    async fn dynamic(&self) -> Result<DynamicContract, ContractError> {
        self.dynamic.get().await
    }
}

/// Typed wrapper for the election contract.
#[derive(Clone)]
pub struct ElectionContract {
    dynamic: LazyDynamicContract,
}

impl ElectionContract {
    /// Creates a typed election contract wrapper.
    pub fn new(client: AElfClient, wallet: Wallet, address: impl Into<String>) -> Self {
        Self {
            dynamic: LazyDynamicContract::new(client, wallet, address),
        }
    }

    /// Returns all registered candidate public keys as hex strings.
    pub async fn get_candidates(&self) -> Result<Vec<String>, ContractError> {
        let output: election::PubkeyList = self
            .dynamic()
            .await?
            .call_typed("GetCandidates", &pbjson_types::Empty {})
            .await?;
        Ok(output.value.into_iter().map(hex::encode).collect())
    }

    /// Returns vote statistics for a candidate public key.
    pub async fn get_candidate_vote(
        &self,
        pubkey: &str,
    ) -> Result<election::CandidateVote, ContractError> {
        self.dynamic()
            .await?
            .call_typed(
                "GetCandidateVote",
                &pbjson_types::StringValue {
                    value: pubkey.to_owned(),
                },
            )
            .await
    }

    /// Returns vote information for an elector public key.
    pub async fn get_elector_vote(
        &self,
        pubkey: &str,
    ) -> Result<election::ElectorVote, ContractError> {
        self.dynamic()
            .await?
            .call_typed(
                "GetElectorVote",
                &pbjson_types::StringValue {
                    value: pubkey.to_owned(),
                },
            )
            .await
    }

    async fn dynamic(&self) -> Result<DynamicContract, ContractError> {
        self.dynamic.get().await
    }
}

/// Typed wrapper for the vote contract.
#[derive(Clone)]
pub struct VoteContract {
    dynamic: LazyDynamicContract,
}

impl VoteContract {
    /// Creates a typed vote contract wrapper.
    pub fn new(client: AElfClient, wallet: Wallet, address: impl Into<String>) -> Self {
        Self {
            dynamic: LazyDynamicContract::new(client, wallet, address),
        }
    }

    /// Returns metadata for a voting item.
    pub async fn get_voting_item(
        &self,
        input: &vote::GetVotingItemInput,
    ) -> Result<vote::VotingItem, ContractError> {
        self.dynamic()
            .await?
            .call_typed("GetVotingItem", input)
            .await
    }

    /// Returns a voting record by vote id hash.
    pub async fn get_voting_record(
        &self,
        vote_id: &aelf_proto::aelf::Hash,
    ) -> Result<vote::VotingRecord, ContractError> {
        self.dynamic()
            .await?
            .call_typed("GetVotingRecord", vote_id)
            .await
    }

    /// Returns the latest voting result for a voting item.
    pub async fn get_latest_voting_result(
        &self,
        voting_item_id: &aelf_proto::aelf::Hash,
    ) -> Result<vote::VotingResult, ContractError> {
        self.dynamic()
            .await?
            .call_typed("GetLatestVotingResult", voting_item_id)
            .await
    }

    async fn dynamic(&self) -> Result<DynamicContract, ContractError> {
        self.dynamic.get().await
    }
}

/// Typed wrapper for the cross-chain contract.
#[derive(Clone)]
pub struct CrossChainContract {
    dynamic: LazyDynamicContract,
}

impl CrossChainContract {
    /// Creates a typed cross-chain contract wrapper.
    pub fn new(client: AElfClient, wallet: Wallet, address: impl Into<String>) -> Self {
        Self {
            dynamic: LazyDynamicContract::new(client, wallet, address),
        }
    }

    /// Returns the parent chain id.
    pub async fn get_parent_chain_id(&self) -> Result<i32, ContractError> {
        let value: pbjson_types::Int32Value = self
            .dynamic()
            .await?
            .call_typed("GetParentChainId", &pbjson_types::Empty {})
            .await?;
        Ok(value.value)
    }

    /// Returns the latest known parent chain height.
    pub async fn get_parent_chain_height(&self) -> Result<i64, ContractError> {
        let value: pbjson_types::Int64Value = self
            .dynamic()
            .await?
            .call_typed("GetParentChainHeight", &pbjson_types::Empty {})
            .await?;
        Ok(value.value)
    }

    /// Returns the latest known side-chain height for the specified chain id.
    pub async fn get_side_chain_height(&self, chain_id: i32) -> Result<i64, ContractError> {
        let value: pbjson_types::Int64Value = self
            .dynamic()
            .await?
            .call_typed(
                "GetSideChainHeight",
                &pbjson_types::Int32Value { value: chain_id },
            )
            .await?;
        Ok(value.value)
    }

    /// Returns chain status information for the specified chain id.
    pub async fn get_chain_status(
        &self,
        chain_id: i32,
    ) -> Result<cross_chain::GetChainStatusOutput, ContractError> {
        self.dynamic()
            .await?
            .call_typed(
                "GetChainStatus",
                &pbjson_types::Int32Value { value: chain_id },
            )
            .await
    }

    async fn dynamic(&self) -> Result<DynamicContract, ContractError> {
        self.dynamic.get().await
    }
}

/// Typed wrapper for the AEDPoS consensus contract.
#[derive(Clone)]
pub struct AedposContract {
    dynamic: LazyDynamicContract,
}

impl AedposContract {
    /// Creates a typed AEDPoS contract wrapper.
    pub fn new(client: AElfClient, wallet: Wallet, address: impl Into<String>) -> Self {
        Self {
            dynamic: LazyDynamicContract::new(client, wallet, address),
        }
    }

    /// Returns the current miner public keys as hex strings.
    pub async fn get_current_miner_list(&self) -> Result<Vec<String>, ContractError> {
        let output: aedpos::MinerList = self
            .dynamic()
            .await?
            .call_typed("GetCurrentMinerList", &pbjson_types::Empty {})
            .await?;
        Ok(output.pubkeys.into_iter().map(hex::encode).collect())
    }

    /// Returns the current consensus round information.
    pub async fn get_current_round_information(&self) -> Result<aedpos::Round, ContractError> {
        self.dynamic()
            .await?
            .call_typed("GetCurrentRoundInformation", &pbjson_types::Empty {})
            .await
    }

    async fn dynamic(&self) -> Result<DynamicContract, ContractError> {
        self.dynamic.get().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aelf_client::provider::Provider;
    use async_trait::async_trait;
    use base64::Engine;
    use http::Method;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    // Public test-only readonly key used by descriptor-related tests. Never fund it.
    const READONLY_PRIVATE_KEY: &str =
        "0000000000000000000000000000000000000000000000000000000000000001";

    fn token_method(name: &str) -> MethodDescriptor {
        let pool = DescriptorPool::decode(aelf_proto::FILE_DESCRIPTOR_SET).expect("descriptor set");
        let method = pool
            .services()
            .find_map(|service| service.methods().find(|method| method.name() == name))
            .expect("token method");
        method
    }

    #[test]
    fn normalizes_address_strings_for_dynamic_input() {
        let wallet = Wallet::from_private_key(READONLY_PRIVATE_KEY).expect("wallet");
        let method = token_method("GetBalance");

        let normalized = normalize_json_input(
            &method.input(),
            json!({
                "symbol": "ELF",
                "owner": wallet.address(),
            }),
        )
        .expect("normalize input");

        let message = DynamicMessage::deserialize(method.input(), normalized.into_deserializer())
            .expect("deserialize");
        let value = serde_json::to_value(message).expect("serialize");

        assert_eq!(
            value.get("owner"),
            Some(
                &serde_json::to_value(address_to_pb(wallet.address()).expect("address"))
                    .expect("json")
            ),
        );
    }

    #[test]
    fn normalizes_address_objects_for_dynamic_output() {
        let wallet = Wallet::from_private_key(READONLY_PRIVATE_KEY).expect("wallet");
        let method = token_method("GetBalance");
        let output = token::GetBalanceOutput {
            symbol: "ELF".to_owned(),
            owner: Some(address_to_pb(wallet.address()).expect("address")),
            balance: 42,
        };

        let normalized = normalize_json_output(
            &method.output(),
            serde_json::to_value(output).expect("json"),
        )
        .expect("normalize output");

        assert_eq!(normalized.get("owner"), Some(&json!(wallet.address())));
        assert_eq!(normalized.get("symbol"), Some(&json!("ELF")));
    }

    #[derive(Clone)]
    struct CountingDescriptorProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Provider for CountingDescriptorProvider {
        async fn request_json(
            &self,
            _method: Method,
            _path: &str,
            _query: &[(&str, String)],
            _body: Option<Value>,
        ) -> Result<Value, AElfError> {
            Err(AElfError::request(
                "unexpected JSON request in descriptor test",
                None,
            ))
        }

        async fn request_text(
            &self,
            method: Method,
            path: &str,
            query: &[(&str, String)],
            _body: Option<Value>,
        ) -> Result<String, AElfError> {
            assert_eq!(method, Method::GET);
            assert_eq!(path, "api/blockChain/contractFileDescriptorSet");
            assert_eq!(query, &[("address", "token-contract".to_owned())]);

            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(format!(
                "\"{}\"",
                base64::engine::general_purpose::STANDARD.encode(aelf_proto::FILE_DESCRIPTOR_SET)
            ))
        }
    }

    #[tokio::test]
    async fn dynamic_contract_fetches_descriptor_on_every_at_call() {
        let requests = Arc::new(AtomicUsize::new(0));
        let client = AElfClient::with_provider(CountingDescriptorProvider {
            requests: requests.clone(),
        })
        .expect("client");
        let wallet = Wallet::from_private_key(READONLY_PRIVATE_KEY).expect("wallet");

        let first = DynamicContract::at(client.clone(), "token-contract", wallet.clone())
            .await
            .expect("first contract");
        let second = DynamicContract::at(client, "token-contract", wallet)
            .await
            .expect("second contract");

        assert!(first.method("GetBalance").is_ok());
        assert!(second.method("GetBalance").is_ok());
        assert_eq!(requests.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn typed_wrapper_reuses_descriptor_within_same_handle() {
        let requests = Arc::new(AtomicUsize::new(0));
        let client = AElfClient::with_provider(CountingDescriptorProvider {
            requests: requests.clone(),
        })
        .expect("client");
        let wallet = Wallet::from_private_key(READONLY_PRIVATE_KEY).expect("wallet");
        let token = TokenContract::new(client, wallet, "token-contract");

        let first = token.dynamic().await.expect("first dynamic");
        let second = token.dynamic().await.expect("second dynamic");

        assert!(first.method("GetBalance").is_ok());
        assert!(second.method("GetBalance").is_ok());
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn typed_wrapper_clone_reuses_descriptor_cache() {
        let requests = Arc::new(AtomicUsize::new(0));
        let client = AElfClient::with_provider(CountingDescriptorProvider {
            requests: requests.clone(),
        })
        .expect("client");
        let wallet = Wallet::from_private_key(READONLY_PRIVATE_KEY).expect("wallet");
        let first = TokenContract::new(client, wallet, "token-contract");
        let second = first.clone();

        assert!(first
            .dynamic()
            .await
            .expect("first dynamic")
            .method("GetBalance")
            .is_ok());
        assert!(second
            .dynamic()
            .await
            .expect("second dynamic")
            .method("GetBalance")
            .is_ok());
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
}
