use bytes::BytesMut;
use futures_util::StreamExt;
use reqwest::{StatusCode, header::LOCATION};
use scraper::{Html, Selector};
use std::{net::IpAddr, sync::Arc, time::Duration};
use url::Url;

use crate::error::AppError;

const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

#[async_trait::async_trait]
pub trait Resolver: Send + Sync {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, AppError>;
}

pub struct SystemResolver;

#[async_trait::async_trait]
impl Resolver for SystemResolver {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, AppError> {
        Ok(tokio::net::lookup_host((host, port))
            .await?
            .map(|address| address.ip())
            .collect())
    }
}

pub async fn fetch_live(
    url: &str,
    resolver: Arc<dyn Resolver>,
) -> Result<(Url, Vec<u8>, String), AppError> {
    let mut current = Url::parse(url)?;
    for redirect_count in 0..=MAX_REDIRECTS {
        validate_scheme(&current)?;
        let (host, sockets) = resolve_destination(&current, resolver.as_ref()).await?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .resolve_to_addrs(&host, &sockets)
            .build()?;
        let response = client.get(current.clone()).send().await?;
        if response.status().is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err(AppError::validation(
                    "too_many_redirects",
                    "live fetch exceeded redirect limit",
                ));
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| {
                    AppError::validation("invalid_redirect", "redirect has no valid Location")
                })?;
            current = current.join(location)?;
            continue;
        }
        if !response.status().is_success() {
            let reason = match response.status() {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "blocked_access",
                _ => "live_fetch_failed",
            };
            return Err(AppError::Client {
                status: axum::http::StatusCode::BAD_GATEWAY,
                code: reason,
                message: format!("live fetch returned HTTP {}", response.status()),
                retryable: response.status().is_server_error(),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(AppError::validation(
                "too_large",
                "live response exceeds size limit",
            ));
        }
        let mut bytes = BytesMut::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(AppError::validation(
                    "too_large",
                    "live response exceeds size limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let text = extract_main_text(&bytes);
        return Ok((current, bytes.freeze().to_vec(), text));
    }
    unreachable!("redirect loop returns on every path")
}

pub(crate) async fn resolve_destination(
    url: &Url,
    resolver: &dyn Resolver,
) -> Result<(String, Vec<std::net::SocketAddr>), AppError> {
    validate_scheme(url)?;
    let host = url
        .host_str()
        .ok_or_else(|| AppError::validation("invalid_url", "URL has no host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| AppError::validation("invalid_url", "URL has no port"))?;
    let addresses = resolver.resolve(host, port).await?;
    if addresses.is_empty() || addresses.iter().any(|address| !is_public(*address)) {
        return Err(AppError::forbidden(
            "ssrf_denied",
            "destination resolved to a denied address",
        ));
    }
    let sockets = addresses
        .into_iter()
        .map(|address| (address, port).into())
        .collect();
    Ok((host.to_owned(), sockets))
}

fn validate_scheme(url: &Url) -> Result<(), AppError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::validation(
            "invalid_url_scheme",
            "only http and https URLs are allowed",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::validation(
            "invalid_url",
            "URL credentials are not allowed",
        ));
    }
    Ok(())
}

pub fn is_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.octets()[0] == 0)
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public(IpAddr::V4(mapped));
            }
            !(ip.is_loopback()
                || ip.is_multicast()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}

fn extract_main_text(bytes: &[u8]) -> String {
    let html = String::from_utf8_lossy(bytes);
    let document = Html::parse_document(&html);
    for selector_text in ["main", "article", "body"] {
        let selector = Selector::parse(selector_text).expect("static selectors are valid");
        if let Some(element) = document.select(&selector).next() {
            let text = element.text().collect::<Vec<_>>().join(" ");
            let normal = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !normal.is_empty() {
                return normal;
            }
        }
    }
    String::new()
}

pub fn canonicalize(input: &str) -> Result<String, AppError> {
    let mut url = Url::parse(input)?;
    validate_scheme(&url)?;
    url.set_fragment(None);
    let retained = url
        .query_pairs()
        .filter(|(name, _)| {
            !name.starts_with("utm_") && !matches!(name.as_ref(), "fbclid" | "gclid" | "ref")
        })
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.query_pairs_mut().clear().extend_pairs(retained);
    if url.query() == Some("") {
        url.set_query(None);
    }
    Ok(url.into())
}
