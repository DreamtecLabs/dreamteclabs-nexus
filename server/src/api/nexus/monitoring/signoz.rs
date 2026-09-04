use anyhow::{Context, Error, bail};
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use url::Url;

use super::store::DEFAULT_SIGNOZ_URL;

async fn request(path: &str) -> Result<Value, Error> {
    let base = std::env::var("NEXUS_SIGNOZ_URL").unwrap_or_else(|_| DEFAULT_SIGNOZ_URL.to_string());
    let api_key =
        std::env::var("NEXUS_SIGNOZ_API_KEY").context("NEXUS_SIGNOZ_API_KEY is not configured")?;
    let base_url = Url::parse(&base).context("invalid NEXUS_SIGNOZ_URL")?;
    if base_url.scheme() != "http" {
        bail!("Nexus SigNoz integration currently requires an internal http URL");
    }
    let host = base_url
        .host_str()
        .context("SigNoz URL is missing a host")?;
    let port = base_url.port_or_known_default().unwrap_or(80);
    let stream = TcpStream::connect((host, port))
        .await
        .with_context(|| format!("unable to connect to SigNoz at {host}:{port}"))?;
    let io = TokioIo::new(stream);
    let (mut sender, connection) = http1::handshake(io)
        .await
        .context("unable to initialize SigNoz HTTP connection")?;
    tokio::spawn(async move {
        if let Err(err) = connection.await {
            log::debug!("SigNoz HTTP connection closed: {err}");
        }
    });

    let prefix = base_url.path().trim_end_matches('/');
    let uri = format!("{prefix}{path}");
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header("Host", host)
        .header("Accept", "application/json")
        .header("SIGNOZ-API-KEY", api_key)
        .body(Empty::<Bytes>::new())?;
    let response = sender
        .send_request(request)
        .await
        .context("SigNoz API request failed")?;
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .context("unable to read SigNoz API response")?
        .to_bytes();
    if !status.is_success() {
        let detail = String::from_utf8_lossy(&bytes);
        bail!("SigNoz API returned {status}: {detail}");
    }
    serde_json::from_slice(&bytes).context("SigNoz API returned invalid JSON")
}

fn rule_count(value: &Value) -> Option<usize> {
    if let Some(array) = value.as_array() {
        return Some(array.len());
    }
    for key in ["data", "rules"] {
        if let Some(array) = value.get(key).and_then(Value::as_array) {
            return Some(array.len());
        }
    }
    None
}

pub(super) async fn status() -> Value {
    let url = std::env::var("NEXUS_SIGNOZ_URL").unwrap_or_else(|_| DEFAULT_SIGNOZ_URL.to_string());
    if std::env::var("NEXUS_SIGNOZ_API_KEY").is_err() {
        return json!({
            "configured": false,
            "connected": false,
            "url": url,
            "error": "NEXUS_SIGNOZ_API_KEY is not configured"
        });
    }
    match request("/api/v1/rules").await {
        Ok(rules) => json!({
            "configured": true,
            "connected": true,
            "url": url,
            "rule_count": rule_count(&rules)
        }),
        Err(err) => json!({
            "configured": true,
            "connected": false,
            "url": url,
            "error": err.to_string()
        }),
    }
}
