# aws-smithy-http-client-reqwest

`aws-smithy-http-client-reqwest` provides a `reqwest`-backed HTTP client for
Smithy-based runtimes, including the AWS SDK for Rust.

It is useful when you want to:

- reuse an existing `reqwest::Client`
- keep transport configuration in one place
- control which `reqwest` features and TLS stack your application uses

## Installation

```toml
[dependencies]
aws-smithy-http-client-reqwest = "0.1"
reqwest = "0.13"
aws-config = "1"
```

## Example

```rust
use aws_config::BehaviorVersion;
use aws_smithy_http_client_reqwest::ReqwestHttpClient;

let reqwest_client = reqwest::Client::builder()
    .user_agent("my-app/1.0")
    .build()?;

let config = aws_config::defaults(BehaviorVersion::latest())
    .http_client(ReqwestHttpClient::new(reqwest_client))
    .load();

# let _ = config;
# Ok::<(), reqwest::Error>(())
```

## What This Crate Does

`ReqwestHttpClient` implements Smithy's
`aws_smithy_runtime_api::client::http::HttpClient` trait and forwards requests
through a provided `reqwest::Client`.

That means you can configure `reqwest` yourself, then plug it into
Smithy-based clients instead of relying on the default transport implementation.

## Limitations

- Connect timeout is currently not supported.
