use std::net::IpAddr;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// A device on the network, identified by the source IP of its queries
/// (CONTEXT.md: Client).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Client {
    pub ip: IpAddr,
    pub name: Option<String>,
    pub first_seen: SystemTime,
    pub last_seen: SystemTime,
}

impl Client {
    pub fn new(ip: IpAddr, first_seen: SystemTime) -> Self {
        Self {
            ip,
            name: None,
            first_seen,
            last_seen: first_seen,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_serde_roundtrip() {
        let client = Client {
            name: Some("laptop".to_string()),
            ..Client::new(IpAddr::from([10, 0, 0, 5]), SystemTime::UNIX_EPOCH)
        };
        let json = serde_json::to_string(&client).unwrap();
        let back: Client = serde_json::from_str(&json).unwrap();
        assert_eq!(client, back);
    }
}
