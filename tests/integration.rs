// Integration tests use wiremock to stand up a fake HTTP server so we
// can exercise the full request/response path without touching prod.
// Faster + deterministic + doesn't burn credits.

use tldrapi::{Client, ClientOptions, Error, SummarizeOptions, Tier};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_for(server: &MockServer) -> Client {
    Client::new(ClientOptions {
        rapidapi_key: "test-key".into(),
        base_url: Some(server.uri()),
        retries: Some(0), // deterministic — tests don't want retry noise
        ..Default::default()
    })
    .expect("client")
}

#[tokio::test]
async fn requires_key() {
    let err = Client::new(ClientOptions::default()).unwrap_err();
    assert!(matches!(err, Error::Config(_)));
}

#[tokio::test]
async fn summarize_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/summarize"))
        .and(header("x-rapidapi-key", "test-key"))
        .and(header("x-quality", "quick"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-request-id", "req-1")
                .insert_header("x-credits-remaining", "99")
                .set_body_json(serde_json::json!({
                    "summary": "hi.",
                    "session_id": "sess-1",
                    "usage": {
                        "input_tokens": 5,
                        "output_tokens": 3,
                        "total_cost": 0.001,
                        "model_used": "openrouter-llama-3.1-8b"
                    }
                })),
        )
        .mount(&server)
        .await;

    let c = client_for(&server);
    let res = c
        .summarize(
            "hello",
            SummarizeOptions {
                tier: Some(Tier::Quick),
                ..Default::default()
            },
        )
        .await
        .expect("summarize");

    assert_eq!(res.summary, "hi.");
    assert_eq!(res.session_id, "sess-1");
    assert_eq!(res.usage.input_tokens, 5);
    assert_eq!(res.usage.model_used, "openrouter-llama-3.1-8b");
    assert_eq!(res.request_id, "req-1");
    assert_eq!(res.credits.remaining.as_deref(), Some("99"));
}

#[tokio::test]
async fn summarize_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "invalid_key",
            "message": "bad key"
        })))
        .mount(&server)
        .await;

    let c = client_for(&server);
    let err = c.summarize("x", SummarizeOptions::default()).await.unwrap_err();
    match err {
        Error::Authentication { status, .. } => assert_eq!(status, 401),
        other => panic!("expected Authentication, got {other:?}"),
    }
}

#[tokio::test]
async fn summarize_rate_limit_carries_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "42")
                .set_body_json(serde_json::json!({
                    "error": "rate_limited",
                    "message": "slow down"
                })),
        )
        .mount(&server)
        .await;

    let c = client_for(&server);
    let err = c.summarize("x", SummarizeOptions::default()).await.unwrap_err();
    match err {
        Error::RateLimit { retry_after_seconds, .. } => assert_eq!(retry_after_seconds, 42),
        other => panic!("expected RateLimit, got {other:?}"),
    }
}

#[tokio::test]
async fn summarize_insufficient_credits_preserves_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(402).set_body_json(serde_json::json!({
            "error": "insufficient_credits",
            "top_up_url": "https://rapidapi.com/x"
        })))
        .mount(&server)
        .await;

    let c = client_for(&server);
    let err = c.summarize("x", SummarizeOptions::default()).await.unwrap_err();
    match err {
        Error::InsufficientCredits { response_body, .. } => {
            assert_eq!(
                response_body.get("top_up_url").and_then(|v| v.as_str()),
                Some("https://rapidapi.com/x")
            );
        }
        other => panic!("expected InsufficientCredits, got {other:?}"),
    }
}

#[tokio::test]
async fn summarize_language_not_supported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error_code": "language_not_supported",
            "message": "nope"
        })))
        .mount(&server)
        .await;

    let c = client_for(&server);
    let err = c.summarize("x", SummarizeOptions::default()).await.unwrap_err();
    assert!(matches!(err, Error::LanguageNotSupported { .. }));
}

#[tokio::test]
async fn rates_ok() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "credits_per_call": { "quick": 1, "standard": 5, "deep": 30, "premium": 110, "ultra": 400 },
            "credit_costs_updated_at": "2026-09-01",
            "history_url": "https://tldrapi.com/TLDRapi/rates/history"
        })))
        .mount(&server)
        .await;

    let c = client_for(&server);
    let r = c.rates().await.unwrap();
    // spec-nested tiers (populated on the convenience flat fields for
    // back-compat with pre-1.0 callers).
    assert_eq!(r.quick, 1);
    assert_eq!(r.ultra, 400);
    assert_eq!(r.credits_per_call.quick, 1);
    assert_eq!(r.credit_costs_updated_at.as_deref(), Some("2026-09-01"));
    assert_eq!(r.updated_at.as_deref(), Some("2026-09-01")); // deprecated alias
    assert_eq!(r.history_url, "https://tldrapi.com/TLDRapi/rates/history");
}

#[tokio::test]
async fn usage_ok() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "usage_count": 42,
            "successful_requests": 40,
            "failed_requests": 2,
            "plan": "free",
            "limits": { "per_minute": 3, "daily": 100 }
        })))
        .mount(&server)
        .await;

    let c = client_for(&server);
    let u = c.usage().await.unwrap();
    assert_eq!(u.usage_count, 42);
    assert_eq!(u.successful_requests, 40);
    assert_eq!(u.plan, "free");
    assert_eq!(u.limits.per_minute, Some(3));
}

#[tokio::test]
async fn empty_input_rejected() {
    let server = MockServer::start().await;
    let c = client_for(&server);
    let err = c.summarize("", SummarizeOptions::default()).await.unwrap_err();
    assert!(matches!(err, Error::InvalidRequest { .. }));
}
