//! Async HTTP client, DTOs and transaction builder primitives for AElf.

#![forbid(unsafe_code)]

pub mod config;
pub mod dto;
pub mod error;
#[doc(hidden)]
pub mod protobuf;
pub mod provider;

#[cfg(test)]
mod tests;

pub use crate::error::AElfError;

#[cfg(feature = "native-http")]
use crate::config::ClientConfig;
use crate::dto::{
    BlockDto, CalculateTransactionFeeInput, CalculateTransactionFeeOutput, ChainStatusDto,
    CreateRawTransactionInput, CreateRawTransactionOutput, ExecuteRawTransactionDto, MerklePathDto,
    NetworkInfoOutput, PeerDto, SendRawTransactionInput, SendRawTransactionOutput,
    SendTransactionOutput, TaskQueueInfoDto, TransactionPoolStatusOutput, TransactionResultDto,
};
use crate::protobuf::RawBytesMessage;
#[cfg(feature = "native-http")]
use crate::provider::HttpProvider;
use crate::provider::Provider;
use aelf_crypto::{
    address_from_public_key, address_to_pb, base58_to_chain_id, decode_address, pb_to_address,
    sha256_bytes, sign_transaction, Wallet,
};
use aelf_proto::aelf::{Address, Hash, Transaction};
use base64::Engine;
use http::Method;
use prost::Message;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroize;

const API_BASE: &str = "api/blockChain";
const NET_API_BASE: &str = "api/net";
const READONLY_PRIVATE_KEY: &str =
    "0000000000000000000000000000000000000000000000000000000000000001";

fn strip_transaction_id_quotes(value: &str) -> &str {
    let trimmed = value.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|unquoted| unquoted.strip_suffix('"'))
        .unwrap_or(trimmed)
}

fn is_valid_transaction_id(value: &str) -> bool {
    let candidate = strip_transaction_id_quotes(value);
    let candidate = candidate
        .strip_prefix("0x")
        .or_else(|| candidate.strip_prefix("0X"))
        .unwrap_or(candidate);
    candidate.len() == 64 && candidate.chars().all(|char| char.is_ascii_hexdigit())
}

fn validate_transaction_id(
    transaction_id: impl Into<String>,
    raw_response: &str,
) -> Result<String, AElfError> {
    let transaction_id = transaction_id.into();
    let transaction_id = strip_transaction_id_quotes(&transaction_id).to_owned();
    if transaction_id.is_empty() {
        return Err(AElfError::UnexpectedResponse(
            "empty sendTransaction response".to_owned(),
        ));
    }
    if is_valid_transaction_id(&transaction_id) {
        Ok(transaction_id)
    } else {
        Err(AElfError::UnexpectedResponse(format!(
            "sendTransaction returned a non-transaction id payload: {raw_response}"
        )))
    }
}

/// Async HTTP client used by the facade crate and lower-level integrations.
#[derive(Clone)]
pub struct AElfClient {
    provider: Arc<dyn Provider>,
}

impl AElfClient {
    /// Creates a client backed by the default HTTP provider.
    #[cfg(feature = "native-http")]
    pub fn new(config: ClientConfig) -> Result<Self, AElfError> {
        Self::with_provider(HttpProvider::new(config)?)
    }

    /// Creates a client from a custom provider implementation.
    pub fn with_provider<P>(provider: P) -> Result<Self, AElfError>
    where
        P: Provider + 'static,
    {
        Ok(Self {
            provider: Arc::new(provider),
        })
    }

    /// Returns block-related API helpers.
    pub fn block(&self) -> BlockService {
        BlockService {
            client: self.clone(),
        }
    }

    /// Returns chain-related API helpers.
    pub fn chain(&self) -> ChainService {
        ChainService {
            client: self.clone(),
        }
    }

    /// Returns network-related API helpers.
    pub fn net(&self) -> NetService {
        NetService {
            client: self.clone(),
        }
    }

    /// Returns transaction-related API helpers.
    pub fn tx(&self) -> TransactionService {
        TransactionService {
            client: self.clone(),
        }
    }

    /// Returns local utility helpers for transaction and address workflows.
    pub fn utils(&self) -> ClientUtilsService {
        ClientUtilsService {
            client: self.clone(),
        }
    }

    /// Creates a transaction builder bound to this client.
    pub fn transaction_builder(&self) -> TransactionBuilder {
        TransactionBuilder::new(self.clone())
    }

    async fn get_json<T>(&self, path: &str, query: &[(&str, String)]) -> Result<T, AElfError>
    where
        T: serde::de::DeserializeOwned,
    {
        let value = self
            .provider
            .request_json(Method::GET, path, query, None)
            .await?;
        serde_json::from_value(value).map_err(AElfError::Json)
    }

    async fn post_json<T>(&self, path: &str, body: serde_json::Value) -> Result<T, AElfError>
    where
        T: serde::de::DeserializeOwned,
    {
        let value = self
            .provider
            .request_json(Method::POST, path, &[], Some(body))
            .await?;
        serde_json::from_value(value).map_err(AElfError::Json)
    }

    async fn get_text(&self, path: &str, query: &[(&str, String)]) -> Result<String, AElfError> {
        self.provider
            .request_text(Method::GET, path, query, None)
            .await
    }

    async fn post_text(&self, path: &str, body: serde_json::Value) -> Result<String, AElfError> {
        self.provider
            .request_text(Method::POST, path, &[], Some(body))
            .await
    }
}

#[derive(Clone)]
pub struct BlockService {
    client: AElfClient,
}

impl BlockService {
    pub async fn get_block_height(&self) -> Result<i64, AElfError> {
        let text = self
            .client
            .get_text(&format!("{API_BASE}/blockHeight"), &[])
            .await?;
        text.trim_matches('"')
            .parse::<i64>()
            .map_err(|err| AElfError::InvalidConfig(format!("invalid block height: {err}")))
    }

    pub async fn get_block_by_hash(
        &self,
        block_hash: &str,
        include_transactions: bool,
    ) -> Result<BlockDto, AElfError> {
        self.client
            .get_json(
                &format!("{API_BASE}/block"),
                &[
                    ("blockHash", block_hash.to_owned()),
                    ("includeTransactions", include_transactions.to_string()),
                ],
            )
            .await
    }

    pub async fn get_block_by_height(
        &self,
        block_height: i64,
        include_transactions: bool,
    ) -> Result<BlockDto, AElfError> {
        self.client
            .get_json(
                &format!("{API_BASE}/blockByHeight"),
                &[
                    ("blockHeight", block_height.to_string()),
                    ("includeTransactions", include_transactions.to_string()),
                ],
            )
            .await
    }
}

#[derive(Clone)]
pub struct ChainService {
    client: AElfClient,
}

impl ChainService {
    pub async fn get_chain_status(&self) -> Result<ChainStatusDto, AElfError> {
        self.client
            .get_json(&format!("{API_BASE}/chainStatus"), &[])
            .await
    }

    pub async fn get_contract_file_descriptor_set(
        &self,
        address: &str,
    ) -> Result<Vec<u8>, AElfError> {
        let text = self
            .client
            .get_text(
                &format!("{API_BASE}/contractFileDescriptorSet"),
                &[("address", address.to_owned())],
            )
            .await?;
        Ok(base64::engine::general_purpose::STANDARD.decode(text.trim_matches('"'))?)
    }

    pub async fn get_task_queue_status(&self) -> Result<Vec<TaskQueueInfoDto>, AElfError> {
        self.client
            .get_json(&format!("{API_BASE}/taskQueueStatus"), &[])
            .await
    }

    pub async fn get_chain_id(&self) -> Result<i32, AElfError> {
        let status = self.get_chain_status().await?;
        base58_to_chain_id(&status.chain_id).map_err(AElfError::Crypto)
    }
}

#[derive(Clone)]
pub struct NetService {
    client: AElfClient,
}

impl NetService {
    pub async fn add_peer(&self, address: &str) -> Result<bool, AElfError> {
        self.client
            .post_json(
                &format!("{NET_API_BASE}/peer"),
                serde_json::json!({ "Address": address }),
            )
            .await
    }

    pub async fn remove_peer(&self, address: &str) -> Result<bool, AElfError> {
        let text = self
            .client
            .provider
            .request_text(
                Method::DELETE,
                &format!("{NET_API_BASE}/peer"),
                &[("address", address.to_owned())],
                None,
            )
            .await?;
        serde_json::from_str(&text).or_else(|_| Ok(text.trim().eq_ignore_ascii_case("true")))
    }

    pub async fn get_peers(&self, with_metrics: bool) -> Result<Vec<PeerDto>, AElfError> {
        self.client
            .get_json(
                &format!("{NET_API_BASE}/peers"),
                &[("withMetrics", with_metrics.to_string())],
            )
            .await
    }

    pub async fn get_network_info(&self) -> Result<NetworkInfoOutput, AElfError> {
        self.client
            .get_json(&format!("{NET_API_BASE}/networkInfo"), &[])
            .await
    }
}

#[derive(Clone)]
pub struct TransactionService {
    client: AElfClient,
}

impl TransactionService {
    pub async fn get_transaction_pool_status(
        &self,
    ) -> Result<TransactionPoolStatusOutput, AElfError> {
        self.client
            .get_json(&format!("{API_BASE}/transactionPoolStatus"), &[])
            .await
    }

    pub async fn execute_transaction(&self, raw_transaction: &str) -> Result<String, AElfError> {
        self.client
            .post_text(
                &format!("{API_BASE}/executeTransaction"),
                serde_json::json!({ "RawTransaction": raw_transaction }),
            )
            .await
    }

    pub async fn execute_raw_transaction(
        &self,
        input: &ExecuteRawTransactionDto,
    ) -> Result<String, AElfError> {
        self.client
            .post_text(
                &format!("{API_BASE}/executeRawTransaction"),
                serde_json::json!({
                    "RawTransaction": input.raw_transaction,
                    "Signature": input.signature,
                }),
            )
            .await
    }

    pub async fn create_raw_transaction(
        &self,
        input: &CreateRawTransactionInput,
    ) -> Result<CreateRawTransactionOutput, AElfError> {
        self.client
            .post_json(
                &format!("{API_BASE}/rawTransaction"),
                serde_json::to_value(input)?,
            )
            .await
    }

    pub async fn send_raw_transaction(
        &self,
        input: &SendRawTransactionInput,
    ) -> Result<SendRawTransactionOutput, AElfError> {
        self.client
            .post_json(
                &format!("{API_BASE}/sendRawTransaction"),
                serde_json::to_value(input)?,
            )
            .await
    }

    pub async fn send_transaction(
        &self,
        raw_transaction: &str,
    ) -> Result<SendTransactionOutput, AElfError> {
        let text = self
            .client
            .post_text(
                &format!("{API_BASE}/sendTransaction"),
                serde_json::json!({ "RawTransaction": raw_transaction }),
            )
            .await?;
        if let Ok(output) = serde_json::from_str::<SendTransactionOutput>(&text) {
            return Ok(SendTransactionOutput {
                transaction_id: validate_transaction_id(output.transaction_id, &text)?,
            });
        }

        if let Ok(transaction_id) = serde_json::from_str::<String>(&text) {
            return Ok(SendTransactionOutput {
                transaction_id: validate_transaction_id(transaction_id, &text)?,
            });
        }

        Ok(SendTransactionOutput {
            transaction_id: validate_transaction_id(&text, &text)?,
        })
    }

    pub async fn send_transactions(
        &self,
        raw_transactions: &str,
    ) -> Result<Vec<String>, AElfError> {
        self.client
            .post_json(
                &format!("{API_BASE}/sendTransactions"),
                serde_json::json!({ "RawTransactions": raw_transactions }),
            )
            .await
    }

    pub async fn get_transaction_result(
        &self,
        transaction_id: &str,
    ) -> Result<TransactionResultDto, AElfError> {
        self.client
            .get_json(
                &format!("{API_BASE}/transactionResult"),
                &[("transactionId", transaction_id.to_owned())],
            )
            .await
    }

    pub async fn get_transaction_results(
        &self,
        block_hash: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TransactionResultDto>, AElfError> {
        self.client
            .get_json(
                &format!("{API_BASE}/transactionResults"),
                &[
                    ("blockHash", block_hash.to_owned()),
                    ("offset", offset.to_string()),
                    ("limit", limit.to_string()),
                ],
            )
            .await
    }

    pub async fn get_merkle_path_by_transaction_id(
        &self,
        transaction_id: &str,
    ) -> Result<MerklePathDto, AElfError> {
        self.client
            .get_json(
                &format!("{API_BASE}/merklePathByTransactionId"),
                &[("transactionId", transaction_id.to_owned())],
            )
            .await
    }

    pub async fn calculate_transaction_fee(
        &self,
        input: &CalculateTransactionFeeInput,
    ) -> Result<CalculateTransactionFeeOutput, AElfError> {
        self.client
            .post_json(
                &format!("{API_BASE}/calculateTransactionFee"),
                serde_json::to_value(input)?,
            )
            .await
    }
}

#[derive(Clone)]
pub struct ClientUtilsService {
    client: AElfClient,
}

impl ClientUtilsService {
    pub async fn is_connected(&self) -> bool {
        self.client.chain().get_chain_status().await.is_ok()
    }

    pub fn get_address_from_pub_key(&self, public_key_hex: &str) -> Result<String, AElfError> {
        let public_key = hex::decode(public_key_hex)?;
        Ok(address_from_public_key(&public_key))
    }

    pub fn get_address_from_private_key(&self, private_key_hex: &str) -> Result<String, AElfError> {
        let wallet = Wallet::from_private_key(private_key_hex)?;
        Ok(wallet.address().to_owned())
    }

    pub fn generate_key_pair_info(&self) -> Result<KeyPairInfo, AElfError> {
        let wallet = Wallet::create()?;
        Ok(KeyPairInfo::from_wallet(&wallet))
    }

    pub async fn get_genesis_contract_address(&self) -> Result<String, AElfError> {
        let status = self.client.chain().get_chain_status().await?;
        Ok(status.genesis_contract_address)
    }

    pub async fn get_contract_address_by_name(
        &self,
        contract_name: &str,
    ) -> Result<String, AElfError> {
        let readonly_wallet = Wallet::from_private_key(READONLY_PRIVATE_KEY)?;
        let genesis_address = self.get_genesis_contract_address().await?;
        let request = Hash {
            value: sha256_bytes(contract_name.as_bytes()).to_vec(),
        };
        let transaction = self
            .generate_transaction(
                readonly_wallet.address(),
                &genesis_address,
                "GetContractAddressByName",
                &request,
            )
            .await?;
        let signed = self.sign_transaction(&readonly_wallet, transaction)?;
        let raw = hex::encode(signed.encode_to_vec());
        let response = self.client.tx().execute_transaction(&raw).await?;
        let bytes = hex::decode(response.trim_matches('"'))?;
        let address = Address::decode(bytes.as_slice())?;
        Ok(pb_to_address(&address))
    }

    pub async fn get_formatted_address(&self, address: &str) -> Result<String, AElfError> {
        let readonly_wallet = Wallet::from_private_key(READONLY_PRIVATE_KEY)?;
        let token_address = self
            .get_contract_address_by_name("AElf.ContractNames.Token")
            .await?;
        let transaction = self
            .generate_transaction(
                readonly_wallet.address(),
                &token_address,
                "GetPrimaryTokenSymbol",
                &pbjson_types::Empty {},
            )
            .await?;
        let signed = self.sign_transaction(&readonly_wallet, transaction)?;
        let raw = hex::encode(signed.encode_to_vec());
        let response = self.client.tx().execute_transaction(&raw).await?;
        let bytes = hex::decode(response.trim_matches('"'))?;
        let symbol = pbjson_types::StringValue::decode(bytes.as_slice())?;
        let status = self.client.chain().get_chain_status().await?;
        Ok(format!("{}_{}_{}", symbol.value, address, status.chain_id))
    }

    pub async fn generate_transaction<M>(
        &self,
        from: &str,
        to: &str,
        method_name: &str,
        input: &M,
    ) -> Result<Transaction, AElfError>
    where
        M: Message,
    {
        let chain_status = self.client.chain().get_chain_status().await?;
        let best_chain_hash = chain_status.best_chain_hash.trim_start_matches("0x");
        let hash_bytes = hex::decode(best_chain_hash)?;
        let prefix = hash_bytes
            .get(..4)
            .ok_or_else(|| AElfError::request("best chain hash is too short", None))?;
        Ok(Transaction {
            from: Some(address_to_pb(from)?),
            to: Some(address_to_pb(to)?),
            ref_block_number: chain_status.best_chain_height,
            ref_block_prefix: prefix.to_vec(),
            method_name: method_name.to_owned(),
            params: input.encode_to_vec(),
            signature: Vec::new(),
        })
    }

    pub fn sign_transaction(
        &self,
        wallet: &Wallet,
        mut transaction: Transaction,
    ) -> Result<Transaction, AElfError> {
        transaction.signature = sign_transaction(wallet, &transaction)?;
        Ok(transaction)
    }

    pub fn decode_address(&self, address: &str) -> Result<Vec<u8>, AElfError> {
        decode_address(address).map_err(AElfError::Crypto)
    }
}

/// Basic key material derived from a wallet.
///
/// The private key is zeroized on drop and redacted from `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyPairInfo {
    pub private_key: String,
    pub public_key: String,
    pub address: String,
}

impl KeyPairInfo {
    /// Builds a serializable key bundle from a wallet.
    pub fn from_wallet(wallet: &Wallet) -> Self {
        Self {
            private_key: wallet.private_key().to_owned(),
            public_key: wallet.public_key().to_owned(),
            address: wallet.address().to_owned(),
        }
    }
}

impl fmt::Debug for KeyPairInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyPairInfo")
            .field("private_key", &"<redacted>")
            .field("public_key", &self.public_key)
            .field("address", &self.address)
            .finish()
    }
}

impl Drop for KeyPairInfo {
    fn drop(&mut self) {
        self.private_key.zeroize();
    }
}

/// Builder for unsigned or signed AElf protobuf transactions.
#[derive(Clone)]
pub struct TransactionBuilder {
    client: AElfClient,
    wallet: Option<Wallet>,
    contract_address: Option<String>,
    system_contract_name: Option<String>,
    method_name: Option<String>,
    params: Option<Vec<u8>>,
}

impl TransactionBuilder {
    /// Creates a new builder bound to an existing client.
    pub fn new(client: AElfClient) -> Self {
        Self {
            client,
            wallet: None,
            contract_address: None,
            system_contract_name: None,
            method_name: None,
            params: None,
        }
    }

    /// Sets the wallet used for address resolution and signing.
    pub fn with_wallet(mut self, wallet: Wallet) -> Self {
        self.wallet = Some(wallet);
        self
    }

    /// Sets the target contract address explicitly.
    pub fn with_contract(mut self, address: impl Into<String>) -> Self {
        self.contract_address = Some(address.into());
        self
    }

    /// Resolves the contract address from a known system contract name.
    pub fn with_system_contract(mut self, name: impl Into<String>) -> Self {
        self.system_contract_name = Some(name.into());
        self
    }

    /// Sets the target contract method name.
    pub fn with_method(mut self, method_name: impl Into<String>) -> Self {
        self.method_name = Some(method_name.into());
        self
    }

    /// Encodes a protobuf message into the transaction `params` payload.
    pub fn with_message<M: Message>(mut self, message: &M) -> Self {
        self.params = Some(message.encode_to_vec());
        self
    }

    /// Builds an unsigned protobuf transaction.
    pub async fn build_unsigned(self) -> Result<Transaction, AElfError> {
        let wallet = self.wallet.ok_or(AElfError::MissingField("wallet"))?;
        let contract_address = match (self.contract_address, self.system_contract_name) {
            (Some(address), _) => address,
            (None, Some(name)) => {
                self.client
                    .utils()
                    .get_contract_address_by_name(&name)
                    .await?
            }
            (None, None) => return Err(AElfError::MissingField("contract address")),
        };
        let method_name = self
            .method_name
            .ok_or(AElfError::MissingField("method name"))?;
        let params = self.params.unwrap_or_default();
        self.client
            .utils()
            .generate_transaction(
                wallet.address(),
                &contract_address,
                &method_name,
                &RawBytesMessage::new(params),
            )
            .await
    }

    /// Builds and signs a protobuf transaction with the selected wallet.
    pub async fn build_signed(self) -> Result<Transaction, AElfError> {
        let wallet = self
            .wallet
            .clone()
            .ok_or(AElfError::MissingField("wallet"))?;
        let unsigned = self.clone().build_unsigned().await?;
        self.client.utils().sign_transaction(&wallet, unsigned)
    }
}
