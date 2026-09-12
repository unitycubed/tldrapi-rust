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
pub const DEFAULT_RAPIDAPI_HOST: &str = "tldrapi-summarizer.p.rapidapi.com";

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

/// Optional per-call generation config for [`Client::summarize`].
/// Omitted fields fall through to server tier defaults; matches
/// `openapi.yaml` `SummarizeRequest.config`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SummarizeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(rename = "top_p", skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u32>,
}

impl SummarizeConfig {
    /// True iff every field is `None` — signals to callers/serializers
    /// that the block can be dropped entirely.
    pub fn is_empty(&self) -> bool {
        self.model_alias.is_none()
            && self.temperature.is_none()
            && self.top_p.is_none()
            && self.max_output_tokens.is_none()
            && self.max_input_tokens.is_none()
    }
}

/// Per-call knobs for [`Client::summarize`].
#[derive(Debug, Clone, Default)]
pub struct SummarizeOptions {
    pub tier: Option<Tier>,
    pub session_id: Option<String>,
    pub model_alias: Option<String>,
    /// Optional per-call generation overrides — see [`SummarizeConfig`].
    pub config: Option<SummarizeConfig>,
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

/// The `credits_per_call` sub-object of [`Rates`]. Split out so the raw
/// serde shape matches `openapi.yaml` `RatesResponse` exactly.
#[derive(Debug, Clone, Deserialize)]
pub struct CreditsPerCall {
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
}

impl Default for CreditsPerCall {
    fn default() -> Self {
        Self {
            quick: default_quick(),
            standard: default_standard(),
            deep: default_deep(),
            premium: default_premium(),
            ultra: default_ultra(),
        }
    }
}

/// `/rates` response. Shape aligned with `openapi.yaml` `RatesResponse`
/// as of session 57. Pre-1.0 releases parsed tier ints from top-level
/// keys and `updated_at`; the server has always nested tiers under
/// `credits_per_call` and named the timestamp `credit_costs_updated_at`.
/// The tier fields on this struct are convenience aliases populated from
/// `credits_per_call`, so existing `rates.quick` / `.standard` / etc.
/// access sites still compile. `updated_at` is kept as a deprecated
/// alias for `credit_costs_updated_at`.
#[derive(Debug, Clone, Deserialize)]
pub struct Rates {
    #[serde(default)]
    pub credits_per_call: CreditsPerCall,
    #[serde(default)]
    pub credit_costs_updated_at: Option<String>,
    #[serde(default)]
    pub history_url: String,

    // The following four fields are populated from `credits_per_call`
    // in the client after deserialization — they exist solely so pre-1.0
    // call sites like `rates.quick` keep compiling.
    #[serde(skip)]
    pub quick: u32,
    #[serde(skip)]
    pub standard: u32,
    #[serde(skip)]
    pub deep: u32,
    #[serde(skip)]
    pub premium: u32,
    #[serde(skip)]
    pub ultra: u32,
    /// Deprecated: use [`Rates::credit_costs_updated_at`].
    #[serde(skip)]
    pub updated_at: Option<String>,
}

fn default_quick() -> u32 { 1 }
fn default_standard() -> u32 { 5 }
fn default_deep() -> u32 { 30 }
fn default_premium() -> u32 { 110 }
fn default_ultra() -> u32 { 400 }

/// Plan-limit sub-object of [`UsageStats`] — populated when the server
/// reports them; `None` otherwise. Shape mirrors `openapi.yaml`
/// `UsageResponse.limits`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UsageLimits {
    #[serde(default)]
    pub per_minute: Option<u32>,
    #[serde(default)]
    pub daily: Option<u32>,
    #[serde(default)]
    pub credits: Option<u32>,
    #[serde(default)]
    pub concurrent: Option<u32>,
}

/// `/usage` response. Fields align with `openapi.yaml` `UsageResponse`.
/// Pre-1.0 releases parsed `period` / `calls` / `credits_charged` /
/// `credits_remaining` — none of which the server has ever emitted, so
/// those fields always returned zero.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct UsageStats {
    #[serde(default)]
    pub usage_count: u32,
    #[serde(default)]
    pub successful_requests: u32,
    #[serde(default)]
    pub failed_requests: u32,
    #[serde(default)]
    pub average_response_time_ms: f64,
    #[serde(default)]
    pub endpoints_used: serde_json::Value,
    #[serde(default)]
    pub error_rate: f64,
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub limits: UsageLimits,
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
        if let Some(cfg) = &opts.config {
            if !cfg.is_empty() {
                if let Ok(v) = serde_json::to_value(cfg) {
                    body["config"] = v;
                }
            }
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
        let mut rates: Rates = serde_json::from_value(body).map_err(|e| Error::Server {
            status: 200,
            message: format!("unexpected /rates response: {e}"),
            request_id: String::new(),
            response_body: serde_json::Value::Null,
        })?;
        // Populate the flat convenience fields from credits_per_call so
        // pre-1.0 call sites like `rates.quick` keep working. Also mirror
        // credit_costs_updated_at into the deprecated updated_at alias.
        rates.quick = rates.credits_per_call.quick;
        rates.standard = rates.credits_per_call.standard;
        rates.deep = rates.credits_per_call.deep;
        rates.premium = rates.credits_per_call.premium;
        rates.ultra = rates.credits_per_call.ultra;
        rates.updated_at = rates.credit_costs_updated_at.clone();
        Ok(rates)
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

    // ─── /convert/{json,html,md}-to-text (JSON body) ────────────────

    /// POST `/convert/json-to-text` — normalize a JSON string to plaintext.
    pub async fn convert_json_to_text(&self, text: &str) -> Result<ConvertResult, Error> {
        self.convert_text("/convert/json-to-text", text, false).await
    }

    /// POST `/convert/html-to-text` — strip HTML to clean plaintext.
    pub async fn convert_html_to_text(&self, text: &str) -> Result<ConvertResult, Error> {
        self.convert_text("/convert/html-to-text", text, false).await
    }

    /// POST `/convert/md-to-text` — render GitHub-flavored Markdown to plaintext.
    pub async fn convert_md_to_text(&self, text: &str) -> Result<ConvertResult, Error> {
        self.convert_text("/convert/md-to-text", text, false).await
    }

    async fn convert_text(&self, path: &str, text: &str, allow_overage: bool) -> Result<ConvertResult, Error> {
        if text.is_empty() {
            return Err(Error::InvalidRequest {
                status: 0,
                message: "text must be non-empty".into(),
                request_id: String::new(),
                response_body: serde_json::Value::Null,
            });
        }
        let mut headers = self.base_headers(None)?;
        if allow_overage {
            headers.insert("X-Allow-Overage", HeaderValue::from_static("true"));
        }
        let body = serde_json::json!({ "text": text });
        let (resp_body, resp_headers) = self
            .request(reqwest::Method::POST, path, Some(body), headers, None)
            .await?;
        Ok(build_convert_result(&resp_body, &resp_headers))
    }

    // ─── multipart /convert/* file endpoints ────────────────────────

    /// POST `/convert/doc-to-text` — extract plaintext from doc/docx/odt/rtf.
    pub async fn convert_doc_to_text(&self, bytes: Vec<u8>, filename: &str) -> Result<ConvertResult, Error> {
        self.convert_file("/convert/doc-to-text", bytes, filename, None).await
    }

    /// POST `/convert/doc-to-latex` — extract LaTeX from doc/docx/odt/rtf.
    pub async fn convert_doc_to_latex(&self, bytes: Vec<u8>, filename: &str) -> Result<ConvertResult, Error> {
        self.convert_file("/convert/doc-to-latex", bytes, filename, None).await
    }

    /// POST `/convert/docx-to-text` — deprecated alias of `convert_doc_to_text`.
    pub async fn convert_docx_to_text(&self, bytes: Vec<u8>, filename: &str) -> Result<ConvertResult, Error> {
        self.convert_file("/convert/docx-to-text", bytes, filename, None).await
    }

    async fn convert_file(
        &self,
        path: &str,
        bytes: Vec<u8>,
        filename: &str,
        backend: Option<&str>,
    ) -> Result<ConvertResult, Error> {
        let file_name = if filename.is_empty() { "upload".to_string() } else { filename.to_string() };
        let form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(bytes).file_name(file_name),
        );
        // For multipart, do NOT pass Content-Type — reqwest sets it with
        // boundary via multipart(). Strip it from the base headers.
        let mut headers = self.base_headers(None)?;
        headers.remove("Content-Type");
        if let Some(b) = backend {
            if !matches!(b, "auto" | "text" | "modal") {
                return Err(Error::InvalidRequest {
                    status: 0,
                    message: format!("backend must be one of auto,text,modal (got {b})"),
                    request_id: String::new(),
                    response_body: serde_json::Value::Null,
                });
            }
            headers.insert("X-PDF-Backend", HeaderValue::from_str(b).map_err(|e| Error::Config(e.to_string()))?);
        }
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http
            .request(reqwest::Method::POST, &url)
            .headers(headers)
            .multipart(form)
            .send()
            .await
            .map_err(|e| if e.is_timeout() { Error::Timeout } else { Error::Network { message: e.to_string() } })?;
        let status = resp.status().as_u16();
        let resp_headers = resp.headers().clone();
        let bytes = resp.bytes().await.map_err(|e| Error::Network { message: format!("read body: {e}") })?;
        if !(200..300).contains(&status) {
            return Err(error_from_response(status, &bytes, &resp_headers));
        }
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        Ok(build_convert_result(&parsed, &resp_headers))
    }

    /// POST `/convert/pdf-to-latex` — extract LaTeX from a PDF. Sync
    /// returns `Status == "done"`; async (>15 pages) returns `Status ==
    /// "queued"` + `job_id` + `poll_url`; poll [`Self::pdf_status`].
    pub async fn convert_pdf_to_latex(
        &self,
        bytes: Vec<u8>,
        filename: &str,
        backend: Option<&str>,
    ) -> Result<PdfConvertResult, Error> {
        let file_name = if filename.is_empty() { "upload.pdf".to_string() } else { filename.to_string() };
        let form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(bytes).file_name(file_name),
        );
        let mut headers = self.base_headers(None)?;
        headers.remove("Content-Type");
        if let Some(b) = backend {
            if !matches!(b, "auto" | "text" | "modal") {
                return Err(Error::InvalidRequest {
                    status: 0,
                    message: format!("backend must be one of auto,text,modal (got {b})"),
                    request_id: String::new(),
                    response_body: serde_json::Value::Null,
                });
            }
            headers.insert("X-PDF-Backend", HeaderValue::from_str(b).map_err(|e| Error::Config(e.to_string()))?);
        }
        let url = format!("{}/convert/pdf-to-latex", self.base_url);
        let resp = self.http
            .request(reqwest::Method::POST, &url)
            .headers(headers)
            .multipart(form)
            .send()
            .await
            .map_err(|e| if e.is_timeout() { Error::Timeout } else { Error::Network { message: e.to_string() } })?;
        let status = resp.status().as_u16();
        let resp_headers = resp.headers().clone();
        let bytes = resp.bytes().await.map_err(|e| Error::Network { message: format!("read body: {e}") })?;
        if status == 202 {
            let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            return Ok(build_pdf_async(&parsed, &resp_headers));
        }
        if !(200..300).contains(&status) {
            return Err(error_from_response(status, &bytes, &resp_headers));
        }
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        Ok(build_pdf_sync(&parsed, &resp_headers))
    }

    /// GET `/convert/pdf-to-latex/status/:job_id` — poll an async PDF job.
    pub async fn pdf_status(&self, job_id: &str) -> Result<PdfConvertResult, Error> {
        if job_id.is_empty() {
            return Err(Error::InvalidRequest {
                status: 0,
                message: "job_id required".into(),
                request_id: String::new(),
                response_body: serde_json::Value::Null,
            });
        }
        let path = format!("/convert/pdf-to-latex/status/{}", urlencode(job_id));
        let (body, headers) = self
            .request(reqwest::Method::GET, &path, None, self.base_headers(None)?, None)
            .await?;
        let status_str = body.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status_str == "queued" || status_str == "running" {
            Ok(build_pdf_async(&body, &headers))
        } else {
            Ok(build_pdf_sync(&body, &headers))
        }
    }

    // ─── rates history + usage range ────────────────────────────────

    /// GET `/rates/history` — change history for tier credit costs.
    pub async fn rates_history(&self) -> Result<RatesHistory, Error> {
        let (body, _) = self
            .request(reqwest::Method::GET, "/rates/history", None, self.base_headers(None)?, None)
            .await?;
        Ok(build_rates_history(&body))
    }

    /// GET `/usage/range?from=&to=` — per-day usage between two YYYY-MM-DD dates.
    pub async fn usage_range(&self, from: &str, to: &str) -> Result<UsageRange, Error> {
        if from.is_empty() || to.is_empty() {
            return Err(Error::InvalidRequest {
                status: 0,
                message: "from and to required (YYYY-MM-DD)".into(),
                request_id: String::new(),
                response_body: serde_json::Value::Null,
            });
        }
        let path = format!("/usage/range?from={}&to={}", urlencode(from), urlencode(to));
        let (body, _) = self
            .request(reqwest::Method::GET, &path, None, self.base_headers(None)?, None)
            .await?;
        Ok(build_usage_range(&body))
    }

    // ─── custom prompts (Business/Enterprise) ───────────────────────

    /// POST `/custom-prompts/submit` — submit a custom voice prompt.
    pub async fn custom_prompt_submit(
        &self,
        voice_name: &str,
        instruction: &str,
        session_id: Option<&str>,
        allow_overage: bool,
    ) -> Result<CustomPromptResult, Error> {
        if voice_name.is_empty() || instruction.is_empty() {
            return Err(Error::InvalidRequest {
                status: 0,
                message: "voice_name and instruction required".into(),
                request_id: String::new(),
                response_body: serde_json::Value::Null,
            });
        }
        let mut body = serde_json::json!({
            "voice_name": voice_name,
            "instruction": instruction,
        });
        if let Some(s) = session_id {
            body["session_id"] = serde_json::Value::String(s.to_string());
        }
        let mut headers = self.base_headers(None)?;
        if allow_overage {
            headers.insert("X-Allow-Overage", HeaderValue::from_static("true"));
        }
        let (resp_body, _) = self
            .request(reqwest::Method::POST, "/custom-prompts/submit", Some(body), headers, None)
            .await?;
        Ok(build_custom_prompt_result(&resp_body))
    }

    /// POST `/custom-prompts/list` — list every custom prompt.
    pub async fn custom_prompts_list(&self, session_id: Option<&str>) -> Result<CustomPromptList, Error> {
        let mut body = serde_json::json!({});
        if let Some(s) = session_id {
            body["session_id"] = serde_json::Value::String(s.to_string());
        }
        let (resp_body, _) = self
            .request(reqwest::Method::POST, "/custom-prompts/list", Some(body), self.base_headers(None)?, None)
            .await?;
        Ok(build_custom_prompt_list(&resp_body))
    }

    /// GET `/custom-prompts/:id` — full detail of a single custom prompt.
    pub async fn custom_prompt_get(&self, prompt_id: &str) -> Result<CustomPromptDetail, Error> {
        if prompt_id.is_empty() {
            return Err(Error::InvalidRequest {
                status: 0,
                message: "prompt_id required".into(),
                request_id: String::new(),
                response_body: serde_json::Value::Null,
            });
        }
        let path = format!("/custom-prompts/{}", urlencode(prompt_id));
        let (body, _) = self
            .request(reqwest::Method::GET, &path, None, self.base_headers(None)?, None)
            .await?;
        Ok(build_custom_prompt_detail(&body))
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

/// Return of the text-body /convert endpoints.
#[derive(Debug, Clone, Default)]
pub struct ConvertResult {
    pub output: String,
    pub output_format: String,
    pub input_format: String,
    pub input_bytes: u32,
    pub output_chars: u32,
    pub elapsed_ms: u32,
    pub request_id: String,
    pub warnings: Vec<serde_json::Value>,
    pub raw: serde_json::Value,
}

/// Return of `/convert/pdf-to-latex`. Sync fields are filled on HTTP 200
/// (Status == "done"); async fields (`job_id` / `poll_url` / `status`)
/// on HTTP 202 (Status == "queued"). Poll [`Client::pdf_status`] for
/// async jobs.
#[derive(Debug, Clone, Default)]
pub struct PdfConvertResult {
    pub output: String,
    pub output_format: String,
    pub input_format: String,
    pub input_bytes: u32,
    pub pages: u32,
    pub elapsed_ms: u32,
    pub backend: String,
    pub request_id: String,
    pub warnings: Vec<serde_json::Value>,
    pub job_id: String,
    pub status: String,
    pub poll_url: String,
    pub estimated_seconds: u32,
    pub raw: serde_json::Value,
}

/// One row of [`RatesHistory::history`].
#[derive(Debug, Clone, Default)]
pub struct RatesHistoryChange {
    pub changed_at: String,
    pub tier: String,
    pub credits_before: u32,
    pub credits_after: u32,
    pub reason: String,
    pub operator: String,
}

/// Return of [`Client::rates_history`].
#[derive(Debug, Clone, Default)]
pub struct RatesHistory {
    pub history: Vec<RatesHistoryChange>,
    pub range_days: u32,
    pub total_changes: u32,
    pub raw: serde_json::Value,
}

/// One day inside a [`UsageRange`].
#[derive(Debug, Clone, Default)]
pub struct UsageRangeDay {
    pub date: String,
    pub credits_used: u32,
    pub call_count: u32,
}

/// Return of [`Client::usage_range`].
#[derive(Debug, Clone, Default)]
pub struct UsageRange {
    pub from: String,
    pub to: String,
    pub credits_used: u32,
    pub daily: Vec<UsageRangeDay>,
    pub raw: serde_json::Value,
}

/// Return of [`Client::custom_prompt_submit`]. `voice_reference` is
/// `None` on rejection; on approval it's the string to pass as `voice=`
/// on future summarize calls.
#[derive(Debug, Clone, Default)]
pub struct CustomPromptResult {
    pub id: String,
    pub voice_name: String,
    pub status: String,
    pub approved: bool,
    pub voice_reference: Option<String>,
    pub rejection_reason: Option<String>,
    pub updated_in_place: bool,
    pub superseded_ids: Vec<String>,
    pub raw: serde_json::Value,
}

/// One row from [`Client::custom_prompts_list`]. Full instruction text
/// is NOT included — call [`Client::custom_prompt_get`] to see it.
#[derive(Debug, Clone, Default)]
pub struct CustomPromptSummary {
    pub id: String,
    pub voice_name: String,
    pub status: String,
    pub voice_reference: Option<String>,
    pub approved_alias: Option<String>,
    pub rejection_reason: Option<String>,
    pub submitted_at: String,
    pub reviewed_at: String,
}

/// Return of [`Client::custom_prompts_list`].
#[derive(Debug, Clone, Default)]
pub struct CustomPromptList {
    pub customer_id: String,
    pub custom_prompts: Vec<CustomPromptSummary>,
    pub raw: serde_json::Value,
}

/// Return of [`Client::custom_prompt_get`].
#[derive(Debug, Clone, Default)]
pub struct CustomPromptDetail {
    pub id: String,
    pub customer_id: String,
    pub voice_name: String,
    pub instruction: String,
    pub status: String,
    pub voice_reference: Option<String>,
    pub approved_alias: Option<String>,
    pub rejection_reason: Option<String>,
    pub judge_verdict_json: String,
    pub submitted_at: String,
    pub reviewed_at: String,
    pub raw: serde_json::Value,
}

// ─── response builders ─────────────────────────────────────────────

fn s(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}
fn s_opt(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string())
}
fn u(v: &serde_json::Value, k: &str) -> u32 {
    v.get(k).and_then(|x| x.as_u64()).map(|n| n as u32).unwrap_or(0)
}

fn first_non_empty(a: &str, b: &str) -> String {
    if !a.is_empty() { a.to_string() } else { b.to_string() }
}

fn build_convert_result(v: &serde_json::Value, h: &HeaderMap) -> ConvertResult {
    ConvertResult {
        output: s(v, "output"),
        output_format: s(v, "output_format"),
        input_format: s(v, "input_format"),
        input_bytes: u(v, "input_bytes"),
        output_chars: u(v, "output_chars"),
        elapsed_ms: u(v, "elapsed_ms"),
        request_id: first_non_empty(&s(v, "request_id"), &header_str(h, "x-request-id")),
        warnings: v.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default(),
        raw: v.clone(),
    }
}

fn build_pdf_sync(v: &serde_json::Value, h: &HeaderMap) -> PdfConvertResult {
    PdfConvertResult {
        output: s(v, "output"),
        output_format: s(v, "output_format"),
        input_format: first_non_empty(&s(v, "input_format"), "pdf"),
        input_bytes: u(v, "input_bytes"),
        pages: u(v, "pages"),
        elapsed_ms: u(v, "elapsed_ms"),
        backend: s(v, "backend"),
        request_id: first_non_empty(&s(v, "request_id"), &header_str(h, "x-request-id")),
        warnings: v.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default(),
        job_id: s(v, "job_id"),
        status: first_non_empty(&s(v, "status"), "done"),
        poll_url: s(v, "poll_url"),
        estimated_seconds: u(v, "estimated_seconds"),
        raw: v.clone(),
    }
}

fn build_pdf_async(v: &serde_json::Value, h: &HeaderMap) -> PdfConvertResult {
    PdfConvertResult {
        input_format: "pdf".into(),
        input_bytes: u(v, "input_bytes"),
        pages: u(v, "pages"),
        backend: s(v, "backend"),
        request_id: first_non_empty(&s(v, "request_id"), &header_str(h, "x-request-id")),
        warnings: v.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default(),
        job_id: s(v, "job_id"),
        status: first_non_empty(&s(v, "status"), "queued"),
        poll_url: s(v, "poll_url"),
        estimated_seconds: u(v, "estimated_seconds"),
        raw: v.clone(),
        ..Default::default()
    }
}

fn build_rates_history(v: &serde_json::Value) -> RatesHistory {
    let empty = vec![];
    let raw = v.get("history").and_then(|x| x.as_array()).unwrap_or(&empty);
    let history: Vec<RatesHistoryChange> = raw.iter().map(|r| RatesHistoryChange {
        changed_at: s(r, "changed_at"),
        tier: s(r, "tier"),
        credits_before: u(r, "credits_before"),
        credits_after: u(r, "credits_after"),
        reason: s(r, "reason"),
        operator: s(r, "operator"),
    }).collect();
    let total = u(v, "total_changes");
    let total = if total == 0 { history.len() as u32 } else { total };
    RatesHistory {
        history,
        range_days: v.get("range_days").and_then(|x| x.as_u64()).map(|n| n as u32).unwrap_or(30),
        total_changes: total,
        raw: v.clone(),
    }
}

fn build_usage_range(v: &serde_json::Value) -> UsageRange {
    let empty = vec![];
    let raw = v.get("daily").and_then(|x| x.as_array()).unwrap_or(&empty);
    let daily: Vec<UsageRangeDay> = raw.iter().map(|r| UsageRangeDay {
        date: s(r, "date"),
        credits_used: u(r, "credits_used"),
        call_count: u(r, "call_count"),
    }).collect();
    UsageRange {
        from: s(v, "from"),
        to: s(v, "to"),
        credits_used: u(v, "credits_used"),
        daily,
        raw: v.clone(),
    }
}

fn build_custom_prompt_result(v: &serde_json::Value) -> CustomPromptResult {
    let status = s(v, "status");
    let approved_flag = v.get("approved").and_then(|x| x.as_bool()).unwrap_or(false);
    let superseded: Vec<String> = v.get("superseded_ids")
        .and_then(|x| x.as_array())
        .map(|arr| arr.iter().filter_map(|e| e.as_str().map(String::from)).collect())
        .unwrap_or_default();
    CustomPromptResult {
        id: s(v, "id"),
        voice_name: s(v, "voice_name"),
        approved: approved_flag || status == "approved",
        status,
        voice_reference: s_opt(v, "voice_reference"),
        rejection_reason: s_opt(v, "rejection_reason"),
        updated_in_place: v.get("updated_in_place").and_then(|x| x.as_bool()).unwrap_or(false),
        superseded_ids: superseded,
        raw: v.clone(),
    }
}

fn build_custom_prompt_summary(v: &serde_json::Value) -> CustomPromptSummary {
    CustomPromptSummary {
        id: s(v, "id"),
        voice_name: s(v, "voice_name"),
        status: s(v, "status"),
        voice_reference: s_opt(v, "voice_reference"),
        approved_alias: s_opt(v, "approved_alias"),
        rejection_reason: s_opt(v, "rejection_reason"),
        submitted_at: s(v, "submitted_at"),
        reviewed_at: s(v, "reviewed_at"),
    }
}

fn build_custom_prompt_list(v: &serde_json::Value) -> CustomPromptList {
    let empty = vec![];
    let raw = v.get("custom_prompts").and_then(|x| x.as_array()).unwrap_or(&empty);
    let list: Vec<CustomPromptSummary> = raw.iter().map(build_custom_prompt_summary).collect();
    CustomPromptList {
        customer_id: s(v, "customer_id"),
        custom_prompts: list,
        raw: v.clone(),
    }
}

fn build_custom_prompt_detail(v: &serde_json::Value) -> CustomPromptDetail {
    CustomPromptDetail {
        id: s(v, "id"),
        customer_id: s(v, "customer_id"),
        voice_name: s(v, "voice_name"),
        instruction: s(v, "instruction"),
        status: s(v, "status"),
        voice_reference: s_opt(v, "voice_reference"),
        approved_alias: s_opt(v, "approved_alias"),
        rejection_reason: s_opt(v, "rejection_reason"),
        judge_verdict_json: s(v, "judge_verdict_json"),
        submitted_at: s(v, "submitted_at"),
        reviewed_at: s(v, "reviewed_at"),
        raw: v.clone(),
    }
}

/// Tiny percent-encoder for URL path/query segments. Uses the RFC-3986
/// unreserved set (A-Z a-z 0-9 - _ . ~), percent-encoding everything else.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
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
