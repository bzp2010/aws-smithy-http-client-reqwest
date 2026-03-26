#![warn(missing_docs)]
//! `reqwest`-backed HTTP client integration for Smithy runtimes.
//!
//! This crate provides [`ReqwestHttpClient`], an implementation of
//! [`aws_smithy_runtime_api::client::http::HttpClient`] backed by
//! [`reqwest::Client`].
//!
//! It is useful when you want to reuse an existing `reqwest` client configuration
//! such as custom headers, proxies, TLS settings, or connection pools with
//! Smithy-based clients.
//! 
//! This helps remove dependencies such as `rustls` and `aws-lc-rs` that are forced
//! onto the AWS SDK; you will need to build these yourself via reqwest.
//!
//! # Examples
//!
//! ```rust
//! use aws_config::BehaviorVersion;
//! use aws_smithy_http_client_reqwest::ReqwestHttpClient;
//!
//! let reqwest_client = reqwest::Client::builder()
//!     .user_agent("my-app/1.0")
//!     .build()?;
//!
//! let config = aws_config::defaults(BehaviorVersion::latest())
//!   .http_client(ReqwestHttpClient::new(reqwest_client))
//!   .load();
//! # Ok::<(), reqwest::Error>(())
//! ```
//! 
//! # Limitations
//! 
//! 1. Connect timeout is not supported

use std::time::Duration;

use aws_smithy_runtime_api::client::{
    http::{
        HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector,
    },
    orchestrator::{HttpRequest, HttpResponse},
    result::ConnectorError,
    runtime_components::RuntimeComponents,
};
use aws_smithy_types::body::SdkBody;
use http_body_util::BodyExt;

#[derive(Debug)]
/// A Smithy [`HttpClient`] implementation backed by [`reqwest::Client`].
///
/// This type lets Smithy-based SDK clients send requests through a preconfigured
/// `reqwest` client, allowing you to share connection pools and transport-level
/// settings across your application.
///
/// Use [`ReqwestHttpClient::new`] when you already have a customized
/// [`reqwest::Client`]. If you just need a default configuration, use
/// [`ReqwestHttpClient::default`].
pub struct ReqwestHttpClient {
    client: reqwest::Client,
}

impl ReqwestHttpClient {
    /// Creates a Smithy HTTP client from an existing [`reqwest::Client`].
    ///
    /// The provided `reqwest` client is cloned as needed when Smithy creates
    /// connectors, so it can be shared safely across multiple service clients.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use aws_smithy_http_client_reqwest::ReqwestHttpClient;
    ///
    /// let reqwest_client = reqwest::Client::new();
    /// let http_client = ReqwestHttpClient::new(reqwest_client);
    /// # let _ = http_client;
    /// ```
    #[must_use]
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self::new(reqwest::Client::new())
    }
}

impl HttpClient for ReqwestHttpClient {
    fn http_connector(
        &self,
        settings: &HttpConnectorSettings,
        _components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        SharedHttpConnector::new(ReqwestHttpConnector {
            client: self.client.clone(),
            settings: settings.clone(),
        })
    }
}

enum CustomConnectorError {
    ReqwestError(reqwest::Error),
    HttpError(aws_smithy_runtime_api::http::HttpError),
}

#[derive(Debug)]
struct ReqwestHttpConnector {
    client: reqwest::Client,
    settings: HttpConnectorSettings,
}

impl ReqwestHttpConnector {
    async fn convert_request(
        req: HttpRequest,
        timeout: Option<Duration>,
    ) -> Result<reqwest::Request, CustomConnectorError> {
        let req = req
            .try_into_http1x()
            .map_err(|err| CustomConnectorError::HttpError(err))?;
        let (parts, body) = req.into_parts();

        let mut req = reqwest::Request::new(
            parts.method.clone(),
            parts.uri.to_string().parse().expect("known valid"),
        );

        *req.headers_mut() = parts.headers;
        req.body_mut()
            .replace(reqwest::Body::wrap_stream(body.into_data_stream()));

        if let Some(timeout) = timeout {
            req.timeout_mut().replace(timeout);
        }

        Ok(req)
    }

    async fn convert_response(
        resp: reqwest::Response,
    ) -> Result<HttpResponse, CustomConnectorError> {
        let headers = resp.headers().clone();

        let mut resp = HttpResponse::new(
            aws_smithy_runtime_api::http::StatusCode::from(resp.status()),
            SdkBody::from(
                resp.bytes()
                    .await
                    .map_err(|err| CustomConnectorError::ReqwestError(err))?,
            ),
        );

        *resp.headers_mut() = aws_smithy_runtime_api::http::Headers::try_from(headers)
            .map_err(|err| CustomConnectorError::HttpError(err))?;

        Ok(resp)
    }
}

impl HttpConnector for ReqwestHttpConnector {
    fn call(&self, req: HttpRequest) -> HttpConnectorFuture {
        let client = self.client.clone();
        let timeout = self.settings.read_timeout();
        HttpConnectorFuture::new(async move {
            let req = Self::convert_request(req, timeout)
                .await
                .map_err(|err| match err {
                    CustomConnectorError::HttpError(err) => {
                        ConnectorError::user(Box::new(err)).never_connected()
                    }
                    CustomConnectorError::ReqwestError(err) => {
                        ConnectorError::other(Box::new(err), None).never_connected()
                    }
                })?;

            let resp = client
                .execute(req)
                .await
                .map_err(|err| ConnectorError::other(Box::new(err), None))?;

            let resp = Self::convert_response(resp)
                .await
                .map_err(|err| match err {
                    CustomConnectorError::HttpError(err) => ConnectorError::user(Box::new(err)),
                    CustomConnectorError::ReqwestError(err) => {
                        ConnectorError::other(Box::new(err), None)
                    }
                })?;

            Ok(resp)
        })
    }
}
