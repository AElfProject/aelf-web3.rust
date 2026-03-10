use http::{HeaderMap, HeaderName, HeaderValue};
use std::time::Duration;

/// HTTP basic authentication credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

/// Retry policy for transient HTTP transport failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_retries: usize,
    pub initial_backoff: Duration,
}

impl RetryPolicy {
    /// Creates a retry policy using exponential backoff with the provided initial delay.
    pub fn new(max_retries: usize, initial_backoff: Duration) -> Self {
        Self {
            max_retries,
            initial_backoff,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            initial_backoff: Duration::from_millis(200),
        }
    }
}

/// Configuration used to build an `AElfClient` or `HttpProvider`.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub endpoint: String,
    pub api_version: Option<String>,
    pub basic_auth: Option<BasicAuth>,
    pub timeout: Duration,
    pub retry_policy: RetryPolicy,
    pub headers: HeaderMap,
}

impl ClientConfig {
    /// Creates a client configuration with sensible defaults for public AElf nodes.
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            api_version: None,
            basic_auth: None,
            timeout: Duration::from_secs(15),
            retry_policy: RetryPolicy::default(),
            headers: HeaderMap::new(),
        }
    }

    /// Sets the versioned `application/json;v=...` media type used in requests.
    pub fn with_api_version(mut self, version: impl Into<String>) -> Self {
        self.api_version = Some(version.into());
        self
    }

    /// Configures HTTP basic authentication for node requests.
    pub fn with_basic_auth(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.basic_auth = Some(BasicAuth {
            username: username.into(),
            password: password.into(),
        });
        self
    }

    /// Overrides the HTTP timeout for each request attempt.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Overrides the retry policy used for transient transport failures.
    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Disables automatic retries.
    pub fn without_retries(mut self) -> Self {
        self.retry_policy = RetryPolicy::new(0, Duration::ZERO);
        self
    }

    /// Adds a custom HTTP header that will be attached to every request.
    pub fn with_header(
        mut self,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
    ) -> Result<Self, http::Error> {
        let name = HeaderName::from_bytes(name.as_ref().as_bytes())?;
        let value = HeaderValue::from_str(value.as_ref())?;
        self.headers.insert(name, value);
        Ok(self)
    }
}
