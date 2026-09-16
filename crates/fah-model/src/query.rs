use std::net::IpAddr;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// The DNS record type asked in a [`Query`] (CONTEXT.md: Query).
///
/// This is FastAdHunter's own lightweight type, not a DNS wire type — those
/// belong to `hickory-proto` and are used by `fah-dns` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryType {
    A,
    Aaaa,
    Https,
    Svcb,
    Cname,
    Mx,
    Txt,
    Ns,
    Ptr,
    Srv,
    Soa,
    Caa,
    Ds,
    Dnskey,
    Naptr,
    Other(u16),
}

/// One DNS question received from a client: domain, record type, client
/// source IP, timestamp (CONTEXT.md: Query).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub domain: String,
    pub qtype: QueryType,
    pub client_ip: IpAddr,
    pub timestamp: SystemTime,
}

impl Query {
    pub fn new(
        domain: impl Into<String>,
        qtype: QueryType,
        client_ip: IpAddr,
        timestamp: SystemTime,
    ) -> Self {
        Self {
            domain: domain.into(),
            qtype,
            client_ip,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_serde_roundtrip() {
        let query = Query::new(
            "ads.example.com",
            QueryType::A,
            IpAddr::from([192, 168, 1, 10]),
            SystemTime::UNIX_EPOCH,
        );
        let json = serde_json::to_string(&query).unwrap();
        let back: Query = serde_json::from_str(&json).unwrap();
        assert_eq!(query, back);
    }

    #[test]
    fn query_type_other_serde_roundtrip() {
        let qtype = QueryType::Other(65534);
        let json = serde_json::to_string(&qtype).unwrap();
        let back: QueryType = serde_json::from_str(&json).unwrap();
        assert_eq!(qtype, back);
    }

    #[test]
    fn every_named_query_type_survives_serde() {
        let named = [
            QueryType::A,
            QueryType::Aaaa,
            QueryType::Https,
            QueryType::Svcb,
            QueryType::Cname,
            QueryType::Mx,
            QueryType::Txt,
            QueryType::Ns,
            QueryType::Ptr,
            QueryType::Srv,
            QueryType::Soa,
            QueryType::Caa,
            QueryType::Ds,
            QueryType::Dnskey,
            QueryType::Naptr,
        ];
        for qtype in named {
            let json = serde_json::to_string(&qtype).unwrap();
            let back: QueryType = serde_json::from_str(&json).unwrap();
            assert_eq!(qtype, back);
        }
    }
}
