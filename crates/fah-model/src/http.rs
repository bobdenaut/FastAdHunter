use serde::{Deserialize, Serialize};

/// What a request is fetching, as the adblock `$script` / `$image` / … options
/// name it (RULE_ENGINE.md: HTTP matching).
///
/// This is the HTTP counterpart of [`crate::QueryType`]: the piece of request
/// context a rule can restrict itself to. Deriving it from the wire (`Accept`,
/// `Sec-Fetch-Dest`, the path's extension) is the proxy's job — this type is
/// the vocabulary, not the inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    /// The top-level page (`$document`).
    Document,
    /// A nested frame (`$subdocument`, `$frame`).
    Subdocument,
    Script,
    /// `$stylesheet` / `$css`.
    Stylesheet,
    Image,
    Font,
    /// Audio or video (`$media`).
    Media,
    /// `$xmlhttprequest` / `$xhr`.
    XmlHttpRequest,
    WebSocket,
    /// `$ping` / `$beacon`.
    Ping,
    Object,
    /// `$other` — a type the option vocabulary names but does not single out.
    Other,
    /// The proxy could not tell. **Not** an adblock option: no
    /// type-restricted rule matches it, positively or negatively, so an
    /// undetermined type can neither over-block via `$script` nor via
    /// `~script`. Rules carrying no type restriction still apply.
    Unknown,
}

/// One HTTP request as the Rule Engine sees it (CONTEXT.md: HTTP Request).
///
/// Borrowed, not owned: this is built per request on the proxy's hot path and
/// handed straight to [`crate::QueryType`]'s HTTP counterpart entry point, so
/// it must cost no allocation. Nothing here is parsed or derived — the proxy
/// supplies each field already extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpRequest<'a> {
    /// Absolute URL, scheme included (`http://host/path?query`). What URL
    /// patterns are matched against.
    pub url: &'a str,
    /// Host the request is addressed to, without port. What domain rules and
    /// the third-party test are judged on.
    pub host: &'a str,
    /// Request method, uppercase (`GET`, `POST`, …) — for `$method=`.
    pub method: &'a str,
    pub resource_type: ResourceType,
    /// Host of the document that caused the request, from `Referer`. `None`
    /// when there is no referer, which makes the request first-party by
    /// definition — there is no other document for it to be third to.
    pub document_host: Option<&'a str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_type_serde_roundtrip() {
        for kind in [
            ResourceType::Document,
            ResourceType::XmlHttpRequest,
            ResourceType::Unknown,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: ResourceType = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn resource_type_serializes_in_snake_case() {
        assert_eq!(
            serde_json::to_string(&ResourceType::XmlHttpRequest).unwrap(),
            "\"xml_http_request\""
        );
    }
}
