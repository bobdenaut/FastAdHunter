use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;
use fah_common::egress::AllowedNet;
use fah_model::InterceptionDocument;
use serde::Serialize;

pub const MAX_NAME_LEN: usize = 253;
pub const MAX_LABEL_LEN: usize = 63;

pub const MAX_CLIENTS: usize = 256;
pub const MAX_EXCLUDE_DOMAINS: usize = 512;

const CLIENTS: &str = "clients";
const EXCLUDE_DOMAINS: &str = "exclude_domains";

pub fn normalize_host(raw: &[u8]) -> Option<Box<str>> {
    if raw.is_empty() || raw.len() > MAX_NAME_LEN {
        return None;
    }
    let mut host = String::with_capacity(raw.len());
    let mut label = 0usize;
    let last = raw.len() - 1;
    for (index, &byte) in raw.iter().enumerate() {
        let ch = byte.to_ascii_lowercase();
        match ch {
            b'.' => {
                if label == 0 || index == last {
                    return None;
                }
                label = 0;
            }
            b'-' => {
                if label == 0 || index == last {
                    return None;
                }
                label += 1;
            }
            b'a'..=b'z' | b'0'..=b'9' | b'_' => {
                label += 1;
                if label > MAX_LABEL_LEN {
                    return None;
                }
            }
            _ => return None,
        }
        host.push(char::from(ch));
    }
    Some(host.into_boxed_str())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidExclusion(pub String);

impl fmt::Display for InvalidExclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a hostname: {:?}", self.0)
    }
}

impl std::error::Error for InvalidExclusion {}

#[derive(Debug, Clone, Default)]
pub struct ExclusionSet {
    hosts: HashSet<Box<str>>,
}

impl ExclusionSet {
    pub fn new<S: AsRef<str>>(user: &[S]) -> Result<Self, InvalidExclusion> {
        let mut hosts: HashSet<Box<str>> = HashSet::with_capacity(user.len());
        for entry in user {
            let raw = entry.as_ref().trim().trim_end_matches('.');
            let host = normalize_host(raw.as_bytes())
                .ok_or_else(|| InvalidExclusion(entry.as_ref().to_string()))?;
            hosts.insert(host);
        }
        Ok(Self { hosts })
    }

    pub fn len(&self) -> usize {
        self.hosts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    pub fn contains(&self, host: &str) -> bool {
        let host = normalize(host);
        let mut candidate: &str = &host;
        loop {
            if self.hosts.contains(candidate) {
                return true;
            }
            match candidate.find('.') {
                Some(dot) => candidate = &candidate[dot + 1..],
                None => return false,
            }
        }
    }
}

fn normalize(host: &str) -> Cow<'_, str> {
    let host = host.trim().trim_end_matches('.');
    if host.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(host.to_ascii_lowercase())
    } else {
        Cow::Borrowed(host)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum DocumentError {
    OverCap {
        list: &'static str,
        len: usize,
        cap: usize,
    },
    InvalidEntry {
        list: &'static str,
        index: usize,
        entry: String,
    },
    Duplicate {
        list: &'static str,
        index: usize,
        entry: String,
        duplicate_of: usize,
    },
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OverCap { list, len, cap } => write!(
                f,
                "{list}: {len} entries exceed the cap of {cap} by {}",
                len.saturating_sub(*cap)
            ),
            Self::InvalidEntry { list, index, entry } => {
                let expected = match *list {
                    CLIENTS => "an IP address or CIDR block",
                    _ => "a hostname",
                };
                write!(f, "{list}[{index}]: {entry:?} is not {expected}")
            }
            Self::Duplicate {
                list,
                index,
                entry,
                duplicate_of,
            } => write!(
                f,
                "{list}[{index}]: {entry:?} duplicates entry {duplicate_of} after normalization"
            ),
        }
    }
}

impl std::error::Error for DocumentError {}

#[derive(Debug, Clone, Default)]
pub struct InterceptionScope {
    clients: Box<[AllowedNet]>,
    exclusions: ExclusionSet,
}

impl InterceptionScope {
    pub fn intercepts(&self, ip: IpAddr) -> bool {
        self.clients.iter().any(|net| net.contains(ip))
    }

    pub fn excludes(&self, host: &str) -> bool {
        self.exclusions.contains(host)
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    pub fn exclusion_count(&self) -> usize {
        self.exclusions.len()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Active {
    pub document: InterceptionDocument,
    pub scope: InterceptionScope,
}

impl Active {
    pub fn compile(document: InterceptionDocument) -> Result<Active, DocumentError> {
        cap(CLIENTS, document.clients.len(), MAX_CLIENTS)?;
        cap(
            EXCLUDE_DOMAINS,
            document.exclude_domains.len(),
            MAX_EXCLUDE_DOMAINS,
        )?;

        let mut seen_clients: Vec<AllowedNet> = Vec::with_capacity(document.clients.len());
        let mut index_of_client: HashSet<AllowedNet> =
            HashSet::with_capacity(document.clients.len());
        for (index, entry) in document.clients.iter().enumerate() {
            let net =
                entry
                    .trim()
                    .parse::<AllowedNet>()
                    .map_err(|_| DocumentError::InvalidEntry {
                        list: CLIENTS,
                        index,
                        entry: entry.clone(),
                    })?;
            if !index_of_client.insert(net) {
                let duplicate_of = seen_clients
                    .iter()
                    .position(|earlier| *earlier == net)
                    .unwrap_or(0);
                return Err(DocumentError::Duplicate {
                    list: CLIENTS,
                    index,
                    entry: entry.clone(),
                    duplicate_of,
                });
            }
            seen_clients.push(net);
        }

        let mut hosts: HashSet<Box<str>> = HashSet::with_capacity(document.exclude_domains.len());
        let mut order: Vec<Box<str>> = Vec::with_capacity(document.exclude_domains.len());
        for (index, entry) in document.exclude_domains.iter().enumerate() {
            let raw = entry.trim().trim_end_matches('.');
            let host =
                normalize_host(raw.as_bytes()).ok_or_else(|| DocumentError::InvalidEntry {
                    list: EXCLUDE_DOMAINS,
                    index,
                    entry: entry.clone(),
                })?;
            if !hosts.insert(host.clone()) {
                let duplicate_of = order
                    .iter()
                    .position(|earlier| *earlier == host)
                    .unwrap_or(0);
                return Err(DocumentError::Duplicate {
                    list: EXCLUDE_DOMAINS,
                    index,
                    entry: entry.clone(),
                    duplicate_of,
                });
            }
            order.push(host);
        }

        Ok(Active {
            document,
            scope: InterceptionScope {
                clients: seen_clients.into_boxed_slice(),
                exclusions: ExclusionSet { hosts },
            },
        })
    }
}

fn cap(list: &'static str, len: usize, cap: usize) -> Result<(), DocumentError> {
    match len > cap {
        true => Err(DocumentError::OverCap { list, len, cap }),
        false => Ok(()),
    }
}

#[derive(Default)]
pub struct InterceptionState {
    active: ArcSwap<Active>,
}

impl fmt::Debug for InterceptionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active = self.active.load();
        f.debug_struct("InterceptionState")
            .field("clients", &active.scope.client_count())
            .field("exclusions", &active.scope.exclusion_count())
            .finish()
    }
}

impl InterceptionState {
    pub fn new(active: Active) -> Self {
        Self {
            active: ArcSwap::from_pointee(active),
        }
    }

    pub fn load(&self) -> arc_swap::Guard<Arc<Active>> {
        self.active.load()
    }

    pub fn current(&self) -> Arc<Active> {
        self.active.load_full()
    }

    pub fn store(&self, next: Arc<Active>) {
        self.active.store(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(clients: &[&str], exclude_domains: &[&str]) -> InterceptionDocument {
        InterceptionDocument {
            clients: clients.iter().map(|entry| entry.to_string()).collect(),
            exclude_domains: exclude_domains
                .iter()
                .map(|entry| entry.to_string())
                .collect(),
        }
    }

    fn set(user: &[&str]) -> ExclusionSet {
        ExclusionSet::new(user).unwrap()
    }

    #[test]
    fn an_exact_host_matches() {
        let set = set(&["bank.com"]);
        assert!(set.contains("bank.com"));
        assert!(set.contains("Bank.COM."));
    }

    #[test]
    fn a_parent_suffix_matches_its_subdomains() {
        let set = set(&["bank.com"]);
        assert!(set.contains("api.bank.com"));
        assert!(set.contains("deep.api.bank.com"));
    }

    #[test]
    fn a_lookalike_that_only_ends_with_the_name_does_not_match() {
        let set = set(&["bank.com"]);
        assert!(!set.contains("notbank.com"));
        assert!(!set.contains("bank.com.evil"));
        assert!(!set.contains("com"));
    }

    #[test]
    fn user_entries_are_normalized() {
        let set = set(&[" Bank.Example. ", "other.example"]);
        assert_eq!(set.len(), 2);
        assert!(set.contains("api.bank.example"));
    }

    #[test]
    fn a_malformed_entry_is_rejected_by_name() {
        for entry in [
            "",
            ".",
            "*.bank.example",
            "https://bank.example",
            "bank.example/",
            "bank.example:443",
            "-bad.example",
            "bad..example",
        ] {
            assert_eq!(
                ExclusionSet::new(&[entry]).err(),
                Some(InvalidExclusion(entry.to_string())),
                "{entry:?}"
            );
        }
    }

    #[test]
    fn the_empty_set_matches_nothing() {
        let set = ExclusionSet::default();
        assert!(set.is_empty());
        assert!(!set.contains("apple.com"));
        assert!(!set.contains("bank.example"));
    }

    #[test]
    fn normalize_host_lowercases_and_bounds_what_it_accepts() {
        assert_eq!(
            normalize_host(b"Example.COM").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            normalize_host(b"host_name.example").as_deref(),
            Some("host_name.example")
        );
        assert_eq!(
            normalize_host(b"a-b.example").as_deref(),
            Some("a-b.example")
        );
        for raw in [
            &b""[..],
            b".",
            b"bad..example",
            b"-bad.example",
            b"bad.example-",
            b"bank.example.",
            b"bank.example:443",
            b"*.bank.example",
        ] {
            assert!(
                normalize_host(raw).is_none(),
                "{:?}",
                String::from_utf8_lossy(raw)
            );
        }
        let long_label = vec![b'a'; MAX_LABEL_LEN + 1];
        assert!(normalize_host(&long_label).is_none());
        let long_name = vec![b'a'; MAX_NAME_LEN + 1];
        assert!(normalize_host(&long_name).is_none());
    }

    #[test]
    fn compile_rejects_over_cap_naming_list_and_overage() {
        let hosts: Vec<String> = (0..MAX_EXCLUDE_DOMAINS + 1)
            .map(|index| format!("h{index}.example"))
            .collect();
        let over_hosts = InterceptionDocument {
            clients: Vec::new(),
            exclude_domains: hosts,
        };
        assert_eq!(
            Active::compile(over_hosts).unwrap_err(),
            DocumentError::OverCap {
                list: EXCLUDE_DOMAINS,
                len: MAX_EXCLUDE_DOMAINS + 1,
                cap: MAX_EXCLUDE_DOMAINS,
            }
        );

        let clients: Vec<String> = (0..MAX_CLIENTS + 1)
            .map(|index| format!("10.0.{}.{}", index / 256, index % 256))
            .collect();
        let over_clients = InterceptionDocument {
            clients,
            exclude_domains: Vec::new(),
        };
        let error = Active::compile(over_clients).unwrap_err();
        assert_eq!(
            error,
            DocumentError::OverCap {
                list: CLIENTS,
                len: MAX_CLIENTS + 1,
                cap: MAX_CLIENTS,
            }
        );
        assert_eq!(
            error.to_string(),
            "clients: 257 entries exceed the cap of 256 by 1"
        );
    }

    #[test]
    fn compile_rejects_an_invalid_client_naming_its_index() {
        let error = Active::compile(document(
            &["192.168.88.1", "192.168.88.2", "10.0.0.0/8", "10.0.0.300"],
            &[],
        ))
        .unwrap_err();
        assert_eq!(
            error,
            DocumentError::InvalidEntry {
                list: CLIENTS,
                index: 3,
                entry: "10.0.0.300".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "clients[3]: \"10.0.0.300\" is not an IP address or CIDR block"
        );

        let error = Active::compile(document(&[], &["ok.example", "bad..example"])).unwrap_err();
        assert_eq!(
            error,
            DocumentError::InvalidEntry {
                list: EXCLUDE_DOMAINS,
                index: 1,
                entry: "bad..example".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "exclude_domains[1]: \"bad..example\" is not a hostname"
        );
    }

    #[test]
    fn compile_rejects_duplicates_after_normalization() {
        let error = Active::compile(document(
            &[],
            &["a.example", "Bank.ro", "b.example", "bank.ro."],
        ))
        .unwrap_err();
        assert_eq!(
            error,
            DocumentError::Duplicate {
                list: EXCLUDE_DOMAINS,
                index: 3,
                entry: "bank.ro.".to_string(),
                duplicate_of: 1,
            }
        );
        assert_eq!(
            error.to_string(),
            "exclude_domains[3]: \"bank.ro.\" duplicates entry 1 after normalization"
        );

        let error = Active::compile(document(
            &["192.168.88.10", "10.0.0.1", "192.168.88.10"],
            &[],
        ))
        .unwrap_err();
        assert_eq!(
            error,
            DocumentError::Duplicate {
                list: CLIENTS,
                index: 2,
                entry: "192.168.88.10".to_string(),
                duplicate_of: 0,
            }
        );
    }

    #[test]
    fn compile_accepts_a_host_inside_a_listed_cidr() {
        let active = Active::compile(document(&["192.168.88.0/24", "192.168.88.10"], &[])).unwrap();
        assert_eq!(active.scope.client_count(), 2);
        assert!(active.scope.intercepts("192.168.88.10".parse().unwrap()));
        assert!(active.scope.intercepts("192.168.88.99".parse().unwrap()));
        assert!(!active.scope.intercepts("192.168.89.1".parse().unwrap()));
    }

    #[test]
    fn compile_keeps_the_document_spelling_out_of_the_scope() {
        let active = Active::compile(document(&[" 192.168.88.10 "], &[" Bank.Example. "])).unwrap();
        assert_eq!(active.document.clients, vec![" 192.168.88.10 ".to_string()]);
        assert_eq!(
            active.document.exclude_domains,
            vec![" Bank.Example. ".to_string()]
        );
        assert!(active.scope.intercepts("192.168.88.10".parse().unwrap()));
        assert!(active.scope.excludes("api.bank.example"));
        assert_eq!(active.scope.exclusion_count(), 1);
    }

    #[test]
    fn document_error_serializes_to_the_documented_details() {
        assert_eq!(
            serde_json::to_value(DocumentError::OverCap {
                list: CLIENTS,
                len: 300,
                cap: 256
            })
            .unwrap(),
            serde_json::json!({"reason":"over_cap","list":"clients","len":300,"cap":256})
        );
        assert_eq!(
            serde_json::to_value(DocumentError::InvalidEntry {
                list: CLIENTS,
                index: 3,
                entry: "10.0.0.300".to_string()
            })
            .unwrap(),
            serde_json::json!({"reason":"invalid_entry","list":"clients","index":3,"entry":"10.0.0.300"})
        );
        assert_eq!(
            serde_json::to_value(DocumentError::Duplicate {
                list: EXCLUDE_DOMAINS,
                index: 7,
                entry: "Bank.ro.".to_string(),
                duplicate_of: 2
            })
            .unwrap(),
            serde_json::json!({"reason":"duplicate","list":"exclude_domains","index":7,"entry":"Bank.ro.","duplicate_of":2})
        );
    }

    #[test]
    fn a_held_guard_keeps_the_old_active_and_the_next_load_sees_the_new_one() {
        let state = InterceptionState::new(Active::compile(document(&["10.0.0.1"], &[])).unwrap());
        let guard = state.load();
        assert!(guard.scope.intercepts("10.0.0.1".parse().unwrap()));

        let next = Arc::new(Active::compile(document(&["10.0.0.2"], &[])).unwrap());
        state.store(Arc::clone(&next));

        assert!(guard.scope.intercepts("10.0.0.1".parse().unwrap()));
        assert!(!guard.scope.intercepts("10.0.0.2".parse().unwrap()));

        let fresh = state.load();
        assert!(fresh.scope.intercepts("10.0.0.2".parse().unwrap()));
        assert!(!fresh.scope.intercepts("10.0.0.1".parse().unwrap()));
        drop(guard);
        assert!(Arc::ptr_eq(&state.current(), &next));
    }

    #[test]
    fn lookup_cost_is_independent_of_list_size() {
        let hosts: Vec<String> = (0..MAX_EXCLUDE_DOMAINS)
            .map(|index| format!("h{index}.example"))
            .collect();
        let active = Active::compile(InterceptionDocument {
            clients: Vec::new(),
            exclude_domains: hosts,
        })
        .unwrap();
        assert_eq!(active.scope.exclusion_count(), MAX_EXCLUDE_DOMAINS);
        assert!(active.scope.excludes("deep.h0.example"));

        let started = std::time::Instant::now();
        for _ in 0..10_000 {
            assert!(!active.scope.excludes("ads.example.com"));
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }
}
