> ### ⚠️ Service notice
>
> **The RapidAPI listing that backs this SDK is temporarily unavailable while we work through a launch-day issue. Please check back in a few days.**
# tldrapi — Rust SDK for TLDRapi

Official async Rust client for the
[TLDRapi](https://tldrapi-summarizer.p.rapidapi.com/) text-summarization API.

- **Async / tokio-native**
- **Pure Rust TLS** via `rustls` (no OpenSSL / libcurl at link time)
- **Typed errors** — match on the variant to branch on failure mode

## Get your app's RapidAPI key

1. Sign in at [rapidapi.com](https://rapidapi.com)
2. Subscribe to the [TLDRapi Summarizer](https://rapidapi.com/thunderAPIs256/api/tldrapi-summarizer) listing (start with **BASIC** — free)
3. Go to **Console** (top nav) → **Applications** → **Add App** (or open an existing one)
4. In the App → **Authorizations** tab → click the copy icon next to your Authorization Key

That's the app's `X-RapidAPI-Key`. Pass it to the SDK constructor.

*Legacy path (deprecated): upper-right (?) → Legacy Developer Dashboard → Add New App → Authorization tab. The new Console path above is simpler.*

The Authorization Key field is the same value in both places — RapidAPI just labels it differently depending on which interface you use:

**New Console:**

![RapidAPI Console — Authorization Method labeled "RAPIDAPI"](https://raw.githubusercontent.com/unitycubed/tldrapi-docs/main/img/rapidapi-key-label-console.png)

**Legacy Developer Dashboard:**

![RapidAPI Legacy Developer Dashboard — Authorization Method labeled "API key"](https://raw.githubusercontent.com/unitycubed/tldrapi-docs/main/img/rapidapi-key-label-legacy.png)



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
