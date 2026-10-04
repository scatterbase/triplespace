//! Who the client is behind proxies (0056 §10 line 5; 0057 §10).
//!
//! Each proxy appends the address that connected to it to `X-Forwarded-For`, and may
//! append its forwarder key to `Triplespace-Forwarder`. The header is text any client can
//! send, so only the server judges it: it reads `X-Forwarded-For` from the right,
//! believing one more entry for each hop it trusts and stopping at the first it does not.
//! A hop is trusted when its address (the peer's, or the entry the next hop appended) is
//! in `server.trusted_proxies`, a list of addresses and CIDR ranges; otherwise when the
//! next unused entry from the right of `Triplespace-Forwarder` is a live forwarder key.
//! `X-Forwarded-Host` and `X-Forwarded-Proto` are believed only from a trusted peer.
//!
//! A hop trusted by its address sends no key; one trusted by a key sends exactly one.
//!
//! ```text
//! client C → edge E → web W → server
//! X-Forwarded-For:       F, C, E      (F is forged by the client)
//! Triplespace-Forwarder: kE, kW
//! W is trusted by kW, so believe E; E is trusted by kE, so believe C; stop: the client is C
//! ```

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;

use crate::app::App;

/// The most `X-Forwarded-For` entries and forwarder keys read from the right. A longer
/// chain is cut at the left, which only ever trusts less.
pub const MAX_HOPS: usize = 16;

/// How long a forwarder key's check is remembered. Revoking a key takes effect within it.
pub const KEY_CACHE_TTL: Duration = Duration::from_secs(30);

/// An address or a CIDR range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Parses `10.0.0.0/8`, `fd00::/8`, or a bare address (a /32 or /128).
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        let (addr, prefix) = match s.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (s, None),
        };
        let addr: IpAddr = addr
            .parse()
            .map_err(|_| format!("`{s}` is not an address or a CIDR range"))?;
        let addr = addr.to_canonical();
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => max,
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(|| format!("`{s}`: the prefix length is 0–{max}"))?,
        };
        Ok(Self { addr, prefix })
    }

    /// Whether the address is in the range.
    #[must_use]
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// `server.trusted_proxies`, parsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustList(Vec<Cidr>);

impl TrustList {
    /// Parses the operator's entries; an invalid one is an error, so a typo fails at start.
    pub fn parse<S: AsRef<str>>(entries: &[S]) -> Result<Self, String> {
        entries
            .iter()
            .map(AsRef::as_ref)
            .filter(|e| !e.trim().is_empty())
            .map(Cidr::parse)
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    /// Whether the address is trusted.
    #[must_use]
    pub fn contains(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|c| c.contains(ip))
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The comma-separated entries of every instance of a header, trimmed, at most
/// [`MAX_HOPS`] of them, keeping the rightmost.
#[must_use]
pub fn entries(headers: &HeaderMap, name: &str) -> Vec<String> {
    let mut all: Vec<String> = headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .collect();
    if all.len() > MAX_HOPS {
        all.drain(..all.len() - MAX_HOPS);
    }
    all
}

/// An `X-Forwarded-For` entry as an address: `198.51.100.7`, `198.51.100.7:4711`,
/// `2001:db8::1`, `[2001:db8::1]:4711`. Anything else (`unknown`, an obfuscated
/// identifier) is `None`.
#[must_use]
pub fn parse_addr(entry: &str) -> Option<IpAddr> {
    let e = entry.trim();
    if let Ok(ip) = e.parse::<IpAddr>() {
        return Some(ip.to_canonical());
    }
    if let Ok(sa) = e.parse::<SocketAddr>() {
        return Some(sa.ip().to_canonical());
    }
    e.strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .and_then(|r| r.parse::<IpAddr>().ok())
        .map(|ip| ip.to_canonical())
}

/// The outcome of the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
    /// The client: the last entry believed, or the peer when no hop is trusted. `None`
    /// when the peer is unknown and nothing could be believed.
    pub client: Option<IpAddr>,
    /// Whether the peer itself is trusted, which is what `X-Forwarded-Host` and
    /// `X-Forwarded-Proto` need.
    pub peer_trusted: bool,
    /// The indexes into the key list of the keys that vouched, rightmost first.
    pub keys_used: Vec<usize>,
}

/// The walk, with the keys already checked: `key_valid[i]` says whether the `i`th entry
/// of `Triplespace-Forwarder` is a live key.
#[must_use]
pub fn walk(peer: Option<IpAddr>, xff: &[String], key_valid: &[bool], trust: &TrustList) -> Walk {
    let mut hop = peer.map(|p| p.to_canonical());
    let mut client = hop;
    let mut next_entry = xff.len();
    let mut next_key = key_valid.len();
    let mut keys_used = Vec::new();
    let mut peer_trusted = false;
    let mut first = true;
    loop {
        let by_address = hop.is_some_and(|h| trust.contains(h));
        let trusted = by_address || {
            if next_key > 0 && key_valid[next_key - 1] {
                next_key -= 1;
                keys_used.push(next_key);
                true
            } else {
                false
            }
        };
        if first {
            peer_trusted = trusted;
            first = false;
        }
        if !trusted || next_entry == 0 {
            break;
        }
        next_entry -= 1;
        let Some(addr) = parse_addr(&xff[next_entry]) else {
            break;
        };
        client = Some(addr);
        hop = Some(addr);
    }
    Walk {
        client,
        peer_trusted,
        keys_used,
    }
}

/// Where a request came from, as the server believes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    /// The address of the connection, when the listener reports one.
    pub peer: Option<IpAddr>,
    /// The client (0016 §3, 0024 §5 and 0030 §11 read it).
    pub client: Option<IpAddr>,
    /// Whether the peer is a trusted proxy.
    pub peer_trusted: bool,
    /// `X-Forwarded-Host`, from a trusted peer only.
    pub forwarded_host: Option<String>,
    /// `X-Forwarded-Proto`, from a trusted peer only, lower-cased.
    pub forwarded_proto: Option<String>,
    /// The forwarder keys that vouched, by key ID, rightmost first.
    pub forwarders: Vec<String>,
}

fn first_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Remembers forwarder key checks for [`KEY_CACHE_TTL`], so a chain of proxies costs a
/// query per key a minute, not per request.
#[derive(Debug, Default)]
pub struct KeyCache {
    entries: Mutex<HashMap<String, (Checked, Instant)>>,
}

/// A remembered check: the key ID of a live key, or refused.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Checked {
    Live(String),
    Refused,
}

impl Checked {
    fn from_check(v: Option<String>) -> Self {
        v.map_or(Self::Refused, Self::Live)
    }

    fn key_id(self) -> Option<String> {
        match self {
            Self::Live(k) => Some(k),
            Self::Refused => None,
        }
    }
}

impl KeyCache {
    const MAX: usize = 1024;

    fn get(&self, presented: &str) -> Option<Checked> {
        let map = self.entries.lock().ok()?;
        map.get(presented)
            .filter(|(_, at)| at.elapsed() < KEY_CACHE_TTL)
            .map(|(v, _)| v.clone())
    }

    fn put(&self, presented: &str, value: Checked) {
        if let Ok(mut map) = self.entries.lock() {
            if map.len() >= Self::MAX {
                map.retain(|_, (_, at)| at.elapsed() < KEY_CACHE_TTL);
                if map.len() >= Self::MAX {
                    map.clear();
                }
            }
            map.insert(presented.to_string(), (value, Instant::now()));
        }
    }
}

/// Checks the presented keys, through the cache. A key that cannot be checked (no
/// connection) is not trusted.
async fn check_keys(app: &App, presented: &[String]) -> Vec<Option<String>> {
    let mut out = vec![None; presented.len()];
    let mut client = None;
    for (i, k) in presented.iter().enumerate() {
        if let Some(v) = app.forwarder_keys().get(k) {
            out[i] = v.key_id();
            continue;
        }
        if client.is_none() {
            client = app.pool().get().await.ok();
        }
        let Some(c) = client.as_ref() else { break };
        if let Ok(v) = triplespace_accounts::forwarder::verify(&***c, k).await {
            app.forwarder_keys().put(k, Checked::from_check(v.clone()));
            out[i] = v;
        }
    }
    out
}

/// Works out the request's [`Origin`].
pub async fn origin(app: &App, peer: Option<IpAddr>, headers: &HeaderMap) -> Origin {
    let xff = entries(headers, "x-forwarded-for");
    let presented = entries(headers, triplespace_accounts::forwarder::HEADER);
    let checked = if presented.is_empty() {
        Vec::new()
    } else {
        check_keys(app, &presented).await
    };
    let valid: Vec<bool> = checked.iter().map(Option::is_some).collect();
    let w = walk(peer, &xff, &valid, app.trust());
    let (forwarded_host, forwarded_proto) = if w.peer_trusted {
        (
            first_value(headers, "x-forwarded-host"),
            first_value(headers, "x-forwarded-proto").map(|p| p.to_ascii_lowercase()),
        )
    } else {
        (None, None)
    };
    Origin {
        peer,
        client: w.client,
        peer_trusted: w.peer_trusted,
        forwarded_host,
        forwarded_proto,
        forwarders: w
            .keys_used
            .iter()
            .filter_map(|&i| checked[i].clone())
            .collect(),
    }
}

/// The layer that puts the [`Origin`] on every request. The peer is the listener's
/// `ConnectInfo`; a request without one (served in process, or by a test) has no peer,
/// so nothing it forwards is believed.
pub async fn layer(State(app): State<App>, mut req: Request, next: Next) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    let o = origin(&app, peer, req.headers()).await;
    req.extensions_mut().insert(o);
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn list(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn cidrs() {
        let c = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(c.contains(ip("10.244.3.7")));
        assert!(!c.contains(ip("11.0.0.1")));
        assert!(c.contains(ip("::ffff:10.1.2.3")), "IPv4-mapped peers match");
        let one = Cidr::parse("192.0.2.5").unwrap();
        assert!(one.contains(ip("192.0.2.5")));
        assert!(!one.contains(ip("192.0.2.6")));
        let v6 = Cidr::parse("fd00::/8").unwrap();
        assert!(v6.contains(ip("fd12:3456::1")));
        assert!(!v6.contains(ip("2001:db8::1")));
        assert!(!v6.contains(ip("10.0.0.1")));
        assert!(
            Cidr::parse("0.0.0.0/0")
                .unwrap()
                .contains(ip("203.0.113.1"))
        );
        assert!(Cidr::parse("10.0.0.0/33").is_err());
        assert!(Cidr::parse("edge.internal").is_err());
        assert!(TrustList::parse(&["10.0.0.0/8", " "]).is_ok());
        assert!(TrustList::parse(&["nonsense"]).is_err());
    }

    #[test]
    fn addresses() {
        assert_eq!(parse_addr("198.51.100.7"), Some(ip("198.51.100.7")));
        assert_eq!(parse_addr("198.51.100.7:4711"), Some(ip("198.51.100.7")));
        assert_eq!(parse_addr("2001:db8::1"), Some(ip("2001:db8::1")));
        assert_eq!(parse_addr("[2001:db8::1]:4711"), Some(ip("2001:db8::1")));
        assert_eq!(parse_addr("[2001:db8::1]"), Some(ip("2001:db8::1")));
        assert_eq!(parse_addr("unknown"), None);
        assert_eq!(parse_addr("_hidden"), None);
    }

    #[test]
    fn untrusted_peer_is_the_client() {
        let trust = TrustList::parse(&["10.0.0.0/8"]).unwrap();
        let w = walk(Some(ip("203.0.113.9")), &list(&["1.2.3.4"]), &[], &trust);
        assert_eq!(w.client, Some(ip("203.0.113.9")));
        assert!(!w.peer_trusted);
    }

    #[test]
    fn no_peer_believes_only_a_key() {
        // Without a connection address (in process, a test) no hop is trusted by address;
        // a valid key still vouches, since it is a credential.
        let trust = TrustList::parse(&["0.0.0.0/0"]).unwrap();
        let w = walk(None, &list(&["198.51.100.7"]), &[], &trust);
        assert_eq!(w.client, None);
        assert!(!w.peer_trusted);
        let w = walk(None, &list(&["198.51.100.7"]), &[true], &trust);
        assert_eq!(w.client, Some(ip("198.51.100.7")));
    }

    #[test]
    fn address_trust_reads_from_the_right() {
        // client C=198.51.100.7 → edge 10.0.0.2 → web 10.0.0.3 → server; F forged.
        let trust = TrustList::parse(&["10.0.0.0/24"]).unwrap();
        let xff = list(&["6.6.6.6", "198.51.100.7", "10.0.0.2"]);
        let w = walk(Some(ip("10.0.0.3")), &xff, &[], &trust);
        assert_eq!(w.client, Some(ip("198.51.100.7")));
        assert!(w.peer_trusted);
    }

    #[test]
    fn a_pod_in_the_range_cannot_forge_when_the_range_is_not_trusted() {
        // Keys only: the pod at 10.244.9.9 sends a forged XFF and no key.
        let w = walk(
            Some(ip("10.244.9.9")),
            &list(&["192.0.2.1"]),
            &[],
            &TrustList::default(),
        );
        assert_eq!(w.client, Some(ip("10.244.9.9")));
        assert!(!w.peer_trusted);
    }

    #[test]
    fn key_trust_reads_from_the_right() {
        // The 0057 §10 example: XFF "F, C, E", keys "kE, kW", peer W.
        let xff = list(&["6.6.6.6", "198.51.100.7", "10.244.1.5"]);
        let w = walk(
            Some(ip("10.244.2.8")),
            &xff,
            &[true, true],
            &TrustList::default(),
        );
        assert_eq!(w.client, Some(ip("198.51.100.7")));
        assert!(w.peer_trusted);
        assert_eq!(w.keys_used, vec![1, 0]);
    }

    #[test]
    fn a_forged_key_stops_the_walk() {
        // A rogue pod connects to the web tier with a forged key and XFF; the web tier
        // appends the pod's address and its own valid key.
        let xff = list(&["192.0.2.1", "10.244.9.9"]);
        let w = walk(
            Some(ip("10.244.2.8")),
            &xff,
            &[false, true],
            &TrustList::default(),
        );
        assert_eq!(w.client, Some(ip("10.244.9.9")));
        assert_eq!(w.keys_used, vec![1]);
    }

    #[test]
    fn address_and_key_trust_mix() {
        // Edge trusted by address (10.0.0.2), web tier by key.
        let trust = TrustList::parse(&["10.0.0.2"]).unwrap();
        let xff = list(&["198.51.100.7", "10.0.0.2"]);
        let w = walk(Some(ip("10.244.2.8")), &xff, &[true], &trust);
        assert_eq!(w.client, Some(ip("198.51.100.7")));
        assert_eq!(w.keys_used, vec![0]);
    }

    #[test]
    fn a_trusted_hop_with_nothing_appended_is_the_client() {
        let trust = TrustList::parse(&["127.0.0.1"]).unwrap();
        let w = walk(Some(ip("127.0.0.1")), &[], &[], &trust);
        assert_eq!(w.client, Some(ip("127.0.0.1")));
        assert!(w.peer_trusted);
    }

    #[test]
    fn an_unparseable_entry_stops_the_walk() {
        let trust = TrustList::parse(&["10.0.0.0/8"]).unwrap();
        let xff = list(&["198.51.100.7", "unknown"]);
        let w = walk(Some(ip("10.0.0.3")), &xff, &[], &trust);
        assert_eq!(w.client, Some(ip("10.0.0.3")));
    }

    #[test]
    fn everything_trusted_ends_at_the_leftmost_entry() {
        let trust = TrustList::parse(&["0.0.0.0/0"]).unwrap();
        let xff = list(&["198.51.100.7", "10.0.0.2"]);
        let w = walk(Some(ip("10.0.0.3")), &xff, &[], &trust);
        assert_eq!(w.client, Some(ip("198.51.100.7")));
    }

    #[test]
    fn long_chains_are_cut_at_the_left() {
        let mut h = HeaderMap::new();
        let many: Vec<String> = (0..40).map(|i| format!("10.0.0.{i}")).collect();
        h.insert("x-forwarded-for", many.join(", ").parse().unwrap());
        let e = entries(&h, "x-forwarded-for");
        assert_eq!(e.len(), MAX_HOPS);
        assert_eq!(e.last().unwrap(), "10.0.0.39");
    }

    #[test]
    fn several_header_lines_concatenate() {
        let mut h = HeaderMap::new();
        h.append("x-forwarded-for", "1.1.1.1".parse().unwrap());
        h.append("x-forwarded-for", "2.2.2.2, 3.3.3.3".parse().unwrap());
        assert_eq!(
            entries(&h, "x-forwarded-for"),
            vec!["1.1.1.1", "2.2.2.2", "3.3.3.3"]
        );
    }
}
