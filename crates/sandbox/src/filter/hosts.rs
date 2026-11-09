//! Which hosts the filter lets through.

use std::net::IpAddr;

/// Lower-cases a host and removes a trailing dot, and writes an IP literal in its one canonical form,
/// so that two spellings of the same host compare equal.
pub(super) fn normalize(host: &str) -> String {
    let lowered = host.trim().to_ascii_lowercase();
    let bare = lowered.trim_end_matches('.');
    let bare = bare
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(bare);
    match bare.parse::<IpAddr>() {
        Ok(ip) => ip.to_string(),
        Err(_) => bare.to_string(),
    }
}

fn is_ip(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok()
}

/// Whether `host` is `domain` or a subdomain of it. Both must already be normalized. IP literals match
/// only exactly: treating `1.1.1.1` as a domain would let a host name that ends in it through.
fn covers(domain: &str, host: &str) -> bool {
    if domain == host {
        return true;
    }
    if is_ip(domain) || is_ip(host) {
        return false;
    }
    host.strip_suffix(domain)
        .is_some_and(|prefix| prefix.ends_with('.'))
}

/// The allowed domains, plus hosts that may each pass once.
pub(super) struct AllowList {
    domains: Vec<String>,
    once: Vec<String>,
}

impl AllowList {
    pub(super) fn new(domains: Vec<String>) -> AllowList {
        let mut list = AllowList {
            domains: Vec::new(),
            once: Vec::new(),
        };
        for domain in domains {
            list.add(&domain);
        }
        list
    }

    /// Whether a request to `host` passes now. Uses up an allow-once when that is what lets it through,
    /// and leaves it alone when the host is allowed anyway.
    pub(super) fn permits(&mut self, host: &str) -> bool {
        if self.domains.iter().any(|d| covers(d, host)) {
            return true;
        }
        match self.once.iter().position(|h| h == host) {
            Some(index) => {
                self.once.remove(index);
                true
            }
            None => false,
        }
    }

    /// Allows the next request to exactly this host, and not its subdomains.
    pub(super) fn add_once(&mut self, host: &str) {
        let host = normalize(host);
        if !host.is_empty() {
            self.once.push(host);
        }
    }

    pub(super) fn add(&mut self, domain: &str) {
        let domain = normalize(domain.trim_start_matches("*."));
        if !domain.is_empty() && domain != "*" && !self.domains.contains(&domain) {
            self.domains.push(domain);
        }
    }

    pub(super) fn domains(&self) -> Vec<String> {
        self.domains.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(domains: &[&str]) -> AllowList {
        AllowList::new(domains.iter().map(|d| d.to_string()).collect())
    }

    #[test]
    fn a_domain_matches_itself_and_its_subdomains() {
        let mut l = list(&["example.com"]);
        assert!(l.permits("example.com"));
        assert!(l.permits("api.example.com"));
        assert!(l.permits("a.b.example.com"));
    }

    #[test]
    fn look_alikes_do_not_match() {
        let mut l = list(&["example.com"]);
        assert!(!l.permits("evil-example.com"));
        assert!(!l.permits("notexample.com"));
        assert!(!l.permits("example.com.evil.net"));
        assert!(!l.permits("com"));
    }

    #[test]
    fn matching_ignores_case_and_a_trailing_dot() {
        let mut l = list(&["Example.COM"]);
        assert!(l.permits(&normalize("API.example.com")));
        assert!(l.permits(&normalize("example.com.")));
    }

    #[test]
    fn an_ip_literal_matches_only_exactly() {
        let mut l = list(&["10.0.0.1"]);
        assert!(l.permits("10.0.0.1"));
        assert!(!l.permits("10.0.0.2"));
        assert!(!l.permits("x.10.0.0.1"));
        assert!(!l.permits("100.0.0.1"));
    }

    #[test]
    fn an_ipv6_literal_matches_in_any_spelling() {
        let mut l = list(&["0:0:0:0:0:0:0:1"]);
        assert!(l.permits(&normalize("::1")));
        assert!(!l.permits(&normalize("::2")));
    }

    #[test]
    fn a_domain_never_matches_an_ip_suffix() {
        // "1.1" as a suffix of an address must not be read as a parent domain.
        let mut l = list(&["0.0.1"]);
        assert!(!l.permits("127.0.0.1"));
    }

    #[test]
    fn a_wildcard_entry_means_the_domain() {
        let mut l = list(&["*.example.com"]);
        assert!(l.permits("api.example.com"));
        assert!(l.permits("example.com"));
    }

    #[test]
    fn allow_once_passes_exactly_once_and_only_that_host() {
        let mut l = list(&[]);
        l.add_once("Once.example.com");
        assert!(!l.permits("other.once.example.com"));
        assert!(l.permits("once.example.com"));
        assert!(!l.permits("once.example.com"));
    }

    #[test]
    fn an_allowed_host_does_not_use_up_an_allow_once() {
        let mut l = list(&["example.com"]);
        l.add_once("example.com");
        assert!(l.permits("example.com"));
        assert!(l.permits("example.com"));
        l.domains().retain(|_| false);
        assert_eq!(l.domains(), ["example.com"]);
    }

    #[test]
    fn add_keeps_the_list_tidy() {
        let mut l = list(&["a.com"]);
        l.add("B.com");
        l.add("b.com");
        assert_eq!(l.domains(), ["a.com", "b.com"]);
        assert!(l.permits("x.b.com"));
    }

    #[test]
    fn nonsense_entries_are_ignored() {
        let l = list(&["", ".", "*."]);
        assert!(l.domains().is_empty());
    }
}
