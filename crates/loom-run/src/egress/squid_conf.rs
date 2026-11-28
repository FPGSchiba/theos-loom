//! The proxy's configuration (spec §5.3 step 4, R-c): Squid allows HTTP and `CONNECT` to any
//! port, except to the blocked address ranges, which it checks against the *resolved* address
//! (`dst`), and logs every request in one line Loom reads back ([`super::access_log`]).

use std::fmt::Write as _;
use std::net::IpAddr;

/// The port Squid listens on.
pub const PROXY_PORT: u16 = 3128;

/// The file Squid writes its access log to. The image's entrypoint follows it onto the
/// container's standard output, where Loom reads it.
pub const ACCESS_LOG: &str = "/var/log/squid/access.log";

/// Blocked always (spec §5.3 step 4, plus multicast and reserved ranges). None of them overlaps
/// another: Squid ignores the wider of two overlapping ranges in one ACL.
pub const DEFAULT_BLOCKED: &[&str] = &[
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "0.0.0.0/8",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
    "224.0.0.0/4",
    "240.0.0.0/4",
    "ff00::/8",
];

/// The text of `squid.conf` for one run. `blocked` are the extra ranges of the run config, each
/// checked with [`check_range`] first; each gets an ACL of its own, so a range that overlaps a
/// default one is never dropped by Squid.
#[must_use]
pub fn squid_conf(blocked: &[String]) -> String {
    let mut conf = format!("http_port {PROXY_PORT}\n");
    let _ = writeln!(conf, "acl loom_blocked dst {}", DEFAULT_BLOCKED.join(" "));
    conf.push_str("http_access deny loom_blocked\n");
    for (n, range) in blocked.iter().enumerate() {
        let _ = writeln!(conf, "acl loom_blocked_{n} dst {range}");
        let _ = writeln!(conf, "http_access deny loom_blocked_{n}");
    }
    conf.push_str(concat!(
        "http_access allow all\n",
        "cache deny all\n",
        "logformat loom %ts %Ss %>Hs %rm %ru\n",
    ));
    let _ = writeln!(conf, "access_log stdio:{ACCESS_LOG} loom");
    // A stopped run's proxy ends at once instead of waiting for open connections; and no ICMP
    // helper, which a rootless container cannot run.
    conf.push_str("shutdown_lifetime 0 seconds\npinger_enable off\n");
    conf
}

/// Checks that `range` is an address range in CIDR form (`10.0.0.0/8`, `fd00::/8`) or a single
/// address, so nothing but a range ever reaches the config.
///
/// # Errors
/// A message naming the range when it is not one.
pub fn check_range(range: &str) -> Result<(), String> {
    let bad = || format!("{range:?} is not an address range (such as 10.0.0.0/8)");
    let (addr, prefix) = match range.split_once('/') {
        Some((addr, prefix)) => (addr, Some(prefix)),
        None => (range, None),
    };
    let addr: IpAddr = addr.parse().map_err(|_| bad())?;
    let max = if addr.is_ipv4() { 32 } else { 128 };
    match prefix {
        None => Ok(()),
        Some(prefix) if prefix.bytes().all(|b| b.is_ascii_digit()) => match prefix.parse::<u8>() {
            Ok(bits) if bits <= max => Ok(()),
            _ => Err(bad()),
        },
        Some(_) => Err(bad()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ranges of the `dst` ACL named `name`.
    fn acl<'a>(conf: &'a str, name: &str) -> Vec<&'a str> {
        let prefix = format!("acl {name} dst ");
        conf.lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .map(|ranges| ranges.split(' ').collect())
            .unwrap_or_default()
    }

    fn position(conf: &str, line: &str) -> usize {
        conf.lines()
            .position(|l| l == line)
            .unwrap_or_else(|| panic!("no line {line:?} in\n{conf}"))
    }

    // AC: TU-03.5
    #[test]
    fn the_config_blocks_every_private_range() {
        let conf = squid_conf(&["198.51.100.0/24".to_owned(), "10.1.0.0/16".to_owned()]);
        let defaults = acl(&conf, "loom_blocked");
        for range in [
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "100.64.0.0/10",
            "127.0.0.0/8",
            "169.254.0.0/16",
            "0.0.0.0/8",
            "::1/128",
            "fc00::/7",
            "fe80::/10",
            "224.0.0.0/4",
            "240.0.0.0/4",
            "ff00::/8",
        ] {
            assert!(defaults.contains(&range), "{range} is not blocked:\n{conf}");
        }
        assert_eq!(acl(&conf, "loom_blocked_0"), ["198.51.100.0/24"]);
        // Overlaps 10.0.0.0/8, so it has an ACL of its own.
        assert_eq!(acl(&conf, "loom_blocked_1"), ["10.1.0.0/16"]);
        let allow = position(&conf, "http_access allow all");
        for deny in [
            "http_access deny loom_blocked",
            "http_access deny loom_blocked_0",
            "http_access deny loom_blocked_1",
        ] {
            assert!(
                position(&conf, deny) < allow,
                "{deny} after the allow:\n{conf}"
            );
        }
        assert_eq!(conf.lines().next(), Some("http_port 3128"));
        for line in [
            "cache deny all",
            "logformat loom %ts %Ss %>Hs %rm %ru",
            "access_log stdio:/var/log/squid/access.log loom",
        ] {
            position(&conf, line);
        }
    }

    #[test]
    fn no_two_default_ranges_overlap() {
        let parsed: Vec<(IpAddr, u8)> = DEFAULT_BLOCKED
            .iter()
            .map(|r| {
                let (a, p) = r.split_once('/').expect("a prefix");
                (a.parse().expect("an address"), p.parse().expect("bits"))
            })
            .collect();
        // Both families as the top bits of a u128, so a prefix of `len` bits is the top `len`.
        let bits = |ip: IpAddr| match ip {
            IpAddr::V4(v4) => u128::from(u32::from(v4)) << 96,
            IpAddr::V6(v6) => u128::from(v6),
        };
        let cover = |(net, len): (IpAddr, u8), ip: IpAddr| {
            let shift = 128 - u32::from(len);
            net.is_ipv4() == ip.is_ipv4()
                && (shift >= 128 || bits(net) >> shift == bits(ip) >> shift)
        };
        for (i, a) in parsed.iter().enumerate() {
            for (j, b) in parsed.iter().enumerate() {
                if i != j {
                    assert!(!cover(*a, b.0), "{a:?} contains {b:?}");
                }
            }
        }
    }

    #[test]
    fn only_address_ranges_pass_the_check() {
        for good in ["10.0.0.0/8", "1.2.3.4", "fd00::/8", "::1/128", "0.0.0.0/0"] {
            assert_eq!(check_range(good), Ok(()), "{good}");
        }
        for bad in [
            "",
            "10.0.0.0/33",
            "fd00::/129",
            "10.0.0.0/+8",
            "example.org",
            "10.0.0.0/8\nhttp_access allow all",
            "10.0.0.0/8 all",
        ] {
            assert!(check_range(bad).is_err(), "{bad:?}");
        }
    }
}
