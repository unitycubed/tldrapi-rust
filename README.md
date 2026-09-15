# tldrapi — Rust SDK for TLDRapi

Official async Rust client for [TLDRapi](https://tldrapi.com) — turn
any content into a clean summary in one API call.

- **Free tier** — 100 credits per month, no card, no trial expiry
- **20+ input formats** — text, HTML, Markdown, PDF (with OCR), .docx,
  .doc, .odt, .rtf, .epub, JSON, YAML, CSV, transcripts
- **5 quality tiers** — pick latency vs. depth per call
- **Custom voice styles** — 20+ built-in voices; paid tiers can define
  their own with plain-English instructions
- **Multi-provider routing** — automatic failover across Anthropic,
  OpenAI, Groq, Gemini, and OpenRouter
- **Refunds you don't have to ask for** — every summary is judge-scored
  and mis-summaries are auto-refunded
- **Async / tokio-native**, pure-Rust TLS via `rustls` (no OpenSSL at
  link time), typed error enum

## Install

```toml
[dependencies]
tldrapi = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Table of contents

- [Getting your free key](#getting-your-free-key)
- [Hello world](#hello-world)
- [Examples gallery](#examples-gallery)
  - [Summarize an article by URL](#summarize-an-article-by-url)
  - [Pin a session across many summaries](#pin-a-session-across-many-summaries)
  - [Batch summarize in parallel](#batch-summarize-in-parallel)
  - [Handle a rate-limit with backoff](#handle-a-rate-limit-with-backoff)
  - [Show live credit balance to your user](#show-live-credit-balance-to-your-user)
  - [Advanced quality controls — 3 axes, 30 named presets](#advanced-quality-controls)
- [Quality tiers](#quality-tiers)
- [Async submit + poll](#async-submit--poll)
- [Error handling](#error-handling)
- [Configuration + retries](#configuration--retries)
- [License](#license)

## Getting your free key

1. Sign in at [rapidapi.com](https://rapidapi.com)
2. Subscribe to the [TLDRapi Summarizer](https://rapidapi.com/thunderAPIs256/api/tldrapi-summarizer)
   listing — choose **BASIC (Free)**
3. Open the listing → **Console** → **Applications** → **Add App**
4. In the App → **Authorizations** tab → copy the Authorization Key

Pass it to `Client::new` as `rapidapi_key`. Everything on the free
tier works exactly like paid tiers — same endpoints, same response
shape, same SDK — just with a 100-credit monthly cap.

## Hello world

```rust
use tldrapi::{Client, ClientOptions, SummarizeOptions};

#[tokio::main]
async fn main() -> Result<(), tldrapi::Error> {
    let c = Client::new(ClientOptions {
        rapidapi_key: std::env::var("TLDRAPI_RAPIDAPI_KEY").unwrap(),
        ..Default::default()
    })?;

    let res = c.summarize(
        "Some long article body here...",
        SummarizeOptions::default(),
    ).await?;

    println!("{}", res.summary);
    println!("credits remaining: {:?}", res.credits.remaining);
    Ok(())
}
```

## Examples gallery

### Summarize an article by URL

TLDRapi accepts URLs directly — the server fetches, extracts main
content, strips nav/ads, and summarizes.

```rust
use tldrapi::{Tier, SummarizeOptions};

let r = c.summarize(
    "https://arxiv.org/abs/1706.03762",
    SummarizeOptions { tier: Some(Tier::Deep), ..Default::default() },
).await?;
println!("{}", r.summary);
```

Works with HTML pages, news sites, GitHub READMEs, blog posts, and
academic PDFs served over HTTP.

### Pin a session across many summaries

```rust
let r1 = c.summarize("Doc 1", SummarizeOptions::default()).await?;
let r2 = c.summarize("Doc 2", SummarizeOptions {
    session_id: Some(r1.session_id.clone()),
    ..Default::default()
}).await?;
let r3 = c.summarize("Doc 3", SummarizeOptions {
    session_id: Some(r1.session_id.clone()),
    ..Default::default()
}).await?;
```

Useful when you want consistent voice across a run — legal briefs,
book chapters, tickets in the same support thread.

### Batch summarize in parallel

```rust
use futures::future::join_all;

let handles = texts.into_iter().map(|t| {
    let c = c.clone();
    tokio::spawn(async move {
        c.summarize(&t, SummarizeOptions {
            tier: Some(Tier::Quick), ..Default::default()
        }).await
    })
});

let results = join_all(handles).await;
```

`Client` is `Clone` — sharing across tasks reuses the internal
connection pool. Free-tier is rate-limited to ~3 rps; paid tiers
are much higher.

### Handle a rate-limit with backoff

```rust
for attempt in 0..3 {
    match c.summarize(text, SummarizeOptions {
        tier: Some(Tier::Deep), ..Default::default()
    }).await {
        Ok(r)  => { println!("{}", r.summary); break; }
        Err(tldrapi::Error::RateLimit { retry_after_seconds, .. }) => {
            let secs = if retry_after_seconds > 0 { retry_after_seconds } else { 60 };
            tokio::time::sleep(std::time::Duration::from_secs(secs as u64)).await;
        }
        Err(e) => { eprintln!("giving up: {e}"); return Err(e); }
    }
}
```

### Show live credit balance to your user

```rust
let u = c.usage().await?;
println!("You have {} credits left ({})",
    u.credits_remaining, u.plan);

let r = c.summarize(text, SummarizeOptions::default()).await?;
println!("That call cost {:?} credits. Remaining: {:?}",
    r.credits.charged, r.credits.remaining);
```

### Advanced quality controls

Every summarize call has three orthogonal knobs. Send zero of them
(defaults are fine), a named preset via `tier`, or set 1-3 optional
axes, or combine — axes override the preset and the server returns
`X-Quality-Warning`.

**30 named presets.** `tier` can be one of the 5 canonical `Tier`
enum variants (`Quick`, `Standard`, `Deep`, `Premium`, `Ultra`) OR
one of 25 compound presets passed as a raw string
(`"thorough-quick"`, `"complete-premium"`, …).

**Three optional axis overrides** on `SummarizeOptions`:

- `optional_quality: Option<String>` — LLM tier
- `optional_extractive_lvl: Option<String>` — retention level
- `optional_strategy: Option<String>` — inference strategy

```rust
// named preset
let r = c.summarize(text, SummarizeOptions {
    tier: Some(Tier::from_str("thorough-quick")?),
    ..Default::default()
}).await?;

// preset + one axis override — axes win, warning header returned
let r = c.summarize(text, SummarizeOptions {
    tier: Some(Tier::Premium),
    optional_extractive_lvl: Some("brief".into()),
    ..Default::default()
}).await?;

// all three axes, no preset
let r = c.summarize(text, SummarizeOptions {
    optional_quality:       Some("ultra".into()),
    optional_extractive_lvl: Some("complete".into()),
    optional_strategy:       Some("premium-single-shot".into()),
    ..Default::default()
}).await?;

// permissive downgrade on paid-tier
let r = c.summarize(text, SummarizeOptions {
    tier: Some(Tier::Premium),
    allow_downgrade: true,
    ..Default::default()
}).await?;
```

## Quality tiers

| Tier      | Reads at once   | Best for                          |
|-----------|----------------:|-----------------------------------|
| quick     |     4K tokens   | Short texts, previews             |
| standard  |    16K tokens   | Default — most articles           |
| deep      |    32K tokens   | Longer content, deeper reasoning  |
| premium   |    64K tokens   | Substantial documents             |
| ultra     |   100K tokens   | Long-form / research-grade        |

Live rates at [/rates](https://tldrapi.com/rates) or `c.rates().await`.

### Paid-tier quality guarantees

Default = strict wait for the tier's canonical primary model. Opt into
permissive fallback with `allow_downgrade: true` — the worker walks
DOWN the ladder (premium → deep → standard → quick) and returns
whichever tier's primary is available. Response carries
`X-Quality-Actual` and `X-Original-Tier` when a downgrade happened,
and the credit-cost delta is automatically refunded.

## Async submit + poll

Native `submit_async` / `get_result` / `wait_for_result` methods
land in the next SDK release. Until then, use `extra_headers` to
opt in to the async pattern:

```rust
// Submit: send X-Async: true, read X-Paid-Request-Id from the response headers
// Poll:   GET /paid/result/{id}
//   200 → SummarizeResult in body
//   202 → still queued
//   410 → expired (past the 1h cache TTL)
```

Credits are deducted at submit time and refunded on failure exactly
like sync.

## Error handling

```rust
match c.summarize(text, SummarizeOptions::default()).await {
    Ok(res) => println!("{}", res.summary),
    Err(tldrapi::Error::RateLimit { retry_after_seconds, .. }) => {
        tokio::time::sleep(std::time::Duration::from_secs(
            retry_after_seconds as u64
        )).await;
    }
    Err(tldrapi::Error::InsufficientCredits { response_body, .. }) => {
        let top_up = response_body.get("top_up_url").and_then(|v| v.as_str());
        eprintln!("out of credits — top up at {:?}", top_up);
    }
    Err(tldrapi::Error::Authentication { .. }) => {
        eprintln!("bad RapidAPI key");
    }
    Err(e) => eprintln!("API error: {e}"),
}
```

All errors carry the response's `status_code`, `request_id` (attach
when reporting bugs), and `response_body` on their struct variants.

## Configuration + retries

```rust
let c = Client::new(ClientOptions {
    rapidapi_key:  "YOUR_KEY".into(),
    rapidapi_host: "tldrapi-summarizer.p.rapidapi.com".into(),
    timeout_secs:  60,
    retries:       3,        // -1 disables retries
    ..Default::default()
})?;
```

Automatic retries on 5xx and transport errors with exponential
backoff + jitter (3 attempts default). 4xx and 429 are **not**
retried — honor `Retry-After` yourself via `RateLimit.retry_after_seconds`.

## Thread safety

`Client` is `Clone` and cheap to share across tasks — it wraps a
`reqwest::Client` with an internal connection pool.

## License

MIT — see [LICENSE](./LICENSE).

Copyright (c) 2026 Ehren Biglari / Unity Cubed.
