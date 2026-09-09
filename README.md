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

Released under the MIT License — see [LICENSE](LICENSE).

Copyright (c) 2026 Ehren Biglari / Unity Cubed.
