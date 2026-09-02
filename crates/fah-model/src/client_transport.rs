use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientTransport {
    Udp,
    Tcp,
    Dot,
    Doh,
}

impl ClientTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            ClientTransport::Udp => "udp",
            ClientTransport::Tcp => "tcp",
            ClientTransport::Dot => "dot",
            ClientTransport::Doh => "doh",
        }
    }
}

impl std::fmt::Display for ClientTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_spelling_is_lowercase_for_all_four_variants() {
        for (transport, spelling) in [
            (ClientTransport::Udp, "\"udp\""),
            (ClientTransport::Tcp, "\"tcp\""),
            (ClientTransport::Dot, "\"dot\""),
            (ClientTransport::Doh, "\"doh\""),
        ] {
            assert_eq!(serde_json::to_string(&transport).unwrap(), spelling);
            assert_eq!(
                serde_json::from_str::<ClientTransport>(spelling).unwrap(),
                transport
            );
            assert_eq!(format!("\"{transport}\""), spelling);
        }
    }

    #[test]
    fn an_unknown_spelling_is_rejected() {
        assert!(serde_json::from_str::<ClientTransport>("\"doq\"").is_err());
    }
}
