//! A safe download of a picture from an address a member typed (Round 11.2).
//!
//! The address is checked (HTTPS, port 443, a public host name, no user info, no IP literal), the name is resolved
//! by us, every resolved address must be public, the connection is pinned to the vetted address (so a second DNS
//! answer cannot redirect it) and every redirect is checked again from the start. The body is streamed and cut off
//! at the size limit. Only the host is ever logged, never the path or the query.

use std::{
    collections::HashMap,
    future::Future,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Most bytes accepted from the network.
pub const MAX_BODY: usize = 8 * 1024 * 1024;
/// The whole download, redirects included.
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(6);
pub const MAX_REDIRECTS: usize = 3;
/// Downloads a member may start per minute, and downloads running at the same time in the whole bot.
pub const PER_MINUTE: usize = 5;
pub const CONCURRENT: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Not a valid address, or not one we may fetch (http, other port, local name, IP literal, user info).
    Unsafe,
    TooLarge,
    TooManyRedirects,
    Failed,
}

/// One answer of the server.
pub struct Reply {
    pub status: u16,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

/// The network, swappable in tests.
pub trait Net: Send + Sync {
    fn resolve(&self, host: &str) -> impl Future<Output = Option<Vec<IpAddr>>> + Send;
    /// One GET, connected to `ip` (the vetted address) with no redirect following; the body is cut at `max`.
    fn get(
        &self,
        url: &reqwest::Url,
        ip: IpAddr,
        max: usize,
        timeout: Duration,
    ) -> impl Future<Output = Result<Reply, FetchError>> + Send;
}

/// Is this address one the public internet can have? Everything private, local, shared, reserved or special is no.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => public_v4(v),
        IpAddr::V6(v) => public_v6(v),
    }
}

fn public_v4(v: Ipv4Addr) -> bool {
    let o = v.octets();
    !(v.is_private()
        || v.is_loopback()
        || v.is_link_local()
        || v.is_broadcast()
        || v.is_unspecified()
        || v.is_multicast()
        || v.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (64..128).contains(&o[1])) // shared address space
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking
        || o[0] >= 240) // reserved
}

fn public_v6(v: Ipv6Addr) -> bool {
    if let Some(m) = v.to_ipv4_mapped() {
        return public_v4(m);
    }
    let s = v.segments();
    !(v.is_loopback()
        || v.is_unspecified()
        || v.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // unique local
        || (s[0] & 0xffc0) == 0xfe80 // link local
        || (s[0] == 0x64 && s[1] == 0xff9b) // NAT64: embeds an IPv4 address
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || s[0] == 0x2002) // 6to4: embeds an IPv4 address
}

/// The address a member typed, parsed and judged. HTTPS only, port 443, a public host name.
pub fn validate_url(input: &str) -> Result<reqwest::Url, FetchError> {
    let t = input.trim();
    if t.is_empty() || t.len() > 2000 || t.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(FetchError::Unsafe);
    }
    let url = reqwest::Url::parse(t).map_err(|_| FetchError::Unsafe)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|p| p != 443)
    {
        return Err(FetchError::Unsafe);
    }
    let host = url.host_str().ok_or(FetchError::Unsafe)?;
    if !crate::wiki::public_host(host) {
        return Err(FetchError::Unsafe);
    }
    Ok(url)
}

/// Download `input` and return the bytes, or why not. Every hop passes the same checks.
pub async fn fetch<N: Net>(net: &N, input: &str) -> Result<Vec<u8>, FetchError> {
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    let mut url = validate_url(input)?;
    for hop in 0..=MAX_REDIRECTS {
        let host = url.host_str().ok_or(FetchError::Unsafe)?.to_string();
        let addrs = net.resolve(&host).await.ok_or(FetchError::Failed)?;
        // One private answer among the public ones is enough to refuse: the name is not trustworthy.
        if addrs.is_empty() || addrs.iter().any(|a| !is_public_ip(*a)) {
            tracing::debug!(
                "refused a picture address: {host} does not resolve to public addresses only"
            );
            return Err(FetchError::Unsafe);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(FetchError::Failed);
        }
        let reply = net.get(&url, addrs[0], MAX_BODY, left).await?;
        match reply.status {
            301 | 302 | 303 | 307 | 308 => {
                if hop == MAX_REDIRECTS {
                    return Err(FetchError::TooManyRedirects);
                }
                let loc = reply.location.ok_or(FetchError::Failed)?;
                let next = url.join(&loc).map_err(|_| FetchError::Unsafe)?;
                url = validate_url(next.as_str())?;
            }
            200..=299 => return Ok(reply.body),
            _ => return Err(FetchError::Failed),
        }
    }
    Err(FetchError::TooManyRedirects)
}

/// The real network.
pub struct Live;

impl Net for Live {
    async fn resolve(&self, host: &str) -> Option<Vec<IpAddr>> {
        let addrs = tokio::net::lookup_host((host, 443)).await.ok()?;
        Some(addrs.map(|a| a.ip()).collect())
    }

    async fn get(
        &self,
        url: &reqwest::Url,
        ip: IpAddr,
        max: usize,
        timeout: Duration,
    ) -> Result<Reply, FetchError> {
        let host = url.host_str().ok_or(FetchError::Unsafe)?;
        // The connection goes to the vetted address whatever DNS says now; TLS still checks the host name.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .resolve(host, SocketAddr::new(ip, 443))
            .timeout(timeout)
            .user_agent("anpot-bot/1.0")
            .build()
            .map_err(|_| FetchError::Failed)?;
        let mut resp = client
            .get(url.clone())
            .header("Accept", "image/png,image/jpeg,image/webp,image/gif")
            .send()
            .await
            .map_err(|_| FetchError::Failed)?;
        let status = resp.status().as_u16();
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if (300..400).contains(&status) {
            return Ok(Reply {
                status,
                location,
                body: Vec::new(),
            });
        }
        if resp.content_length().is_some_and(|n| n as usize > max) {
            return Err(FetchError::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|_| FetchError::Failed)? {
            if body.len() + chunk.len() > max {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Reply {
            status,
            location,
            body,
        })
    }
}

fn recent() -> &'static Mutex<HashMap<u64, Vec<Instant>>> {
    static R: OnceLock<Mutex<HashMap<u64, Vec<Instant>>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn permits() -> &'static tokio::sync::Semaphore {
    static S: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    S.get_or_init(|| tokio::sync::Semaphore::new(CONCURRENT))
}

/// May this member start a download now? Five per minute each.
pub fn allow(user: u64) -> bool {
    allow_at(user, Instant::now())
}

fn allow_at(user: u64, now: Instant) -> bool {
    let Ok(mut m) = recent().lock() else {
        return false;
    };
    let list = m.entry(user).or_default();
    list.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
    if list.len() >= PER_MINUTE {
        return false;
    }
    list.push(now);
    true
}

/// A download under the global concurrency cap; `None` when the bot is already busy with enough of them.
pub async fn fetch_limited(input: &str) -> Option<Result<Vec<u8>, FetchError>> {
    let _permit = permits().try_acquire().ok()?;
    Some(fetch(&Live, input).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Names to addresses and URLs to answers; records which address each request was pinned to.
    struct Fake {
        dns: HashMap<&'static str, Vec<IpAddr>>,
        pages: HashMap<String, (u16, Option<&'static str>, Vec<u8>)>,
        pinned: Mutex<Vec<(String, IpAddr)>>,
    }

    impl Net for Fake {
        async fn resolve(&self, host: &str) -> Option<Vec<IpAddr>> {
            self.dns.get(host).cloned()
        }
        async fn get(
            &self,
            url: &reqwest::Url,
            ip: IpAddr,
            max: usize,
            _t: Duration,
        ) -> Result<Reply, FetchError> {
            self.pinned
                .lock()
                .unwrap()
                .push((url.host_str().unwrap().to_string(), ip));
            let (status, loc, body) = self.pages.get(url.as_str()).ok_or(FetchError::Failed)?;
            if body.len() > max {
                return Err(FetchError::TooLarge);
            }
            Ok(Reply {
                status: *status,
                location: loc.map(str::to_string),
                body: body.clone(),
            })
        }
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn net() -> Fake {
        let mut dns = HashMap::new();
        dns.insert("img.example.com", vec![ip("93.184.216.34")]);
        dns.insert("cdn.example.org", vec![ip("151.101.1.1")]);
        dns.insert("evil.example.net", vec![ip("10.0.0.5")]);
        dns.insert(
            "mixed.example.net",
            vec![ip("93.184.216.34"), ip("127.0.0.1")],
        );
        dns.insert("meta.example.net", vec![ip("169.254.169.254")]);
        dns.insert("v6.example.net", vec![ip("fd00::1")]);
        let mut pages = HashMap::new();
        pages.insert(
            "https://img.example.com/a.png".to_string(),
            (200, None, vec![1, 2, 3]),
        );
        pages.insert(
            "https://img.example.com/r".to_string(),
            (302, Some("https://cdn.example.org/b.png"), vec![]),
        );
        pages.insert(
            "https://cdn.example.org/b.png".to_string(),
            (200, None, vec![9]),
        );
        pages.insert(
            "https://img.example.com/to-private".to_string(),
            (302, Some("https://evil.example.net/x.png"), vec![]),
        );
        pages.insert(
            "https://img.example.com/to-http".to_string(),
            (302, Some("http://cdn.example.org/b.png"), vec![]),
        );
        pages.insert(
            "https://img.example.com/to-ip".to_string(),
            (302, Some("https://127.0.0.1/x.png"), vec![]),
        );
        pages.insert(
            "https://img.example.com/loop".to_string(),
            (302, Some("https://img.example.com/loop"), vec![]),
        );
        pages.insert(
            "https://img.example.com/big.png".to_string(),
            (200, None, vec![0; MAX_BODY + 1]),
        );
        pages.insert(
            "https://evil.example.net/x.png".to_string(),
            (200, None, vec![1]),
        );
        pages.insert(
            "https://mixed.example.net/x.png".to_string(),
            (200, None, vec![1]),
        );
        Fake {
            dns,
            pages,
            pinned: Mutex::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn a_valid_image_is_fetched_from_the_vetted_address() {
        let n = net();
        assert_eq!(
            fetch(&n, "https://img.example.com/a.png").await,
            Ok(vec![1, 2, 3])
        );
        assert_eq!(
            n.pinned.lock().unwrap()[0],
            ("img.example.com".to_string(), ip("93.184.216.34")),
            "the connection is pinned to the address we resolved"
        );
    }

    #[tokio::test]
    async fn a_redirect_to_another_public_host_is_followed() {
        assert_eq!(
            fetch(&net(), "https://img.example.com/r").await,
            Ok(vec![9])
        );
    }

    #[tokio::test]
    async fn a_redirect_to_a_private_address_an_http_url_or_an_ip_is_refused() {
        for p in ["to-private", "to-http", "to-ip"] {
            assert_eq!(
                fetch(&net(), &format!("https://img.example.com/{p}")).await,
                Err(FetchError::Unsafe),
                "{p}"
            );
        }
        assert_eq!(
            fetch(&net(), "https://img.example.com/loop").await,
            Err(FetchError::TooManyRedirects),
            "at most three redirects"
        );
    }

    #[tokio::test]
    async fn a_host_that_resolves_to_private_space_even_partly_is_refused() {
        for h in ["evil", "mixed", "meta", "v6"] {
            assert_eq!(
                fetch(&net(), &format!("https://{h}.example.net/x.png")).await,
                Err(FetchError::Unsafe),
                "{h}: a rebinding-style host never gets a request"
            );
        }
        assert!(net().pinned.lock().unwrap().is_empty());
    }

    #[test]
    fn only_https_on_443_with_a_public_name_and_no_user_info_passes() {
        for bad in [
            "http://img.example.com/a.png",
            "https://img.example.com:8443/a.png",
            "https://user:pw@img.example.com/a.png",
            "https://localhost/a.png",
            "https://127.0.0.1/a.png",
            "https://[::1]/a.png",
            "https://169.254.169.254/latest/meta-data",
            "https://printer.local/a.png",
            "https://db.internal/a.png",
            "https://intranet/a.png",
            "ftp://img.example.com/a.png",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "",
            "not a url",
        ] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
        assert!(validate_url("https://img.example.com/a.png?x=1").is_ok());
        assert!(validate_url("https://img.example.com:443/a.png").is_ok());
    }

    #[test]
    fn the_address_classes_are_judged() {
        for bad in [
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "127.0.0.1",
            "0.0.0.0",
            "169.254.169.254",
            "100.64.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "198.18.0.1",
            "240.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12::1",
            "ff02::1",
            "::ffff:10.0.0.1",
            "::ffff:127.0.0.1",
            "64:ff9b::7f00:1",
            "2002:7f00:1::1",
        ] {
            assert!(!is_public_ip(ip(bad)), "{bad}");
        }
        for good in [
            "93.184.216.34",
            "8.8.8.8",
            "2606:4700:4700::1111",
            "151.101.1.1",
        ] {
            assert!(is_public_ip(ip(good)), "{good}");
        }
    }

    #[tokio::test]
    async fn an_oversized_body_is_cut_off() {
        assert_eq!(
            fetch(&net(), "https://img.example.com/big.png").await,
            Err(FetchError::TooLarge)
        );
    }

    #[test]
    fn five_downloads_a_minute_per_member() {
        let t = Instant::now();
        for _ in 0..PER_MINUTE {
            assert!(allow_at(9_900_001, t));
        }
        assert!(!allow_at(9_900_001, t), "the sixth is refused");
        assert!(allow_at(9_900_002, t), "another member is not affected");
        assert!(
            allow_at(9_900_001, t + Duration::from_secs(61)),
            "a minute later it is allowed again"
        );
    }
}
