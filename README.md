> ### ⚠️ Service notice
>
> **The RapidAPI listing that backs this SDK is temporarily unavailable while we work through a launch-day issue. Please check back in a few days.**

# tldrapi — Rust SDK for TLDRapi

Official async Rust client for the
[TLDRapi](https://unitycubed.dev/TLDRapi/) text-summarization API.

- **Async / tokio-native**
- **Pure Rust TLS** via `rustls` (no OpenSSL / libcurl at link time)
- **Typed errors** — match on the variant to branch on failure mode

## Install

```toml
[dependencies]
tldrapi = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Auth

Subscribe to the [TLDRapi listing on RapidAPI](https://rapidapi.com/)
and get your `X-RapidAPI-Key`.

## Usage

```rust
use tldrapi::{Client, ClientOptions, SummarizeOptions, Tier};

#[tokio::main]
async fn main() -> Result<(), tldrapi::Error> {
    let c = Client::new(ClientOptions {
        rapidapi_key: std::env::var("TLDRAPI_RAPIDAPI_KEY").unwrap(),
        ..Default::default()
    })?;

    let res = c.summarize(
        "Long text goes here.",
        SummarizeOptions {
            tier: Some(Tier::Quick),
            ..Default::default()
        },
    ).await?;

    println!("{}", res.summary);
    println!("cost=${:.6}  credits_remaining={:?}",
        res.usage.total_cost, res.credits.remaining);
    Ok(())
}
```

## Quality levels + pricing

Tiers: `Tier::Quick`, `Standard`, `Deep`, `Premium`, `Ultra`.
Higher → higher quality, larger chunks, more credits.

Credit cost scales with input size (v2.1):
`cost = 1 + Σ over chunks of (base × ceil(chunk_tokens / 1000))`.
Base costs and chunk caps are dynamic — fetch the current schedule
via `client.rates()` or `GET /rates`.

## Advanced quality controls (v-session129+)

Three orthogonal knobs on `SummarizeOptions`. Send zero (default
`standard`), OR set `tier` to any of 30 named presets, OR set 1-3
optional axes:

- `allow_downgrade: bool` — opt-in permissive paid-tier downgrade
- `optional_quality: Option<String>` — LLM: `quick|standard|deep|premium|ultra`
- `optional_extractive_lvl: Option<String>` — `minimal|brief|balanced|thorough|detailed|complete`
- `optional_strategy: Option<String>` — `contextual-compression|premium-single-shot|hierarchical-merge`

Preset + axes together → axes override, server sets `X-Quality-Warning`.

**30 named presets** = `{minimal|brief|balanced|thorough|detailed|complete}
-{quick|standard|deep|premium|ultra}` (e.g. `"thorough-standard"`).
The 5 short names (`quick`/`standard`/`deep`/`premium`/`ultra`) are the
SCORECARD-validated highlighted anchors.

```rust
let opts = SummarizeOptions {
    tier: Some(Tier::Premium),
    allow_downgrade: true,
    optional_extractive_lvl: Some("brief".into()),
    ..Default::default()
};
let r = client.summarize(text, opts).await?;
// r headers may carry x-quality-actual naming the served tier.
```

### Async submit + poll

Not yet exposed as native methods (falls back to `extra_headers`):

```rust
// Submit: set X-Async: true, capture the request_id from X-Paid-Request-Id header
// Poll: GET /paid/result/{id} — 200 returns summary, 202 = still pending, 410 = expired
```

Full native `submit_async` / `get_result` / `wait_for_result` methods
land in the next SDK release. Use raw HTTP via reqwest today if needed.

## Error handling

```rust
match c.summarize(text, opts).await {
    Ok(res) => println!("{}", res.summary),
    Err(tldrapi::Error::RateLimit { retry_after_seconds, .. }) => {
        tokio::time::sleep(std::time::Duration::from_secs(retry_after_seconds as u64)).await;
    }
    Err(tldrapi::Error::InsufficientCredits { response_body, .. }) => {
        let top_up = response_body.get("top_up_url").and_then(|v| v.as_str());
        eprintln!("out of credits — top up at {:?}", top_up);
    }
    Err(e) => eprintln!("API error: {e}"),
}
```

## Retries

5xx and transport errors auto-retry 3× with exponential backoff + jitter.
4xx (including 429) is **never** auto-retried. Configure via
`ClientOptions::retries`.

## Thread safety

`Client` is `Clone` and cheap to share across tasks — it wraps a
`reqwest::Client` which uses a connection pool internally.

## License

MIT — see [LICENSE](./LICENSE).
