//! Validation of feed URLs an admin may add: https only, no credentials, no private or internal addresses, and the
//! fetch is pinned to the address that was checked (no DNS-rebinding window, no redirects).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub const MAX_URL_LEN: usize = 300;
/// Largest feed body that is read.
pub const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum UrlError {
    TooLong,
    Invalid,
    NotHttps,
    Credentials,
    Port,
    InternalHost,
    PrivateAddress,
    Unresolvable,
}

fn private_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (64..128).contains(&o[1])) // carrier-grade NAT
        || (o[0] == 198 && (18..20).contains(&o[1])) // benchmarking
        || o[0] >= 240
}

fn private_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return private_v4(v4);
    }
    let s = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // unique local
        || (s[0] & 0xffc0) == 0xfe80 // link local
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
}

/// Is this a public, routable address?
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => !private_v4(v),
        IpAddr::V6(v) => !private_v6(v),
    }
}

/// Everything that can be checked without the network. Returns the parsed URL.
pub fn check_syntax(raw: &str) -> Result<reqwest::Url, UrlError> {
    let raw = raw.trim();
    if raw.len() > MAX_URL_LEN {
        return Err(UrlError::TooLong);
    }
    let url = reqwest::Url::parse(raw).map_err(|_| UrlError::Invalid)?;
    if url.scheme() != "https" {
        return Err(UrlError::NotHttps);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UrlError::Credentials);
    }
    if url.port().is_some_and(|p| p != 443) {
        return Err(UrlError::Port);
    }
    let host = url.host_str().ok_or(UrlError::Invalid)?.to_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".lan")
        || !host.contains('.') && host.parse::<IpAddr>().is_err() && !host.starts_with('[')
    {
        return Err(UrlError::InternalHost);
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        if !is_public(ip) {
            return Err(UrlError::PrivateAddress);
        }
    }
    Ok(url)
}

/// Resolve the host and require every address to be public. Returns the address to connect to.
pub async fn resolve_public(url: &reqwest::Url) -> Result<std::net::SocketAddr, UrlError> {
    let host = url.host_str().ok_or(UrlError::Invalid)?;
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, 443))
        .await
        .map_err(|_| UrlError::Unresolvable)?
        .collect();
    if addrs.is_empty() {
        return Err(UrlError::Unresolvable);
    }
    if addrs.iter().any(|a| !is_public(a.ip())) {
        return Err(UrlError::PrivateAddress);
    }
    Ok(addrs[0])
}

/// Fetch a feed: the connection is pinned to the checked address, redirects are refused, the time and the size are
/// capped. `None` on any problem.
pub async fn fetch_feed(raw: &str) -> Option<String> {
    let url = check_syntax(raw).ok()?;
    let addr = resolve_public(&url).await.ok()?;
    let host = url.host_str()?.to_string();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(8))
        .resolve(&host, addr)
        .user_agent("anpot-chapters/1.0")
        .build()
        .ok()?;
    let mut resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        // Only the host is logged: a feed address may carry a private token in its query.
        crate::http_log::failed(&format!("rss:{host}"), &host, resp).await;
        return None;
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_BODY) {
        return None;
    }
    crate::ops::health::ok(&format!("rss:{host}"));
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BODY {
            return None;
        }
    }
    String::from_utf8(body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_public_https_urls_pass() {
        assert!(check_syntax("https://example.com/feed.xml").is_ok());
        assert!(check_syntax("https://example.com:443/feed").is_ok());
        assert_eq!(
            check_syntax("http://example.com/feed"),
            Err(UrlError::NotHttps)
        );
        assert_eq!(
            check_syntax("ftp://example.com/feed"),
            Err(UrlError::NotHttps)
        );
        assert_eq!(
            check_syntax("https://user:pw@example.com/"),
            Err(UrlError::Credentials)
        );
        assert_eq!(
            check_syntax("https://example.com:8443/"),
            Err(UrlError::Port)
        );
        assert_eq!(check_syntax("not a url"), Err(UrlError::Invalid));
        assert_eq!(
            check_syntax(&format!("https://example.com/{}", "a".repeat(400))),
            Err(UrlError::TooLong)
        );
    }

    #[test]
    fn internal_hosts_and_private_ip_literals_are_refused() {
        for bad in [
            "https://localhost/x",
            "https://printer.local/x",
            "https://db.internal/x",
            "https://intranet/x",
        ] {
            assert_eq!(check_syntax(bad), Err(UrlError::InternalHost), "{bad}");
        }
        for bad in [
            "https://127.0.0.1/x",
            "https://10.0.0.5/x",
            "https://192.168.1.1/x",
            "https://172.16.3.4/x",
            "https://169.254.169.254/latest/meta-data",
            "https://100.64.0.1/x",
            "https://[::1]/x",
            "https://[fd00::1]/x",
            "https://[fe80::1]/x",
            "https://[::ffff:10.0.0.1]/x",
            "https://0.0.0.0/x",
        ] {
            assert_eq!(check_syntax(bad), Err(UrlError::PrivateAddress), "{bad}");
        }
        assert!(
            check_syntax("https://8.8.8.8/x").is_ok(),
            "a public literal is fine"
        );
    }

    #[test]
    fn address_classes() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "172.31.0.1",
            "169.254.1.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "fc00::1",
            "fe80::2",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public(ip.parse().expect("ip")), "{ip}");
        }
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "2606:4700:4700::1111",
        ] {
            assert!(is_public(ip.parse().expect("ip")), "{ip}");
        }
    }

    #[tokio::test]
    async fn names_that_resolve_to_loopback_are_refused() {
        let url = reqwest::Url::parse("https://localhost.localdomain/x").expect("url");
        // Whatever the resolver says, loopback must never be accepted.
        match resolve_public(&url).await {
            Ok(addr) => assert!(is_public(addr.ip())),
            Err(e) => assert!(matches!(
                e,
                UrlError::PrivateAddress | UrlError::Unresolvable
            )),
        }
    }
}
