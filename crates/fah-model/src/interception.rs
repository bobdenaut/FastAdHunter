use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct InterceptionDocument {
    pub clients: Vec<String>,
    pub exclude_domains: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_object_is_the_empty_document() {
        let document: InterceptionDocument = serde_json::from_str("{}").unwrap();
        assert_eq!(document, InterceptionDocument::default());
    }

    #[test]
    fn both_lists_round_trip() {
        let document = InterceptionDocument {
            clients: vec!["192.168.88.10".to_string(), "192.168.88.0/24".to_string()],
            exclude_domains: vec!["bank.example".to_string()],
        };
        let text = serde_json::to_string(&document).unwrap();
        assert_eq!(
            serde_json::from_str::<InterceptionDocument>(&text).unwrap(),
            document
        );
    }

    #[test]
    fn an_unknown_key_is_rejected() {
        let error = serde_json::from_str::<InterceptionDocument>(r#"{"clients":[],"extra":1}"#)
            .expect_err("deny_unknown_fields");
        assert!(error.to_string().contains("extra"), "{error}");
    }

    #[test]
    fn a_missing_key_is_an_empty_list() {
        let document: InterceptionDocument =
            serde_json::from_str(r#"{"clients":["10.0.0.1"]}"#).unwrap();
        assert_eq!(document.clients, vec!["10.0.0.1".to_string()]);
        assert!(document.exclude_domains.is_empty());
    }
}
