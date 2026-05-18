//! IP-based access control for `.htaccess`.
//!
//! Implements both the legacy Apache 2.2 syntax (`Allow from`,
//! `Deny from`, `Order`) and the modern Apache 2.4 syntax
//! (`Require ip`, `Require not ip`).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Default)]
pub struct AccessControl {
    /// `Require ip <CIDR>` — at least one match → allow.
    pub require_ip_allow: Vec<IpMatcher>,
    /// `Require not ip <CIDR>` — any match → deny.
    pub require_ip_deny: Vec<IpMatcher>,
    /// `Allow from <CIDR>` (legacy Apache 2.2).
    pub allow_from: Vec<IpMatcher>,
    /// `Deny from <CIDR>` (legacy Apache 2.2).
    pub deny_from: Vec<IpMatcher>,
    /// `Order allow,deny` or `Order deny,allow`. Default is
    /// `DenyAllow` (Apache 2.2 default: default-allow).
    pub order: Order,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// Default-deny; Allow first, then Deny. Deny wins on overlap.
    AllowDeny,
    /// Default-allow; Deny first, then Allow. Allow wins on overlap.
    #[default]
    DenyAllow,
}

#[derive(Debug, Clone)]
pub enum IpMatcher {
    /// Matches any IP.
    All,
    /// IPv4 CIDR: `192.168.1.0/24`, or bare `192.168.1.1` (/32).
    #[allow(dead_code)] // used in T4
    V4 { network: Ipv4Addr, prefix: u8 },
    /// IPv6 CIDR: `fe80::/10`, or bare `::1` (/128).
    #[allow(dead_code)] // used in T4
    V6 { network: Ipv6Addr, prefix: u8 },
}

impl IpMatcher {
    #[allow(dead_code)] // used in T4
    pub fn matches(&self, peer: IpAddr) -> bool {
        match (self, peer) {
            (IpMatcher::All, _) => true,
            (IpMatcher::V4 { network, prefix }, IpAddr::V4(p)) => {
                ipv4_in_cidr(*network, *prefix, p)
            }
            (IpMatcher::V6 { network, prefix }, IpAddr::V6(p)) => {
                ipv6_in_cidr(*network, *prefix, p)
            }
            _ => false, // V4 vs V6 mismatch never matches
        }
    }
}

#[allow(dead_code)] // used in T4 via IpMatcher::matches
fn ipv4_in_cidr(network: Ipv4Addr, prefix: u8, peer: Ipv4Addr) -> bool {
    if prefix == 0 {
        return true;
    }
    if prefix > 32 {
        return false;
    }
    let mask = u32::MAX.checked_shl(32 - prefix as u32).unwrap_or(0);
    let n = u32::from(network) & mask;
    let p = u32::from(peer) & mask;
    n == p
}

#[allow(dead_code)] // used in T4 via IpMatcher::matches
fn ipv6_in_cidr(network: Ipv6Addr, prefix: u8, peer: Ipv6Addr) -> bool {
    if prefix == 0 {
        return true;
    }
    if prefix > 128 {
        return false;
    }
    let mask: u128 = u128::MAX.checked_shl(128 - prefix as u32).unwrap_or(0);
    let n = u128::from(network) & mask;
    let p = u128::from(peer) & mask;
    n == p
}

/// Parse a CIDR/IP/`"all"` matcher string. Returns `Err(reason)`
/// on malformed input.
pub fn parse_ip_matcher(s: &str) -> Result<IpMatcher, String> {
    if s.eq_ignore_ascii_case("all") {
        return Ok(IpMatcher::All);
    }
    let (addr_part, prefix_opt) = match s.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (s, None),
    };
    if let Ok(v4) = addr_part.parse::<Ipv4Addr>() {
        let prefix = match prefix_opt {
            Some(p) => p
                .parse::<u8>()
                .map_err(|_| format!("invalid prefix '{}'", p))?,
            None => 32,
        };
        if prefix > 32 {
            return Err(format!("invalid IPv4 prefix /{}", prefix));
        }
        return Ok(IpMatcher::V4 { network: v4, prefix });
    }
    if let Ok(v6) = addr_part.parse::<Ipv6Addr>() {
        let prefix = match prefix_opt {
            Some(p) => p
                .parse::<u8>()
                .map_err(|_| format!("invalid prefix '{}'", p))?,
            None => 128,
        };
        if prefix > 128 {
            return Err(format!("invalid IPv6 prefix /{}", prefix));
        }
        return Ok(IpMatcher::V6 { network: v6, prefix });
    }
    Err(format!("invalid IP/CIDR '{}'", s))
}

/// Evaluate access control for a peer IP. Returns `Ok(())` if
/// access is allowed, or `Err(403)` if denied.
#[allow(dead_code)] // called in T4
pub fn evaluate(ac: &AccessControl, peer: IpAddr) -> Result<(), u16> {
    let has_modern = !ac.require_ip_allow.is_empty() || !ac.require_ip_deny.is_empty();
    let has_legacy = !ac.allow_from.is_empty() || !ac.deny_from.is_empty();
    if !has_modern && !has_legacy {
        return Ok(());
    }

    if has_modern {
        if ac.require_ip_deny.iter().any(|m| m.matches(peer)) {
            return Err(403);
        }
        if !ac.require_ip_allow.is_empty()
            && !ac.require_ip_allow.iter().any(|m| m.matches(peer))
        {
            return Err(403);
        }
    }

    if has_legacy {
        let allow = ac.allow_from.iter().any(|m| m.matches(peer));
        let deny = ac.deny_from.iter().any(|m| m.matches(peer));
        let ok = match ac.order {
            Order::AllowDeny => allow && !deny,
            Order::DenyAllow => allow || !deny,
        };
        if !ok {
            return Err(403);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn ip(s: &str) -> IpAddr {
        IpAddr::from_str(s).unwrap()
    }

    #[test]
    fn parse_ip_matcher_ipv4_cidr() {
        let m = parse_ip_matcher("192.168.1.0/24").unwrap();
        match m {
            IpMatcher::V4 { network, prefix } => {
                assert_eq!(network, Ipv4Addr::new(192, 168, 1, 0));
                assert_eq!(prefix, 24);
            }
            _ => panic!("expected V4"),
        }
    }

    #[test]
    fn parse_ip_matcher_ipv4_bare() {
        let m = parse_ip_matcher("127.0.0.1").unwrap();
        match m {
            IpMatcher::V4 { network, prefix } => {
                assert_eq!(network, Ipv4Addr::new(127, 0, 0, 1));
                assert_eq!(prefix, 32);
            }
            _ => panic!("expected V4"),
        }
    }

    #[test]
    fn parse_ip_matcher_ipv6_cidr() {
        let m = parse_ip_matcher("fe80::/10").unwrap();
        match m {
            IpMatcher::V6 { prefix, .. } => assert_eq!(prefix, 10),
            _ => panic!("expected V6"),
        }
    }

    #[test]
    fn parse_ip_matcher_all() {
        assert!(matches!(parse_ip_matcher("all").unwrap(), IpMatcher::All));
        assert!(matches!(parse_ip_matcher("ALL").unwrap(), IpMatcher::All));
    }

    #[test]
    fn parse_ip_matcher_malformed_errs() {
        assert!(parse_ip_matcher("999.999.999.999").is_err());
        assert!(parse_ip_matcher("192.168.1.0/40").is_err());
        assert!(parse_ip_matcher("foo").is_err());
    }

    #[test]
    fn ipv4_cidr_match_in_range() {
        let m = parse_ip_matcher("192.168.1.0/24").unwrap();
        assert!(m.matches(ip("192.168.1.50")));
        assert!(m.matches(ip("192.168.1.0")));
        assert!(m.matches(ip("192.168.1.255")));
    }

    #[test]
    fn ipv4_cidr_match_out_of_range() {
        let m = parse_ip_matcher("192.168.1.0/24").unwrap();
        assert!(!m.matches(ip("10.0.0.1")));
        assert!(!m.matches(ip("192.168.2.0")));
    }

    #[test]
    fn ipv6_cidr_match_loopback() {
        let m = parse_ip_matcher("::1/128").unwrap();
        assert!(m.matches(ip("::1")));
        assert!(!m.matches(ip("::2")));
    }

    #[test]
    fn evaluate_no_rules_allows() {
        let ac = AccessControl::default();
        assert!(evaluate(&ac, ip("1.2.3.4")).is_ok());
    }

    #[test]
    fn evaluate_require_ip_match_allows() {
        let mut ac = AccessControl::default();
        ac.require_ip_allow.push(parse_ip_matcher("127.0.0.0/8").unwrap());
        assert!(evaluate(&ac, ip("127.0.0.1")).is_ok());
    }

    #[test]
    fn evaluate_require_ip_no_match_denies() {
        let mut ac = AccessControl::default();
        ac.require_ip_allow.push(parse_ip_matcher("127.0.0.0/8").unwrap());
        assert_eq!(evaluate(&ac, ip("10.0.0.1")), Err(403));
    }

    #[test]
    fn evaluate_require_not_ip_match_denies() {
        let mut ac = AccessControl::default();
        ac.require_ip_deny.push(parse_ip_matcher("1.2.3.4").unwrap());
        assert_eq!(evaluate(&ac, ip("1.2.3.4")), Err(403));
    }

    #[test]
    fn evaluate_allow_deny_order_allow_deny_match_allow_no_deny_allows() {
        let mut ac = AccessControl {
            order: Order::AllowDeny,
            ..Default::default()
        };
        ac.allow_from.push(parse_ip_matcher("127.0.0.0/8").unwrap());
        assert!(evaluate(&ac, ip("127.0.0.1")).is_ok());
    }

    #[test]
    fn evaluate_allow_deny_order_allow_deny_match_both_denies() {
        let mut ac = AccessControl {
            order: Order::AllowDeny,
            ..Default::default()
        };
        ac.allow_from.push(parse_ip_matcher("127.0.0.0/8").unwrap());
        ac.deny_from.push(parse_ip_matcher("127.0.0.1").unwrap());
        assert_eq!(evaluate(&ac, ip("127.0.0.1")), Err(403));
    }

    #[test]
    fn evaluate_allow_deny_order_deny_allow_default_allows() {
        let mut ac = AccessControl {
            order: Order::DenyAllow,
            ..Default::default()
        };
        ac.deny_from.push(parse_ip_matcher("1.2.3.4").unwrap());
        assert!(evaluate(&ac, ip("127.0.0.1")).is_ok());
    }

    #[test]
    fn evaluate_allow_deny_order_deny_allow_deny_match_denies() {
        let mut ac = AccessControl {
            order: Order::DenyAllow,
            ..Default::default()
        };
        ac.deny_from.push(parse_ip_matcher("1.2.3.4").unwrap());
        assert_eq!(evaluate(&ac, ip("1.2.3.4")), Err(403));
    }
}
