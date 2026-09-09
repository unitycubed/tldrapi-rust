//! # tldrapi — Official Rust SDK for the TLDRapi API
//!
//! Async client using `reqwest` (rustls). Auth at launch is RapidAPI-only:
//! subscribe to the TLDRapi listing on RapidAPI, get an `X-RapidAPI-Key`,
//! and pass it to [`Client::new`].
//!
//! ```ignore
//! use tldrapi::{Client, ClientOptions, SummarizeOptions, Tier};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), tldrapi::Error> {
//!     let c = Client::new(ClientOptions {
//!         rapidapi_key: std::env::var("TLDRAPI_RAPIDAPI_KEY").unwrap(),
//!         ..Default::default()
//!     })?;
//!     let res = c.summarize("Long text goes here.",
//!         SummarizeOptions { tier: Some(Tier::Quick), ..Default::default() }).await?;
//!     println!("{}", res.summary);
//!     Ok(())
//! }
//! ```
//!
//! ## Retries
//!
//! 5xx and transport errors auto-retry 3× with exponential backoff + jitter.
//! 4xx (including 429) is **never** auto-retried — that would burn credits
//! or worsen a throttle. Callers respect `Retry-After` themselves via
//! [`Error::RateLimit`] (see the `retry_after_seconds` field).

use std::collections::HashMap;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// SDK version — emitted in the User-Agent header.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// RapidAPI hostname for the TLDRapi listing. Override via
/// [`ClientOptions::rapidapi_host`] only if the listing is renamed.
pub const DEFAULT_RAPIDAPI_HOST: &str = "tldrapi-summarization.p.rapidapi.com";

/// Paid quality tiers. Free tier: leave [`SummarizeOptions::tier`] as `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Quick,
    Standard,
    Deep,
    Premium,
    Ultra,
}

impl Tier {
    fn as_str(self) -> &'static str {
        match self {
            Tier::Quick => "quick",
            Tier::Standard => "standard",
            Tier::Deep => "deep",
            Tier::Premium => "premium",
            Tier::Ultra => "ultra",
        }
    }
}

/// Typed error hierarchy. Callers can match on variants to branch on
/// failure mode without string-matching messages.
#[derive(Debug, Error)]
pub enum Error {
    #[error("authentication failed ({status}): {message}")]
    Authentication { status: u16, message: String, request_id: String, response_body: serde_json::Value },

    #[error("insufficient credits ({status}): {message}")]
    InsufficientCredits { status: u16, message: String, request_id: String, response_body: serde_json::Value },

    #[error("rate limited ({status}): {message} (retry_after={retry_after_seconds}s)")]
    RateLimit {
        status: u16,
        message: String,
        request_id: String,
        response_body: serde_json::Value,
        retry_after_seconds: u32,
    },

    #[error("language not supported: {message}")]
    LanguageNotSupported { status: u16, message: String, request_id: String, response_body: serde_json::Value },

    #[error("quality selection requires paid plan: {message}")]
    QualitySelectionRequiresPaidPlan { status: u16, message: String, request_id: String, response_body: serde_json::Value },

    #[error("invalid request ({status}): {message}")]
    InvalidRequest { status: u16, message: String, request_id: String, response_body: serde_json::Value },

    #[error("server error ({status}) after retries: {message}")]
    Server { status: u16, message: String, request_id: String, response_body: serde_json::Value },

    #[error("network error: {message}")]
    Network { message: String },

    #[error("request timed out")]
    Timeout,

    #[error("invalid client configuration: {0}")]
    Config(String),
}

impl Error {
    /// If this variant carries a `status`, return it. Otherwise 0.
    pub fn status(&self) -> u16 {
        match self {
            Error::Authentication { status, .. }
            | Error::InsufficientCredits { status, .. }
            | Error::RateLimit { status, .. }
            | Error::LanguageNotSupported { status, .. }
            | Error::QualitySelectionRequiresPaidPlan { status, .. }
            | Error::InvalidRequest { status, .. }
            | Error::Server { status, .. } => *status,
            _ => 0,
        }
    }
}

/// Configuration for [`Client::new`]. `rapidapi_key` is the only
/// required field; everything else has a sensible default.
#[derive(Debug, Clone, Default)]
pub struct ClientOptions {
    pub rapidapi_key: String,
    pub rapidapi_host: Option<String>,
    pub base_url: Option<String>,
    pub timeout: Option<Duration>,
    pub retries: Option<u32>,
    pub user_agent: Option<String>,
}

/// Per-call knobs for [`Client::summarize`].
#[derive(Debug, Clone, Default)]
pub struct SummarizeOptions {
    pub tier: Option<Tier>,
    pub session_id: Option<String>,
    pub model_alias: Option<String>,
    pub allow_overage: bool,
    pub extra_headers: HashMap<String, String>,
    pub timeout: Option<Duration>,
}

/// The `usage` sub-object from `/summarize` responses.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub total_cost: f64,
    #[serde(default)]
    pub model_used: String,
}

/// `X-Credits-*` response headers, wrapped for ergonomic access.
#[derive(Debug, Clone, Default)]
pub struct Credits {
    pub charged: Option<String>,
    pub remaining: Option<String>,
    pub tier: Option<String>,
}

/// The full result of a `/summarize` call.
#[derive(Debug, Clone)]
pub struct SummarizeResult {
    pub summary: String,
    pub session_id: String,
    pub usage: Usage,
    pub request_id: String,
    pub credits: Credits,
    /// Full server response body — for fields not modeled above.
    pub raw: serde_json::Value,
}

/// `/rates` response.
#[derive(Debug, Clone, Deserialize)]
pub struct Rates {
    #[serde(default = "default_quick")]
    pub quick: u32,
    #[serde(default = "default_standard")]
    pub standard: u32,
    #[serde(default = "default_deep")]
    pub deep: u32,
    #[serde(default = "default_premium")]
    pub premium: u32,
    #[serde(default = "default_ultra")]
    pub ultra: u32,
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_quick() -> u32 { 1 }
fn default_standard() -> u32 { 5 }
fn default_deep() -> u32 { 30 }
fn default_premium() -> u32 { 110 }
fn default_ultra() -> u32 { 400 }

/// `/usage` response.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UsageStats {
    #[serde(default)]
    pub period: String,
    #[serde(default)]
    pub calls: u32,
    #[serde(default)]
    pub credits_charged: u32,
    #[serde(default)]
    pub credits_remaining: u32,
}

/// The main entry point. Thread-safe (clone the `Client` cheaply — it
/// wraps `reqwest::Client` which shares a connection pool internally).
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    rapidapi_key: String,
    rapidapi_host: String,
    retries: u32,
    user_agent: String,
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_RETRIES: u32 = 3;
const RETRY_BASE_MS: u64 = 500;

impl Client {
    /// Build a new client. Returns `Error::Config` if `rapidapi_key` is empty.
    pub fn new(opts: ClientOptions) -> Result<Self, Error> {
        if opts.rapidapi_key.trim().is_empty() {
            return Err(Error::Config(
                "rapidapi_key is required (subscribe on RapidAPI to obtain one)".into(),
            ));
        }
        let host = opts.rapidapi_host.unwrap_or_else(|| DEFAULT_RAPIDAPI_HOST.to_string());
        let base_url = opts
            .base_url
            .unwrap_or_else(|| format!("https://{host}"))
            .trim_end_matches('/')
            .to_string();
        let timeout = opts.timeout.unwrap_or(DEFAULT_TIMEOUT);
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| Error::Config(format!("build reqwest client: {e}")))?;
        let user_agent = opts.user_agent.unwrap_or_else(|| format!("tldrapi-rust/{VERSION}"));
        Ok(Self {
            http,
            base_url,
            rapidapi_key: opts.rapidapi_key,
            rapidapi_host: host,
            retries: opts.retries.unwrap_or(DEFAULT_RETRIES),
            user_agent,
        })
    }

    /// POST `/summarize`. Returns the summary + usage + session info.
    pub async fn summarize(
        &self,
        input_text: &str,
        opts: SummarizeOptions,
    ) -> Result<SummarizeResult, Error> {
        if input_text.is_empty() {
            return Err(Error::InvalidRequest {
                status: 0,
                message: "input_text must be non-empty".into(),
                request_id: String::new(),
                response_body: serde_json::Value::Null,
            });
        }
        let mut body = serde_json::json!({ "input_text": input_text });
        if let Some(s) = &opts.session_id {
            body["session_id"] = serde_json::Value::String(s.clone());
        }
        if let Some(m) = &opts.model_alias {
            body["model_alias"] = serde_json::Value::String(m.clone());
        }

        let mut headers = self.base_headers(opts.tier)?;
        if opts.allow_overage {
            headers.insert("X-Allow-Overage", HeaderValue::from_static("true"));
        }
        for (k, v) in &opts.extra_headers {
            if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
                headers.insert(name, val);
            }
        }

        let (resp_body, resp_headers) = self
            .request(
                reqwest::Method::POST,
                "/summarize",
                Some(body),
                headers,
                opts.timeout,
            )
            .await?;

        let request_id = header_str(&resp_headers, "x-request-id");
        let credits = Credits {
            charged: header_opt(&resp_headers, "x-credits-charged"),
            remaining: header_opt(&resp_headers, "x-credits-remaining"),
            tier: header_opt(&resp_headers, "x-credits-tier"),
        };
        let summary = resp_body
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let session_id = resp_body
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let usage: Usage = resp_body
            .get("usage")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Ok(SummarizeResult {
            summary,
            session_id,
            usage,
            request_id,
            credits,
            raw: resp_body,
        })
    }

    /// GET `/rates` — credit-per-call pricing table.
    pub async fn rates(&self) -> Result<Rates, Error> {
        let (body, _) = self
            .request(reqwest::Method::GET, "/rates", None, self.base_headers(None)?, None)
            .await?;
        serde_json::from_value(body).map_err(|e| Error::Server {
            status: 200,
            message: format!("unexpected /rates response: {e}"),
            request_id: String::new(),
            response_body: serde_json::Value::Null,
        })
    }

    /// GET `/usage` — customer's aggregate usage stats.
    pub async fn usage(&self) -> Result<UsageStats, Error> {
        let (body, _) = self
            .request(reqwest::Method::GET, "/usage", None, self.base_headers(None)?, None)
            .await?;
        serde_json::from_value(body).map_err(|e| Error::Server {
            status: 200,
            message: format!("unexpected /usage response: {e}"),
            request_id: String::new(),
            response_body: serde_json::Value::Null,
        })
    }

    fn base_headers(&self, tier: Option<Tier>) -> Result<HeaderMap, Error> {
        let mut h = HeaderMap::new();
        h.insert("Content-Type", HeaderValue::from_static("application/json"));
        h.insert(
            "User-Agent",
            HeaderValue::from_str(&self.user_agent).map_err(|e| Error::Config(e.to_string()))?,
        );
        h.insert(
            "X-RapidAPI-Key",
            HeaderValue::from_str(&self.rapidapi_key).map_err(|e| Error::Config(e.to_string()))?,
        );
        h.insert(
            "X-RapidAPI-Host",
            HeaderValue::from_str(&self.rapidapi_host).map_err(|e| Error::Config(e.to_string()))?,
        );
        if let Some(t) = tier {
            h.insert("X-Quality", HeaderValue::from_static("__replaced__"));
            // HeaderValue::from_static requires 'static; use from_str for the tier string.
            let v = HeaderValue::from_str(t.as_str()).map_err(|e| Error::Config(e.to_string()))?;
            h.insert("X-Quality", v);
        }
        Ok(h)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
        headers: HeaderMap,
        per_call_timeout: Option<Duration>,
    ) -> Result<(serde_json::Value, HeaderMap), Error> {
        let url = format!("{}{}", self.base_url, path);
        let mut last_transport_err: Option<String> = None;

        for attempt in 0..=self.retries {
            let mut req = self.http.request(method.clone(), &url).headers(headers.clone());
            if let Some(t) = per_call_timeout {
                req = req.timeout(t);
            }
            if let Some(b) = &body {
                req = req.json(b);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let resp_headers = resp.headers().clone();
                    let bytes = match resp.bytes().await {
                        Ok(b) => b,
                        Err(e) => {
                            last_transport_err = Some(format!("read body: {e}"));
                            if attempt < self.retries {
                                sleep_backoff(attempt).await;
                                continue;
                            }
                            return Err(Error::Network { message: last_transport_err.unwrap() });
                        }
                    };
                    if (200..300).contains(&status) {
                        let parsed: serde_json::Value = if bytes.is_empty() {
                            serde_json::Value::Null
                        } else {
                            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
                        };
                        return Ok((parsed, resp_headers));
                    }
                    if status >= 500 && attempt < self.retries {
                        sleep_backoff(attempt).await;
                        continue;
                    }
                    return Err(error_from_response(status, &bytes, &resp_headers));
                }
                Err(e) => {
                    if e.is_timeout() {
                        last_transport_err = Some("timeout".into());
                        if attempt < self.retries {
                            sleep_backoff(attempt).await;
                            continue;
                        }
                        return Err(Error::Timeout);
                    }
                    last_transport_err = Some(e.to_string());
                    if attempt < self.retries {
                        sleep_backoff(attempt).await;
                        continue;
                    }
                    return Err(Error::Network { message: e.to_string() });
                }
            }
        }
        Err(Error::Network {
            message: last_transport_err.unwrap_or_else(|| "unknown transport failure".into()),
        })
    }
}

async fn sleep_backoff(attempt: u32) {
    use std::time::Duration;
    let base = RETRY_BASE_MS.saturating_mul(1u64 << attempt.min(6));
    let jitter = (rand_u64() % 200) as u64;
    tokio::time::sleep(Duration::from_millis(base + jitter)).await;
}

// Tiny std-only RNG — we don't want the `rand` crate as a dep for
// nothing more than jittered backoff.
fn rand_u64() -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut h = DefaultHasher::new();
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut h);
    std::thread::current().id().hash(&mut h);
    h.finish()
}

fn header_str(h: &HeaderMap, k: &str) -> String {
    h.get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}

fn header_opt(h: &HeaderMap, k: &str) -> Option<String> {
    h.get(k).and_then(|v| v.to_str().ok()).map(|s| s.to_string())
}

fn error_from_response(status: u16, body_bytes: &[u8], headers: &HeaderMap) -> Error {
    let body: serde_json::Value =
        serde_json::from_slice(body_bytes).unwrap_or(serde_json::Value::Null);
    let request_id = header_str(headers, "x-request-id");
    let retry_after: u32 = headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let err_code = body
        .get("error_code")
        .or_else(|| body.get("error"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let message = extract_message(&body, body_bytes, status);

    match status {
        401 | 403 => Error::Authentication {
            status,
            message,
            request_id,
            response_body: body,
        },
        402 => Error::InsufficientCredits {
            status,
            message,
            request_id,
            response_body: body,
        },
        429 => Error::RateLimit {
            status,
            message,
            request_id,
            response_body: body,
            retry_after_seconds: retry_after,
        },
        400 => {
            if err_code == "language_not_supported" || err_code.contains("language") {
                Error::LanguageNotSupported {
                    status,
                    message,
                    request_id,
                    response_body: body,
                }
            } else if err_code == "quality_selection_requires_paid_plan" {
                Error::QualitySelectionRequiresPaidPlan {
                    status,
                    message,
                    request_id,
                    response_body: body,
                }
            } else {
                Error::InvalidRequest {
                    status,
                    message,
                    request_id,
                    response_body: body,
                }
            }
        }
        500..=599 => Error::Server {
            status,
            message,
            request_id,
            response_body: body,
        },
        _ => Error::InvalidRequest {
            status,
            message,
            request_id,
            response_body: body,
        },
    }
}

fn extract_message(body: &serde_json::Value, raw: &[u8], status: u16) -> String {
    for k in ["message", "detail", "error", "reason"] {
        if let Some(s) = body.get(k).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    let s = String::from_utf8_lossy(raw);
    let trimmed = s.trim();
    if !trimmed.is_empty() && !trimmed.starts_with('{') {
        return if trimmed.len() > 400 {
            trimmed[..400].to_string()
        } else {
            trimmed.to_string()
        };
    }
    format!("HTTP {status}")
}
