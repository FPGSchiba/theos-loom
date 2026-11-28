//! One line of the proxy's access log (format `loom` of [`super::squid_conf()`]:
//! `%ts %Ss %>Hs %rm %ru`) as a [`RunEvent::Egress`]. The URL is read with the `http` crate:
//! a full URL for plain HTTP, `host:port` for `CONNECT`.

use http::Uri;
use loom_core::run::RunEvent;

/// The `Egress` event of one access-log line; `None` for a line that is not one, and for a
/// request that names no destination. The caller logs those lines
/// ([`super::log_file`]), so none vanishes silently.
#[must_use]
pub fn parse_access_line(line: &str) -> Option<RunEvent> {
    let mut fields = line.split_ascii_whitespace();
    let (time, status, code, _method, url) = (
        fields.next()?,
        fields.next()?,
        fields.next()?,
        fields.next()?,
        fields.next()?,
    );
    let well_formed = fields.next().is_none()
        && is_number(time)
        && is_number(code)
        && status.bytes().all(|b| b.is_ascii_uppercase() || b == b'_');
    if !well_formed {
        return None;
    }
    let uri: Uri = url.parse().ok()?;
    // Without a scheme, only `CONNECT`'s `host:port` names a destination; Squid's
    // `error:invalid-request` and the like do not.
    if uri.scheme().is_none() && uri.port_u16().is_none() {
        return None;
    }
    let host = uri.host()?;
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    let port = uri.port_u16().or_else(|| match uri.scheme_str() {
        Some("http") => Some(80),
        Some("https") => Some(443),
        _ => None,
    });
    Some(RunEvent::Egress {
        host: host.to_owned(),
        port,
        allowed: !status.starts_with("TCP_DENIED"),
    })
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn egress(host: &str, port: Option<u16>, allowed: bool) -> RunEvent {
        RunEvent::Egress {
            host: host.to_owned(),
            port,
            allowed,
        }
    }

    // AC: TU-03.5
    #[test]
    fn a_denied_line_is_a_blocked_egress() {
        assert_eq!(
            parse_access_line("1795881442 TCP_DENIED 403 GET http://10.1.2.3:8080/x"),
            Some(egress("10.1.2.3", Some(8080), false))
        );
        assert_eq!(
            parse_access_line("1795881445 TCP_DENIED 403 CONNECT 10.1.2.3:443"),
            Some(egress("10.1.2.3", Some(443), false))
        );
        assert_eq!(
            parse_access_line("1795881445 TCP_DENIED_REPLY 403 GET http://intranet/"),
            Some(egress("intranet", Some(80), false))
        );
    }

    // AC: TU-03.5
    #[test]
    fn an_allowed_connect_names_host_and_port() {
        assert_eq!(
            parse_access_line("1795881444 TCP_TUNNEL 200 CONNECT example.org:8443\n"),
            Some(egress("example.org", Some(8443), true))
        );
        assert_eq!(
            parse_access_line("1795881444 TCP_TUNNEL 200 CONNECT [2001:db8::1]:443"),
            Some(egress("2001:db8::1", Some(443), true))
        );
        assert_eq!(
            parse_access_line("1795881442 TCP_MISS 200 GET http://198.18.7.10:8080/"),
            Some(egress("198.18.7.10", Some(8080), true))
        );
        assert_eq!(
            parse_access_line("1795881442 TCP_MISS_ABORTED 503 GET http://nowhere.invalid/"),
            Some(egress("nowhere.invalid", Some(80), true))
        );
    }

    #[test]
    fn other_lines_are_no_egress() {
        for line in [
            "",
            "2026/11/28 15:57:19| Accepting HTTP Socket connections at conn3 local=[::]:3128",
            "    listening port: 3128",
            "1795881442 TCP_MISS 200 GET",
            "1795881442 TCP_MISS 200 GET http://a/ extra",
            "x TCP_MISS 200 GET http://a/",
            "1795881442 tcp_miss 200 GET http://a/",
            "1795881442 NONE_NONE 400 NONE error:invalid-request",
        ] {
            assert_eq!(parse_access_line(line), None, "{line:?}");
        }
    }
}
