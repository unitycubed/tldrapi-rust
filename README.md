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

Useful when you want consistent voice across a run — chapters of the same book, articles in a series, tickets in the same support thread

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

**30 named presets** arranged as a 1D spectrum across the underlying 3D
quality space (LLM x retention x strategy). The 5 bolded rows are the
main anchors; each also accepts a short alias equal to its LLM tier
name (`quick` / `standard` / `deep` / `premium` / `ultra`).

| #  | Preset                | What it delivers                                                                                       |
|---:|-----------------------|--------------------------------------------------------------------------------------------------------|
|  1 | `minimal-quick`       | Cheapest and fastest. Headline-length blurb from a small chunk. Title-level takeaway.                  |
|  2 | **`brief-quick`**     | 3-sentence recap with the fastest LLM. Previews and low-latency feed cards.                            |
|  3 | `minimal-standard`    | Headline blurb with the mid-tier LLM's fluency; still very cheap.                                      |
|  4 | `balanced-quick`      | 3-5 sentences from the fast LLM; slightly deeper than `brief-quick`.                                   |
|  5 | `brief-standard`      | 3-sentence recap with smoother phrasing than `brief-quick`.                                            |
|  6 | **`balanced-standard`** | Balanced coverage without run-ons. The general default for most articles.                            |
|  7 | `thorough-quick`      | Paragraph-length from the fast LLM; retains the top 2-3 supporting facts.                              |
|  8 | `minimal-deep`        | Headline output with the deeper LLM's coherence; frugal way to buy fluency without length.             |
|  9 | `brief-deep`          | 3-sentence recap with deeper-model reasoning.                                                          |
| 10 | **`thorough-deep`**   | Preserves specific dates, names, secondary facts. Research papers, meeting transcripts, long articles. |
| 11 | `detailed-quick`      | Longer paragraph from the fast LLM; more supporting facts, still light on nuance.                      |
| 12 | `thorough-standard`   | Retains dates and names on Standard-class content; great for meeting-transcript recaps.                |
| 13 | `complete-quick`      | Maximum retention the Quick LLM can produce; nearing Standard breadth but Quick tone.                  |
| 14 | `balanced-deep`       | 4-6 sentences with deep-model narrative flow.                                                          |
| 15 | `detailed-standard`   | Full-paragraph, entity-preserving; approaches Deep on retention.                                       |
| 16 | `complete-standard`   | Maximum Standard retention; substantial output length.                                                 |
| 17 | `detailed-deep`       | Heavy retention with deep-model reasoning; picks up minor arguments.                                   |
| 18 | `complete-deep`       | Maximum Deep retention; edging into Premium coverage.                                                  |
| 19 | `minimal-premium`     | Very short output with premium-model tone; premium quality at bargain length.                          |
| 20 | `brief-premium`       | 3-sentence recap with high-fidelity entity handling.                                                   |
| 21 | **`detailed-premium`** | Entity preservation, edge cases, atmospheric detail. Substantial documents and long-form reports.     |
| 22 | `balanced-premium`    | Moderate-length premium coverage; smoother than Deep, more concise than `detailed-premium`.            |
| 23 | `thorough-premium`    | Heavy retention with premium reasoning.                                                                |
| 24 | `minimal-ultra`       | Single-shot on the full document, minimum output length. Ultra fidelity, tiny output.                  |
| 25 | `brief-ultra`         | Full-context single-shot, 3-sentence output. Ideal for research-grade preview blurbs.                  |
| 26 | **`complete-ultra`**  | Single-shot on the full document, maximum retention, no chunking artifacts. Book-length manuscripts, long-form technical documentation. |
| 27 | `complete-premium`    | Maximum Premium retention; almost every noteworthy fact.                                               |
| 28 | `balanced-ultra`      | Full-context single-shot, balanced-length output.                                                      |
| 29 | `thorough-ultra`      | Full-context, retains most secondary facts.                                                            |
| 30 | `detailed-ultra`      | Full-context, near-maximum retention; the top rung of the spectrum.                                    |


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

Full pricing detail (methodology, formula, dynamic-pricing audit trail): [tldrapi.com/pricing](https://tldrapi.com/pricing). Live rates via `/rates` or the SDK's `rates()` method.rates().await`.

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
