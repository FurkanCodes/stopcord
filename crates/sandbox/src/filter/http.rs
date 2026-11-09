//! Interprets the start of a proxy request.

use std::net::Ipv6Addr;

use super::hosts::normalize;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// `CONNECT host:port`: a tunnel, normally for TLS.
    Connect,
    /// An absolute-URI request, already rewritten into the head to send upstream.
    Plain { head: Vec<u8> },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub host: String,
    pub port: u16,
    pub kind: Kind,
}

/// Headers that belong to the connection to the proxy, or that a replaced value makes wrong.
const DROP_HEADERS: &[&str] = &[
    "host",
    "connection",
    "proxy-connection",
    "keep-alive",
    "proxy-authorization",
    "upgrade",
    "te",
];

/// Interprets a complete request head, final blank line included. `None` means malformed. Anything
/// ambiguous is refused rather than guessed at, because the filter's decision depends on the host.
pub(super) fn parse_head(head: &[u8]) -> Option<Target> {
    let body = head.strip_suffix(b"\r\n\r\n")?;
    let mut lines = split_lines(body);
    let request = std::str::from_utf8(lines.next()?).ok()?;
    let mut parts = request.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !is_token(method) || !version.starts_with("HTTP/1.") {
        return None;
    }
    if target.bytes().any(|b| b < 0x21 || b == 0x7f) {
        return None;
    }

    if method == "CONNECT" {
        let (host, port) = parse_authority(target, None)?;
        return Some(Target {
            host,
            port,
            kind: Kind::Connect,
        });
    }

    let rest = strip_scheme(target)?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    let (host, port) = parse_authority(authority, Some(80))?;
    let path = path.split('#').next().unwrap_or_default();
    let path = match path.chars().next() {
        Some('/') => path.to_string(),
        _ => format!("/{path}"),
    };

    let mut out = format!(
        "{method} {path} {version}\r\nHost: {}\r\n",
        host_header(&host, port)
    )
    .into_bytes();
    for line in lines {
        let name = header_name(line)?;
        if !DROP_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
            out.extend_from_slice(line);
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    Some(Target {
        host,
        port,
        kind: Kind::Plain { head: out },
    })
}

fn split_lines(head: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = Some(head);
    std::iter::from_fn(move || {
        let current = rest?;
        match current.windows(2).position(|w| w == b"\r\n") {
            Some(index) => {
                rest = Some(&current[index + 2..]);
                Some(&current[..index])
            }
            None => {
                rest = None;
                Some(current)
            }
        }
    })
}

/// The name of a `Name: value` header line. `None` for a line a server might read differently from us:
/// a bare CR or LF, a folded continuation, or a name that isn't a token.
fn header_name(line: &[u8]) -> Option<&str> {
    if line.iter().any(|b| *b == b'\r' || *b == b'\n') {
        return None;
    }
    let colon = line.iter().position(|b| *b == b':')?;
    let name = std::str::from_utf8(&line[..colon]).ok()?;
    is_token(name).then_some(name)
}

fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

fn strip_scheme(target: &str) -> Option<&str> {
    let (scheme, rest) = target.split_at_checked(7)?;
    scheme.eq_ignore_ascii_case("http://").then_some(rest)
}

/// Splits `host:port`. `default_port` of `None` means the port is required. Userinfo is refused:
/// `allowed.com@evil.com` has fooled more than one proxy about which host it is talking to.
fn parse_authority(authority: &str, default_port: Option<u16>) -> Option<(String, u16)> {
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        host.parse::<Ipv6Addr>().ok()?;
        let port = if after.is_empty() {
            None
        } else {
            Some(after.strip_prefix(':')?)
        };
        (host, port)
    } else {
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        if !valid_name(host) {
            return None;
        }
        (host, port)
    };
    let port = match port {
        Some(text) => text.parse::<u16>().ok().filter(|p| *p != 0)?,
        None => default_port?,
    };
    Some((normalize(host), port))
}

fn valid_name(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

fn host_header(host: &str, port: u16) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    if port == 80 {
        host
    } else {
        format!("{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_head(t: &Target) -> String {
        match &t.kind {
            Kind::Plain { head } => String::from_utf8_lossy(head).into_owned(),
            Kind::Connect => panic!("expected a plain request"),
        }
    }

    #[test]
    fn connect_gives_host_and_port() {
        let t = parse_head(b"CONNECT Example.com:8443 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        assert_eq!(t.host, "example.com");
        assert_eq!(t.port, 8443);
        assert_eq!(t.kind, Kind::Connect);
    }

    #[test]
    fn connect_needs_a_port() {
        assert!(parse_head(b"CONNECT example.com HTTP/1.1\r\n\r\n").is_none());
    }

    #[test]
    fn connect_takes_a_bracketed_ipv6_address() {
        let t = parse_head(b"CONNECT [::1]:443 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(t.host, "::1");
        assert_eq!(t.port, 443);
    }

    #[test]
    fn userinfo_is_refused_rather_than_guessed_at() {
        assert!(parse_head(b"CONNECT example.com@evil.com:443 HTTP/1.1\r\n\r\n").is_none());
        assert!(parse_head(b"GET http://example.com@evil.com/ HTTP/1.1\r\n\r\n").is_none());
    }

    #[test]
    fn hosts_with_stray_characters_are_refused() {
        assert!(parse_head(b"GET http://example.com\\.evil.com/ HTTP/1.1\r\n\r\n").is_none());
        assert!(parse_head(b"GET http://exa%6dple.com/ HTTP/1.1\r\n\r\n").is_none());
        assert!(parse_head(b"CONNECT :443 HTTP/1.1\r\n\r\n").is_none());
    }

    #[test]
    fn plain_request_becomes_origin_form_with_the_url_as_host() {
        let t = parse_head(
            b"GET http://Example.com/a/b?c=d HTTP/1.1\r\nHost: wrong.test\r\nAccept: */*\r\n\r\n",
        )
        .unwrap();
        assert_eq!(t.host, "example.com");
        assert_eq!(t.port, 80);
        assert_eq!(
            plain_head(&t),
            "GET /a/b?c=d HTTP/1.1\r\nHost: example.com\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn a_non_default_port_stays_in_the_host_header() {
        let t = parse_head(b"GET http://example.com:8080 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(t.port, 8080);
        assert_eq!(
            plain_head(&t),
            "GET / HTTP/1.1\r\nHost: example.com:8080\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn hop_by_hop_and_proxy_headers_are_dropped_whatever_their_case() {
        let t = parse_head(
            b"POST http://example.com/ HTTP/1.1\r\nPROXY-Connection: keep-alive\r\nProxy-Authorization: Basic x\r\nconnection: Upgrade\r\nKeep-Alive: 5\r\nUpgrade: websocket\r\nTE: trailers\r\nContent-Length: 2\r\n\r\n",
        )
        .unwrap();
        assert_eq!(
            plain_head(&t),
            "POST / HTTP/1.1\r\nHost: example.com\r\nContent-Length: 2\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn origin_form_and_other_schemes_are_refused() {
        assert!(parse_head(b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n").is_none());
        assert!(parse_head(b"GET https://example.com/ HTTP/1.1\r\n\r\n").is_none());
        assert!(parse_head(b"GET ftp://example.com/ HTTP/1.1\r\n\r\n").is_none());
    }

    #[test]
    fn malformed_lines_are_refused() {
        assert!(parse_head(b"GET\r\n\r\n").is_none());
        assert!(parse_head(b"GET http://a.com/ HTTP/1.1 extra\r\n\r\n").is_none());
        assert!(parse_head(b"GET http://a.com/ FTP/1.1\r\n\r\n").is_none());
        assert!(parse_head(b"G@T http://a.com/ HTTP/1.1\r\n\r\n").is_none());
        assert!(parse_head(b"\xff http://a.com/ HTTP/1.1\r\n\r\n").is_none());
    }

    #[test]
    fn control_characters_and_folded_headers_are_refused() {
        assert!(parse_head(b"GET http://a.com/\x01 HTTP/1.1\r\n\r\n").is_none());
        assert!(parse_head(b"GET http://a.com/ HTTP/1.1\r\nA: b\nC: d\r\n\r\n").is_none());
        assert!(parse_head(b"GET http://a.com/ HTTP/1.1\r\nA: b\r\n folded\r\n\r\n").is_none());
        assert!(parse_head(b"GET http://a.com/ HTTP/1.1\r\nno colon\r\n\r\n").is_none());
    }

    #[test]
    fn header_values_need_not_be_utf8() {
        let t = parse_head(b"GET http://a.com/ HTTP/1.1\r\nX: caf\xe9\r\n\r\n").unwrap();
        let Kind::Plain { head } = t.kind else {
            panic!()
        };
        assert!(head.windows(6).any(|w| w == b"X: caf"));
    }
}
