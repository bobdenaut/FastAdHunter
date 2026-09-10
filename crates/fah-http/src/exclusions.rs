use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;

/// Hosts that are never HTTPS-intercepted by default.
///
/// These are product safety exclusions for platform infrastructure,
/// update/notification services, messaging/payment services, and
/// banking/financial applications known to be incompatible with TLS MITM.
///
/// Exclusion means SPLICE, not BLOCK: DNS/SNI policy still applies.
pub const BASELINE_EXCLUSIONS: &[&str] = &[
    "apple.com",
    "icloud.com",
    "mzstatic.com",
    "apple-cloudkit.com",
    "android.com",
    "googleapis.com",
    "play.google.com",
    "android.clients.google.com",
    "clients.google.com",
    "mtalk.google.com",
    "gvt1.com",
    "gvt2.com",
    "gvt3.com",
    "windowsupdate.com",
    "update.microsoft.com",
    "delivery.mp.microsoft.com",
    "login.microsoftonline.com",
    "notify.windows.com",
    "wns.windows.com",
    "whatsapp.net",
    "whatsapp.com",
    "signal.org",
    "paypal.com",
    "revolut.com",
    "wise.com",
    "n26.com",
    "bancatransilvania.ro",
    "btrl.ro",
    "ing.ro",
    "bcr.ro",
    "george.ro",
    "brd.ro",
    "raiffeisen.ro",
    "unicredit.ro",
];

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
        let mut hosts: HashSet<Box<str>> =
            BASELINE_EXCLUSIONS.iter().copied().map(Box::from).collect();
        for entry in user {
            let raw = entry.as_ref().trim().trim_end_matches('.');
            let host = crate::sni::normalize(raw.as_bytes())
                .ok_or_else(|| InvalidExclusion(entry.as_ref().to_string()))?;
            hosts.insert(host);
        }
        Ok(Self { hosts })
    }

    pub fn empty() -> Self {
        Self::default()
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_baseline_is_present_with_an_empty_user_list() {
        let set = ExclusionSet::new::<&str>(&[]).unwrap();
        assert_eq!(set.len(), BASELINE_EXCLUSIONS.len());
        for host in BASELINE_EXCLUSIONS {
            assert!(set.contains(host), "{host}");
        }
        assert!(set.contains("push.apple.com"));
        assert!(!set.contains("ads.example.com"));
    }

    #[test]
    fn user_entries_are_normalized() {
        let set = set(&[" Bank.Example. ", "bank.example"]);
        assert_eq!(set.len(), BASELINE_EXCLUSIONS.len() + 1);
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
    fn every_baseline_entry_is_a_valid_hostname() {
        for host in BASELINE_EXCLUSIONS {
            assert!(crate::sni::normalize(host.as_bytes()).is_some(), "{host}");
        }
    }

    #[test]
    fn the_empty_set_matches_nothing() {
        let set = ExclusionSet::empty();
        assert!(set.is_empty());
        assert!(!set.contains("apple.com"));
    }
}
