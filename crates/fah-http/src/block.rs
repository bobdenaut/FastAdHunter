//! What a blocked request gets back.
//!
//! Blocking is not one response, because a browser reacts to a failed subresource
//! very differently from a failed navigation:
//!
//! - A **subresource** (script, stylesheet, image, XHR, font, media, beacon)
//!   wants to fail *quietly*. Returning an error makes the page log a console
//!   error, retry, or run a fallback path that fetches the same thing another
//!   way; returning an empty success collapses the element and the page moves
//!   on. This is what ad blockers do and why blocked ads leave a gap rather
//!   than a broken-image icon.
//! - A **document** wants to fail *visibly*. A blank page with a 200 tells the
//!   user their browser is broken; a short page naming FastAdHunter and the
//!   rule tells them what happened and what to change.
//!
//! An **undetermined** type takes the visible form. `ResourceType::Unknown`
//! means the proxy could not tell, and a person seeing an explanation is
//! recoverable in a way of a silently blank fetch is not.

use bytes::Bytes;
use fah_model::{DecisiveRule, ResourceType};
use hyper::header::{HeaderValue, CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Response, StatusCode};

/// The shape of a block response for one resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStyle {
    /// 200 with an empty body of a matching `Content-Type` — the element
    /// collapses and nothing retries.
    EmptySuccess(&'static str),
    /// 204, for a request whose response was never going to be rendered.
    NoContent,
    /// A short 403 page naming FastAdHunter and the decisive rule.
    Explained,
}

impl BlockStyle {
    /// Which style a resource type gets. Exhaustive over [`ResourceType`] on
    /// purpose: a new type must make this decision rather than inherit one.
    pub fn for_resource(kind: ResourceType) -> Self {
        match kind {
            // A script that 403s runs the page's error path; an empty script
            // is a script that did nothing, which is the intent.
            ResourceType::Script => BlockStyle::EmptySuccess("application/javascript"),
            ResourceType::Stylesheet => BlockStyle::EmptySuccess("text/css"),
            // A 1×1 transparent GIF would also work, but an empty body with an
            // image type collapses the element just as well and ships no
            // payload at all — "blocked requests die cheaply" is the criterion.
            ResourceType::Image => BlockStyle::EmptySuccess("image/gif"),
            ResourceType::Font => BlockStyle::EmptySuccess("font/woff2"),
            ResourceType::Media => BlockStyle::EmptySuccess("video/mp4"),
            ResourceType::Object => BlockStyle::EmptySuccess("application/octet-stream"),
            // `fetch()`/XHR callers branch on `response.ok`, so a success with
            // an empty JSON-ish body is the least disruptive answer.
            ResourceType::XmlHttpRequest => BlockStyle::EmptySuccess("application/json"),
            // A beacon's response is never read. 204 says "received, nothing
            // to render" and costs no body at all.
            ResourceType::Ping | ResourceType::WebSocket => BlockStyle::NoContent,
            // Documents, frames, and anything we could not classify: explain.
            ResourceType::Document
            | ResourceType::Subdocument
            | ResourceType::Other
            | ResourceType::Unknown => BlockStyle::Explained,
        }
    }

    /// The body this style sends. Empty for everything but [`Self::Explained`].
    fn body(self, rule: Option<&DecisiveRule>) -> Bytes {
        match self {
            BlockStyle::EmptySuccess(_) | BlockStyle::NoContent => Bytes::new(),
            BlockStyle::Explained => Bytes::from(explanation(rule)),
        }
    }

    fn status(self) -> StatusCode {
        match self {
            BlockStyle::EmptySuccess(_) => StatusCode::OK,
            BlockStyle::NoContent => StatusCode::NO_CONTENT,
            BlockStyle::Explained => StatusCode::FORBIDDEN,
        }
    }

    fn content_type(self) -> Option<&'static str> {
        match self {
            BlockStyle::EmptySuccess(mime) => Some(mime),
            BlockStyle::NoContent => None,
            BlockStyle::Explained => Some("text/html; charset=utf-8"),
        }
    }
}

/// Builds the response for a blocked request.
///
/// `no-store` on every one of them: a block is a policy decision that can change
/// the moment a list refreshes or a rule is added, and a cached block would
/// outlive the rule that caused it with no way for the user to tell.
pub fn response(kind: ResourceType, rule: Option<&DecisiveRule>) -> Response<Bytes> {
    let style = BlockStyle::for_resource(kind);
    let mut response = Response::new(style.body(rule));
    *response.status_mut() = style.status();
    let headers = response.headers_mut();
    if let Some(mime) = style.content_type() {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(mime));
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The 403 page. Deliberately tiny and dependency-free — no template engine, no
/// CSS framework, nothing fetched from anywhere (a blocked page that tries to
/// load a stylesheet through the proxy that just blocked it is a bad look).
fn explanation(rule: Option<&DecisiveRule>) -> String {
    let mut page = String::with_capacity(512);
    page.push_str(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Blocked by FastAdHunter</title></head>\
         <body style=\"font:16px/1.5 system-ui,sans-serif;margin:0;\
         display:grid;place-items:center;min-height:100vh\">\
         <main style=\"max-width:34rem;padding:2rem\">\
         <h1 style=\"font-size:1.25rem;margin:0 0 .5rem\">Blocked by FastAdHunter</h1>\
         <p style=\"margin:0 0 1rem\">This request was refused by a filtering rule.</p>",
    );
    if let Some(rule) = rule {
        page.push_str("<p style=\"margin:0\"><strong>Rule:</strong> <code>");
        escape_into(&mut page, &rule.rule);
        page.push_str("</code><br><strong>List:</strong> <code>");
        escape_into(&mut page, &rule.list);
        page.push_str("</code></p>");
    }
    page.push_str("</main></body></html>");
    page
}

/// Escapes rule text into HTML. **Not optional:** rule text and list names come
/// from subscribed lists, which are third-party input, and this page is
/// rendered by a browser. A rule containing `<script>` must appear as text.
fn escape_into(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn rule() -> DecisiveRule {
        DecisiveRule::new(Arc::from("easylist"), "||ads.example.com^".to_string())
    }

    #[test]
    fn a_blocked_script_gets_an_empty_success() {
        let response = response(ResourceType::Script, Some(&rule()));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "application/javascript");
        assert!(response.body().is_empty(), "a block must ship no payload");
    }

    #[test]
    fn a_blocked_image_and_stylesheet_collapse_quietly() {
        for (kind, mime) in [
            (ResourceType::Image, "image/gif"),
            (ResourceType::Stylesheet, "text/css"),
        ] {
            let response = response(kind, None);
            assert_eq!(response.status(), StatusCode::OK, "{kind:?}");
            assert_eq!(response.headers()[CONTENT_TYPE], mime);
            assert!(response.body().is_empty());
        }
    }

    #[test]
    fn a_blocked_beacon_is_no_content() {
        let response = response(ResourceType::Ping, None);
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(response.body().is_empty());
        assert!(
            !response.headers().contains_key(CONTENT_TYPE),
            "204 carries no body, so it must not claim a type"
        );
    }

    #[test]
    fn a_blocked_document_gets_a_page_naming_the_rule_and_list() {
        let response = response(ResourceType::Document, Some(&rule()));
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = String::from_utf8(response.body().to_vec()).unwrap();
        assert!(body.contains("FastAdHunter"));
        assert!(body.contains("||ads.example.com^"), "{body}");
        assert!(body.contains("easylist"), "{body}");
    }

    /// An undetermined type takes the visible form: a person who can read what
    /// happened can act on it, a silently blank fetch leaves nothing to act on.
    #[test]
    fn an_undetermined_type_explains_rather_than_blanking() {
        assert_eq!(
            BlockStyle::for_resource(ResourceType::Unknown),
            BlockStyle::Explained
        );
    }

    /// Rule text is third-party input rendered by a browser.
    #[test]
    fn rule_text_is_escaped_into_the_page() {
        let hostile = DecisiveRule::new(
            Arc::from("<script>alert(1)</script>"),
            "||x.com^<img src=x onerror=alert(1)>".to_string(),
        );
        let response = response(ResourceType::Document, Some(&hostile));
        let body = String::from_utf8(response.body().to_vec()).unwrap();
        assert!(!body.contains("<script>alert"), "{body}");
        assert!(!body.contains("<img src=x"), "{body}");
        assert!(body.contains("&lt;script&gt;"), "{body}");
    }

    /// A cached block would outlive the rule that caused it.
    #[test]
    fn every_block_forbids_caching() {
        for kind in [
            ResourceType::Script,
            ResourceType::Image,
            ResourceType::Ping,
            ResourceType::Document,
            ResourceType::Unknown,
        ] {
            assert_eq!(
                response(kind, None).headers()[CACHE_CONTROL],
                "no-store",
                "{kind:?}"
            );
        }
    }

    /// No resource type may fall through without a decision.
    #[test]
    fn every_resource_type_has_a_block_style() {
        for kind in [
            ResourceType::Document,
            ResourceType::Subdocument,
            ResourceType::Script,
            ResourceType::Stylesheet,
            ResourceType::Image,
            ResourceType::Font,
            ResourceType::Media,
            ResourceType::XmlHttpRequest,
            ResourceType::WebSocket,
            ResourceType::Ping,
            ResourceType::Object,
            ResourceType::Other,
            ResourceType::Unknown,
        ] {
            let response = response(kind, Some(&rule()));
            assert!(
                response.status().is_success() || response.status() == StatusCode::FORBIDDEN,
                "{kind:?} produced {}",
                response.status()
            );
        }
    }
}
