//! The HTTP client every network feature shares, and a download helper
//! that refuses answers larger than the caller allows.

use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("\"{0}\" is not a web address")]
    InvalidUrl(String),
    #[error("only http and https addresses can be used")]
    UnsupportedScheme,
    #[error("could not reach the server: {0}")]
    Request(#[from] reqwest::Error),
    #[error("the server answered with error {0}")]
    Status(u16),
    #[error("the answer was larger than {limit} bytes")]
    TooLarge { limit: usize },
}

/// A client with Haven's user agent and timeouts. Cheap to clone; make one
/// and share it.
pub fn client() -> Result<reqwest::Client, HttpError> {
    // Fails only if a provider is already installed, which is fine.
    let _ = rustls::crypto::ring::default_provider().install_default();
    Ok(reqwest::Client::builder()
        .user_agent(concat!("Haven/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?)
}

/// Parses an address and accepts only http and https.
pub fn web_url(address: &str) -> Result<reqwest::Url, HttpError> {
    let url = reqwest::Url::parse(address.trim())
        .map_err(|_| HttpError::InvalidUrl(address.trim().to_owned()))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(HttpError::UnsupportedScheme),
    }
}

/// Downloads `url`, giving up once the body passes `limit` bytes.
pub async fn get_bytes(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, HttpError> {
    let mut response = client.get(web_url(url)?).send().await?;
    if !response.status().is_success() {
        return Err(HttpError::Status(response.status().as_u16()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > limit {
            return Err(HttpError::TooLarge { limit });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Sends a request with optional JSON body and reads the answer, giving
/// up once it passes `limit` bytes. Returns the status code with the body
/// whatever the status is, so callers can read an error's details.
pub async fn send(
    client: &reqwest::Client,
    url: &str,
    headers: &[(&str, String)],
    body: Option<String>,
    limit: usize,
) -> Result<(u16, Vec<u8>), HttpError> {
    let url = web_url(url)?;
    let mut request = match body {
        Some(body) => client.post(url).header("Content-Type", "application/json").body(body),
        None => client.get(url),
    };
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    let mut response = request.send().await?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > limit {
            return Err(HttpError::TooLarge { limit });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((status, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_web_addresses() {
        assert!(web_url(" https://example.com/a ").is_ok());
        assert!(web_url("http://127.0.0.1:7000/manifest.json").is_ok());
        assert!(matches!(web_url("file:///etc/passwd"), Err(HttpError::UnsupportedScheme)));
        assert!(matches!(web_url("not an address"), Err(HttpError::InvalidUrl(_))));
    }

    #[test]
    fn builds_a_client() {
        client().unwrap();
    }
}
