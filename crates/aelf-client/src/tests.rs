use crate::dto::CreateRawTransactionInput;
use crate::provider::{MockCallKind, MockProvider, MockRecordedRequest, MockResponse};
use crate::{AElfClient, AElfError};
use http::Method;
use serde_json::{json, Value};

const VALID_TX_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn mock_client(responses: Vec<MockResponse>) -> (AElfClient, MockProvider) {
    let provider = MockProvider::new(responses);
    let client = AElfClient::with_provider(provider.clone()).expect("client");
    (client, provider)
}

fn chain_status_json() -> Value {
    json!({
        "ChainId": "AELF",
        "Branches": {},
        "NotLinkedBlocks": {},
        "LongestChainHeight": 1,
        "LongestChainHash": "0x01",
        "GenesisBlockHash": "0x02",
        "GenesisContractAddress": "genesis-address",
        "LastIrreversibleBlockHash": "0x03",
        "LastIrreversibleBlockHeight": 1,
        "BestChainHash": "04050607",
        "BestChainHeight": 2
    })
}

fn assert_single_request(provider: &MockProvider) -> MockRecordedRequest {
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    requests.into_iter().next().expect("request")
}

#[tokio::test]
async fn send_transaction_accepts_object_response() {
    let (client, provider) = mock_client(vec![MockResponse::text(format!(
        r#"{{"TransactionId":"{VALID_TX_ID}"}}"#
    ))]);

    let output = client
        .tx()
        .send_transaction("raw-transaction")
        .await
        .expect("send transaction");

    assert_eq!(output.transaction_id, VALID_TX_ID);

    let request = assert_single_request(&provider);
    assert_eq!(request.kind, MockCallKind::Text);
    assert_eq!(request.method, Method::POST);
    assert_eq!(request.path, "api/blockChain/sendTransaction");
    assert_eq!(
        request.body,
        Some(json!({ "RawTransaction": "raw-transaction" }))
    );
}

#[tokio::test]
async fn send_transaction_accepts_json_string_response() {
    let (client, _) = mock_client(vec![MockResponse::text(format!(r#""{VALID_TX_ID}""#))]);

    let output = client
        .tx()
        .send_transaction("raw-transaction")
        .await
        .expect("send transaction");

    assert_eq!(output.transaction_id, VALID_TX_ID);
}

#[tokio::test]
async fn send_transaction_accepts_plain_text_response() {
    let (client, _) = mock_client(vec![MockResponse::text(VALID_TX_ID)]);

    let output = client
        .tx()
        .send_transaction("raw-transaction")
        .await
        .expect("send transaction");

    assert_eq!(output.transaction_id, VALID_TX_ID);
}

#[tokio::test]
async fn send_transaction_accepts_0x_prefixed_plain_text_response() {
    let (client, _) = mock_client(vec![MockResponse::text(format!("0x{VALID_TX_ID}"))]);

    let output = client
        .tx()
        .send_transaction("raw-transaction")
        .await
        .expect("send transaction");

    assert_eq!(output.transaction_id, format!("0x{VALID_TX_ID}"));
}

#[tokio::test]
async fn send_transaction_rejects_empty_response() {
    let (client, _) = mock_client(vec![MockResponse::text("")]);

    let error = client
        .tx()
        .send_transaction("raw-transaction")
        .await
        .expect_err("empty response should fail");

    assert!(matches!(error, AElfError::UnexpectedResponse(_)));
}

#[tokio::test]
async fn send_transaction_rejects_non_txid_plain_text_response() {
    let (client, _) = mock_client(vec![MockResponse::text("tx-plain")]);

    let error = client
        .tx()
        .send_transaction("raw-transaction")
        .await
        .expect_err("non-txid response should fail");

    assert!(matches!(error, AElfError::UnexpectedResponse(_)));
}

#[tokio::test]
async fn send_transaction_rejects_ok_plain_text_response() {
    let (client, _) = mock_client(vec![MockResponse::text("ok")]);

    let error = client
        .tx()
        .send_transaction("raw-transaction")
        .await
        .expect_err("ok response should fail");

    assert!(matches!(error, AElfError::UnexpectedResponse(_)));
}

#[tokio::test]
async fn get_block_height_parses_quoted_number() {
    let (client, provider) = mock_client(vec![MockResponse::text("\"42\"")]);

    let height = client
        .block()
        .get_block_height()
        .await
        .expect("block height");

    assert_eq!(height, 42);
    let request = assert_single_request(&provider);
    assert_eq!(request.kind, MockCallKind::Text);
    assert_eq!(request.method, Method::GET);
    assert_eq!(request.path, "api/blockChain/blockHeight");
    assert!(request.query.is_empty());
    assert_eq!(request.body, None);
}

#[tokio::test]
async fn get_block_height_rejects_invalid_text() {
    let (client, _) = mock_client(vec![MockResponse::text("\"not-a-number\"")]);

    let error = client
        .block()
        .get_block_height()
        .await
        .expect_err("invalid height should fail");

    match error {
        AElfError::InvalidConfig(message) => {
            assert!(message.contains("invalid block height"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn create_raw_transaction_uses_pascal_case_body() {
    let (client, provider) = mock_client(vec![MockResponse::json(json!({
        "RawTransaction": "raw-transaction"
    }))]);

    let output = client
        .tx()
        .create_raw_transaction(&CreateRawTransactionInput {
            from: "from-address".to_owned(),
            to: "to-address".to_owned(),
            ref_block_number: 100,
            ref_block_hash: "abcd".to_owned(),
            method_name: "Transfer".to_owned(),
            params: "{\"amount\":1}".to_owned(),
        })
        .await
        .expect("raw transaction");

    assert_eq!(output.raw_transaction, "raw-transaction");
    let request = assert_single_request(&provider);
    assert_eq!(request.kind, MockCallKind::Json);
    assert_eq!(request.method, Method::POST);
    assert_eq!(request.path, "api/blockChain/rawTransaction");
    assert_eq!(
        request.body,
        Some(json!({
            "From": "from-address",
            "To": "to-address",
            "RefBlockNumber": 100,
            "RefBlockHash": "abcd",
            "MethodName": "Transfer",
            "Params": "{\"amount\":1}"
        }))
    );
}

#[tokio::test]
async fn get_transaction_result_passes_query_parameter() {
    let (client, provider) = mock_client(vec![MockResponse::json(json!({
        "TransactionId": "txid",
        "Status": "MINED"
    }))]);

    let output = client
        .tx()
        .get_transaction_result("txid")
        .await
        .expect("transaction result");

    assert_eq!(output.transaction_id, "txid");
    assert_eq!(output.status, "MINED");

    let request = assert_single_request(&provider);
    assert_eq!(request.kind, MockCallKind::Json);
    assert_eq!(request.method, Method::GET);
    assert_eq!(request.path, "api/blockChain/transactionResult");
    assert_eq!(
        request.query,
        vec![("transactionId".to_owned(), "txid".to_owned())]
    );
    assert_eq!(request.body, None);
}

#[tokio::test]
async fn is_connected_returns_true_when_chain_status_succeeds() {
    let (client, provider) = mock_client(vec![MockResponse::json(chain_status_json())]);

    assert!(client.utils().is_connected().await);
    let request = assert_single_request(&provider);
    assert_eq!(request.kind, MockCallKind::Json);
    assert_eq!(request.method, Method::GET);
    assert_eq!(request.path, "api/blockChain/chainStatus");
}

#[tokio::test]
async fn is_connected_returns_false_when_provider_fails() {
    let (client, provider) = mock_client(vec![MockResponse::json_error(AElfError::request(
        "offline", None,
    ))]);

    assert!(!client.utils().is_connected().await);
    let request = assert_single_request(&provider);
    assert_eq!(request.kind, MockCallKind::Json);
    assert_eq!(request.method, Method::GET);
    assert_eq!(request.path, "api/blockChain/chainStatus");
}
