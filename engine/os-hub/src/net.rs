//! Which client addresses the hub accepts (ADR-0029 section 2): private
//! ranges only by default, a setting on the hub.

use std::net::IpAddr;

/// The default allowed ranges: this computer (loopback, for the hub
/// computer's own device), RFC 1918, link-local, IPv6 unique local
/// addresses, and 100.64.0.0/10 (carrier-grade NAT, which VPNs such as
/// Tailscale use).
pub const DEFAULT_RANGES: [&str; 9] = [
    "127.0.0.0/8",
    "::1/128",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "169.254.0.0/16",
    "fe80::/10",
    "fc00::/7",
    "100.64.0.0/10",
];

/// One address range, `address/prefix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl Cidr {
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim();
        let (a, p) = match t.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (t, None),
        };
        let addr = canonical(a.trim().trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>().ok()?);
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match p {
            Some(p) => p.trim().parse::<u8>().ok().filter(|p| *p <= max)?,
            None => max,
        };
        Some(Self { addr, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, canonical(ip)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - self.prefix) };
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u128::MAX << (128 - self.prefix) };
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for Cidr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

/// An IPv4 address that arrived as IPv6 (`::ffff:a.b.c.d` on a dual-stack
/// socket) is checked as IPv4.
pub fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 => v4,
    }
}

/// Parse a list of ranges; an entry that doesn't parse is an error naming it.
pub fn parse_ranges(list: &[String]) -> Result<Vec<Cidr>, String> {
    let mut out = Vec::new();
    for item in list {
        if item.trim().is_empty() {
            continue;
        }
        out.push(
            Cidr::parse(item)
                .ok_or_else(|| format!("{} isn't an address range such as 192.168.0.0/16.", item.trim()))?,
        );
    }
    Ok(out)
}

pub fn default_ranges() -> Vec<String> {
    DEFAULT_RANGES.iter().map(|s| s.to_string()).collect()
}

pub fn allowed(ranges: &[Cidr], ip: IpAddr) -> bool {
    let ip = canonical(ip);
    ranges.iter().any(|r| r.contains(ip))
}

/// This computer's addresses that devices could use (not loopback or IPv6
/// link-local, within the allowed ranges), IPv4 first, for the join code.
pub fn lan_addresses(ranges: &[Cidr]) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback() && i.is_oper_up())
        .map(|i| canonical(i.ip()))
        .filter(|ip| !matches!(ip, IpAddr::V6(v6) if v6.segments()[0] & 0xffc0 == 0xfe80))
        .filter(|ip| allowed(ranges, *ip))
        .collect();
    out.sort_by_key(|ip| (!ip.is_ipv4(), *ip));
    out.dedup();
    out
}

/// The key lockouts and rate limits count under: the address, or for IPv6
/// its /64 (one device can hold a whole /64).
pub fn limit_key(ip: IpAddr) -> String {
    match canonical(ip) {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn private_ranges_are_allowed_by_default_and_nothing_else() {
        let r = parse_ranges(&default_ranges()).unwrap();
        for ok in [
            "192.168.1.20",
            "10.2.3.4",
            "172.16.0.1",
            "172.31.255.255",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "::1",
            "fd12::1",
            "fe80::1",
            "::ffff:192.168.1.5",
        ] {
            assert!(allowed(&r, ip(ok)), "{ok}");
        }
        for no in ["8.8.8.8", "172.32.0.1", "100.128.0.1", "2001:db8::1", "::ffff:8.8.8.8", "1.1.1.1"] {
            assert!(!allowed(&r, ip(no)), "{no}");
        }
    }

    #[test]
    fn ranges_parse_and_refuse_nonsense() {
        let r = parse_ranges(&["127.0.0.1/32".into(), "".into(), "fd00::/8".into()]).unwrap();
        assert_eq!(r.len(), 2);
        assert!(allowed(&r, ip("127.0.0.1")));
        assert!(!allowed(&r, ip("127.0.0.2")));
        assert!(Cidr::parse("10.0.0.0/33").is_none());
        assert!(Cidr::parse("nonsense").is_none());
        assert!(parse_ranges(&["10.0.0.0/8".into(), "x".into()]).is_err());
        assert_eq!(Cidr::parse("192.168.1.7").unwrap().to_string(), "192.168.1.7/32");
        assert!(Cidr::parse("0.0.0.0/0").unwrap().contains(ip("8.8.8.8")));
    }

    #[test]
    fn ipv6_counts_per_64() {
        assert_eq!(limit_key(ip("fd00:1:2:3:4:5:6:7")), limit_key(ip("fd00:1:2:3:ffff::1")));
        assert_ne!(limit_key(ip("fd00:1:2:3::1")), limit_key(ip("fd00:1:2:4::1")));
        assert_eq!(limit_key(ip("::ffff:10.0.0.1")), "10.0.0.1");
    }
}
