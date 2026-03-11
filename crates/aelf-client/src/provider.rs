#[cfg(feature = "native-http")]
use crate::config::{ClientConfig, RetryPolicy};
use crate::error::AElfError;
use async_trait::async_trait;
use http::Method;
#[cfg(feature = "native-http")]
use reqwest::header::{ACCEPT, CONTENT_TYPE};
#[cfg(feature = "native-http")]
use reqwest::StatusCode;
use serde_json::Value;
#[cfg(feature = "native-http")]
use std::time::Duration;

/// Abstract transport used by the SDK client.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Sends a request and parses the response body as JSON.
    async fn request_json(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value, AElfError>;

    /// Sends a request and returns the raw response text.
    async fn request_text(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<String, AElfError>;
}

/// Default HTTP transport backed by `reqwest`.
#[cfg(feature = "native-http")]
#[derive(Clone, Debug)]
pub struct HttpProvider {
    config: ClientConfig,
    client: reqwest::Client,
}

#[cfg(feature = "native-http")]
impl HttpProvider {
    /// Creates a new HTTP transport from client configuration.
    pub fn new(config: ClientConfig) -> Result<Self, AElfError> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(AElfError::Http)?;
        Ok(Self { config, client })
    }

    fn make_url(&self, path: &str) -> String {
        let endpoint = self.config.endpoint.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        format!("{endpoint}/{path}")
    }

    fn versioned_content_type(&self) -> String {
        match &self.config.api_version {
            Some(version) => format!("application/json;v={version}"),
            None => "application/json".to_owned(),
        }
    }

    fn build_request(
        &self,
        method: &Method,
        url: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> reqwest::RequestBuilder {
        let mut request = self.client.request(method.clone(), url);

        request = request.header(ACCEPT, self.versioned_content_type());
        if *method != Method::GET {
            request = request.header(CONTENT_TYPE, self.versioned_content_type());
        }

        for (name, value) in &self.config.headers {
            request = request.header(name, value);
        }

        if let Some(auth) = &self.config.basic_auth {
            request = request.basic_auth(&auth.username, Some(&auth.password));
        }

        if !query.is_empty() {
            request = request.query(&query);
        }

        if let Some(body) = body {
            request = request.json(body);
        }

        request
    }

    fn should_retry_status(status: StatusCode) -> bool {
        status.is_server_error()
    }

    fn should_retry_error(error: &reqwest::Error) -> bool {
        error.is_connect() || error.is_timeout() || error.is_request() || error.is_body()
    }

    async fn sleep_before_retry(backoff: Duration) {
        if !backoff.is_zero() {
            tokio::time::sleep(backoff).await;
        }
    }

    async fn retry_after(backoff: &mut Duration, retries_remaining: &mut usize) {
        let current = *backoff;
        *retries_remaining -= 1;
        *backoff = backoff.saturating_mul(2);
        Self::sleep_before_retry(current).await;
    }
}

#[cfg(feature = "native-http")]
#[async_trait]
impl Provider for HttpProvider {
    async fn request_json(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value, AElfError> {
        let text = self.request_text(method, path, query, body).await?;
        serde_json::from_str(&text).map_err(AElfError::Json)
    }

    async fn request_text(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<String, AElfError> {
        let url = self.make_url(path);
        let RetryPolicy {
            mut max_retries,
            mut initial_backoff,
        } = self.config.retry_policy;

        loop {
            let request = self.build_request(&method, &url, query, body.as_ref());
            let response = match request.send().await {
                Ok(response) => response,
                Err(error) if max_retries > 0 && Self::should_retry_error(&error) => {
                    Self::retry_after(&mut initial_backoff, &mut max_retries).await;
                    continue;
                }
                Err(error) => return Err(AElfError::Http(error)),
            };

            let status = response.status();
            let request_id = response
                .headers()
                .get("x-request-id")
                .or_else(|| response.headers().get("request-id"))
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned);
            let text = match response.text().await {
                Ok(text) => text,
                Err(error) if max_retries > 0 && Self::should_retry_error(&error) => {
                    Self::retry_after(&mut initial_backoff, &mut max_retries).await;
                    continue;
                }
                Err(error) => return Err(AElfError::Http(error)),
            };

            if status.is_success() {
                return Ok(text);
            }

            if max_retries > 0 && Self::should_retry_status(status) {
                Self::retry_after(&mut initial_backoff, &mut max_retries).await;
                continue;
            }

            return Err(AElfError::from_response(
                url.clone(),
                status,
                request_id,
                &text,
            ));
        }
    }
}

#[cfg(test)]
pub(crate) use test_support::{MockCallKind, MockProvider, MockRecordedRequest, MockResponse};

#[cfg(test)]
mod test_support {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum MockCallKind {
        Json,
        Text,
    }

    #[derive(Debug)]
    pub(crate) enum MockResponse {
        Json(Result<Value, AElfError>),
        Text(Result<String, AElfError>),
    }

    impl MockResponse {
        pub(crate) fn json(value: Value) -> Self {
            Self::Json(Ok(value))
        }

        pub(crate) fn json_error(error: AElfError) -> Self {
            Self::Json(Err(error))
        }

        pub(crate) fn text(value: impl Into<String>) -> Self {
            Self::Text(Ok(value.into()))
        }

        fn kind(&self) -> MockResponseKind {
            match self {
                Self::Json(_) => MockResponseKind::Json,
                Self::Text(_) => MockResponseKind::Text,
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum MockResponseKind {
        Json,
        Text,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub(crate) struct MockRecordedRequest {
        pub kind: MockCallKind,
        pub method: Method,
        pub path: String,
        pub query: Vec<(String, String)>,
        pub body: Option<Value>,
    }

    #[derive(Default)]
    struct MockProviderState {
        responses: Mutex<VecDeque<MockResponse>>,
        requests: Mutex<Vec<MockRecordedRequest>>,
    }

    /// FIFO provider double used for client/service unit tests.
    #[derive(Clone, Default)]
    pub(crate) struct MockProvider {
        state: Arc<MockProviderState>,
    }

    impl MockProvider {
        pub(crate) fn new(responses: Vec<MockResponse>) -> Self {
            Self {
                state: Arc::new(MockProviderState {
                    responses: Mutex::new(responses.into()),
                    requests: Mutex::new(Vec::new()),
                }),
            }
        }

        pub(crate) fn requests(&self) -> Vec<MockRecordedRequest> {
            self.state
                .requests
                .lock()
                .expect("recorded requests lock")
                .clone()
        }

        fn record(
            &self,
            kind: MockCallKind,
            method: Method,
            path: &str,
            query: &[(&str, String)],
            body: Option<Value>,
        ) {
            self.state
                .requests
                .lock()
                .expect("recorded requests lock")
                .push(MockRecordedRequest {
                    kind,
                    method,
                    path: path.to_owned(),
                    query: query
                        .iter()
                        .map(|(name, value)| ((*name).to_owned(), value.clone()))
                        .collect(),
                    body,
                });
        }

        fn pop_response(&self, expected: MockResponseKind) -> MockResponse {
            let response = self
                .state
                .responses
                .lock()
                .expect("mock response lock")
                .pop_front()
                .unwrap_or_else(|| panic!("missing mock response for {expected:?} request"));
            assert_eq!(
                response.kind(),
                expected,
                "mock response kind mismatch: expected {expected:?}, got {:?}",
                response.kind()
            );
            response
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn request_json(
            &self,
            method: Method,
            path: &str,
            query: &[(&str, String)],
            body: Option<Value>,
        ) -> Result<Value, AElfError> {
            self.record(MockCallKind::Json, method, path, query, body);
            match self.pop_response(MockResponseKind::Json) {
                MockResponse::Json(result) => result,
                MockResponse::Text(_) => unreachable!("json response kind already validated"),
            }
        }

        async fn request_text(
            &self,
            method: Method,
            path: &str,
            query: &[(&str, String)],
            body: Option<Value>,
        ) -> Result<String, AElfError> {
            self.record(MockCallKind::Text, method, path, query, body);
            match self.pop_response(MockResponseKind::Text) {
                MockResponse::Text(result) => result,
                MockResponse::Json(_) => unreachable!("text response kind already validated"),
            }
        }
    }
}

#[cfg(all(test, feature = "native-http"))]
mod tests {
    use super::*;
    use base64::Engine;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn header_value<'a>(request: &'a wiremock::Request, name: &str) -> Option<&'a str> {
        request
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
    }

    #[tokio::test]
    async fn get_request_uses_accept_header_without_content_type() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_string("\"123\""))
            .mount(&server)
            .await;

        let provider = HttpProvider::new(ClientConfig::new(server.uri())).expect("provider");
        let response = provider
            .request_text(
                Method::GET,
                "api/blockChain/blockHeight",
                &[("includeTransactions", "true".to_owned())],
                None,
            )
            .await
            .expect("response");

        assert_eq!(response, "\"123\"");
        let requests = server.received_requests().await.expect("requests");
        let request = requests.first().expect("request");
        assert_eq!(request.method.as_str(), "GET");
        assert_eq!(request.url.path(), "/api/blockChain/blockHeight");
        assert_eq!(request.url.query(), Some("includeTransactions=true"));
        assert_eq!(header_value(request, "accept"), Some("application/json"));
        assert!(header_value(request, "content-type").is_none());
    }

    #[tokio::test]
    async fn post_request_includes_version_auth_and_custom_headers() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
            .mount(&server)
            .await;

        let config = ClientConfig::new(server.uri())
            .with_api_version("1.0")
            .with_basic_auth("open", "sesame")
            .with_header("x-sdk-test", "yes")
            .expect("header");
        let provider = HttpProvider::new(config).expect("provider");

        let response = provider
            .request_json(
                Method::POST,
                "api/blockChain/rawTransaction",
                &[("verbose", "true".to_owned())],
                Some(json!({ "From": "from-address" })),
            )
            .await
            .expect("response");

        assert_eq!(response, json!({ "ok": true }));
        let requests = server.received_requests().await.expect("requests");
        let request = requests.first().expect("request");
        let auth = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("open:sesame")
        );

        assert_eq!(request.method.as_str(), "POST");
        assert_eq!(request.url.path(), "/api/blockChain/rawTransaction");
        assert_eq!(request.url.query(), Some("verbose=true"));
        assert_eq!(
            header_value(request, "accept"),
            Some("application/json;v=1.0")
        );
        assert_eq!(
            header_value(request, "content-type"),
            Some("application/json;v=1.0")
        );
        assert_eq!(header_value(request, "authorization"), Some(auth.as_str()));
        assert_eq!(header_value(request, "x-sdk-test"), Some("yes"));
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body).expect("body"),
            json!({ "From": "from-address" })
        );
    }

    #[tokio::test]
    async fn request_error_uses_webapp_error_shape() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(400)
                    .insert_header("x-request-id", "req-123")
                    .set_body_json(json!({
                        "error": {
                            "code": "InvalidTransaction",
                            "message": "bad transaction",
                            "details": "signature mismatch"
                        }
                    })),
            )
            .mount(&server)
            .await;

        let provider = HttpProvider::new(ClientConfig::new(server.uri())).expect("provider");
        let error = provider
            .request_text(Method::POST, "api/blockChain/sendTransaction", &[], None)
            .await
            .expect_err("request should fail");
        let expected_endpoint = format!("{}/api/blockChain/sendTransaction", server.uri());

        match error {
            AElfError::Request(error) => {
                assert_eq!(error.message, "bad transaction");
                assert_eq!(error.request_id.as_deref(), Some("req-123"));
                assert_eq!(error.endpoint.as_deref(), Some(expected_endpoint.as_str()));
                assert_eq!(error.status, Some(400));
                assert_eq!(error.chain_code.as_deref(), Some("InvalidTransaction"));
                assert_eq!(error.details.as_deref(), Some("signature mismatch"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_error_falls_back_to_plain_text_message() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(503)
                    .insert_header("request-id", "req-plain")
                    .set_body_string("service unavailable"),
            )
            .mount(&server)
            .await;

        let provider = HttpProvider::new(ClientConfig::new(server.uri())).expect("provider");
        let error = provider
            .request_text(Method::GET, "api/net/peers", &[], None)
            .await
            .expect_err("request should fail");
        let expected_endpoint = format!("{}/api/net/peers", server.uri());

        match error {
            AElfError::Request(error) => {
                assert_eq!(
                    error.message,
                    "request failed with status 503 Service Unavailable: service unavailable"
                );
                assert_eq!(error.request_id.as_deref(), Some("req-plain"));
                assert_eq!(error.endpoint.as_deref(), Some(expected_endpoint.as_str()));
                assert_eq!(error.status, Some(503));
                assert_eq!(error.chain_code, None);
                assert_eq!(error.details, None);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_retries_server_errors_until_success() {
        let server = MockServer::start().await;
        let attempts = Arc::new(AtomicUsize::new(0));
        let responder_attempts = attempts.clone();

        Mock::given(any())
            .respond_with(move |_request: &wiremock::Request| {
                let attempt = responder_attempts.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    ResponseTemplate::new(503).set_body_string("temporary outage")
                } else {
                    ResponseTemplate::new(200).set_body_string("\"ok\"")
                }
            })
            .mount(&server)
            .await;

        let provider = HttpProvider::new(
            ClientConfig::new(server.uri())
                .with_retry_policy(RetryPolicy::new(2, Duration::from_millis(1))),
        )
        .expect("provider");

        let response = provider
            .request_text(Method::GET, "api/blockChain/blockHeight", &[], None)
            .await
            .expect("response");

        assert_eq!(response, "\"ok\"");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        let requests = server.received_requests().await.expect("requests");
        assert_eq!(requests.len(), 3);
    }

    #[tokio::test]
    async fn request_does_not_retry_client_errors() {
        let server = MockServer::start().await;
        let attempts = Arc::new(AtomicUsize::new(0));
        let responder_attempts = attempts.clone();

        Mock::given(any())
            .respond_with(move |_request: &wiremock::Request| {
                responder_attempts.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(400).set_body_json(json!({
                    "error": {
                        "code": "BadRequest",
                        "message": "no retry",
                        "details": null
                    }
                }))
            })
            .mount(&server)
            .await;

        let provider = HttpProvider::new(
            ClientConfig::new(server.uri())
                .with_retry_policy(RetryPolicy::new(2, Duration::from_millis(1))),
        )
        .expect("provider");

        let error = provider
            .request_text(Method::GET, "api/net/peers", &[], None)
            .await
            .expect_err("request should fail");

        assert!(matches!(error, AElfError::Request(_)));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        let requests = server.received_requests().await.expect("requests");
        assert_eq!(requests.len(), 1);
    }
}
