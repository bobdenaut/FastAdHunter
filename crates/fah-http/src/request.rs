//! Turning a request head into the Rule Engine's request model.
//!
//! `fah_model::HttpRequest` is pure data with every field already extracted —
//! "deriving the resource type from the wire and the document host from
//! `Referer` is the HTTP pipeline's job, not the engine's" (RULE_ENGINE.md
//! §HTTP matching). This module is that job, and it is the only place that
//! reads a header for anything other than forwarding it.
//!
//! Two of its rules were written by the p2-03 review rather than discovered
//! here, and both fail silently if broken — see the task file's "Inherited from
//! the p2-03 review" section.

use fah_model::ResourceType;
use hyper::header::{HeaderMap, ACCEPT, REFERER};
use hyper::{Request, Uri};

/// `Sec-Fetch-Dest`, which no `hyper::header` constant covers.
const SEC_FETCH_DEST: &str = "sec-fetch-dest";

/// Strips a `:port` suffix from an authority, leaving the bare host.
///
/// **Load-bearing, and silent when wrong.** `fah_model::HttpRequest`'s `host`
/// and `document_host` are documented "without port" and nothing in the matcher
/// enforces it: with a port attached, `registrable()` reduces
/// `news.org:8080` to `com:8080`, which inverts the third-party test, makes
/// `$domain=news.org` stop applying, and leaves the domain-tier walk matching
/// nothing at all. The *URL* keeps its port — `||example.com^` relies on `:`
/// being a separator — so this is applied to these two fields only.
///
/// IPv6 literals arrive bracketed (`[::1]:80`); the bracket, not the last
/// colon, is what separates address from port.
fn without_port(authority: &str) -> &str {
    if let Some(end) = authority.rfind(']') {
        return authority[..=end]
            .trim_start_matches('[')
            .trim_end_matches(']');
    }
    match authority.rsplit_once(':') {
        // Exactly one colon and digits after it is a port. A *bare* IPv6
        // address also ends in `:<digits>` — `2606:2800::1` — so the remaining
        // host must be colon-free, or this truncates an address into a host
        // that was never asked for. (`claim.rs` unbrackets IPv6 before storing
        // it, so bare ones do reach here.)
        Some((host, port))
            if !port.is_empty()
                && port.bytes().all(|byte| byte.is_ascii_digit())
                && !host.contains(':') =>
        {
            host
        }
        _ => authority,
    }
}

/// The document host a request was made from, per `Referer`.
///
/// `None` when there is no referer, which the matcher reads as first-party —
/// "there is no other document for it to be third to".
pub fn document_host(headers: &HeaderMap) -> Option<&str> {
    let referer = headers.get(REFERER)?.to_str().ok()?;
    // Hand-parsed rather than through `Uri`: a `Referer` is attacker-shaped
    // input and `Uri::from_str` allocates. Everything before the authority is
    // the scheme, everything after is the path.
    let after_scheme = referer.split_once("://").map_or(referer, |(_, rest)| rest);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    // A `Referer` may carry userinfo; the host is what follows it.
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = without_port(authority);
    (!host.is_empty()).then_some(host)
}

/// The host the request is addressed to, without its port.
pub fn request_host(claim_host: &str) -> &str {
    without_port(claim_host)
}

/// What the request is fetching.
///
/// **Order matters, and so does declining to guess.** `Sec-Fetch-Dest` is the
/// browser stating its own intent and is trusted first; `Accept` is a
/// negotiation hint and is read only for the types it names unambiguously; the
/// path extension is last and weakest. When none of them speak the answer is
/// [`ResourceType::Unknown`], which is not a failure mode but a documented
/// verdict input: a type-restricted **block** declines it, while a
/// type-restricted **exception** applies (p2-03 review), so guessing here would
/// be the only way to over-block.
pub fn resource_type(headers: &HeaderMap, uri: &Uri) -> ResourceType {
    if let Some(kind) = from_sec_fetch_dest(headers) {
        return kind;
    }
    if let Some(kind) = from_accept(headers) {
        return kind;
    }
    from_extension(uri.path())
}

/// <https://fetch.spec.whatwg.org/#concept-request-destination>, mapped onto
/// the adblock option vocabulary. The empty destination means "a fetch with no
/// specific destination" — an XHR or `fetch()` — which is exactly
/// `$xmlhttprequest`.
fn from_sec_fetch_dest(headers: &HeaderMap) -> Option<ResourceType> {
    let value = headers.get(SEC_FETCH_DEST)?.to_str().ok()?.trim();
    Some(match value {
        "document" => ResourceType::Document,
        "iframe" | "frame" => ResourceType::Subdocument,
        "script" | "serviceworker" | "sharedworker" | "worker" => ResourceType::Script,
        "style" => ResourceType::Stylesheet,
        "image" => ResourceType::Image,
        "font" => ResourceType::Font,
        "audio" | "video" | "track" => ResourceType::Media,
        "empty" => ResourceType::XmlHttpRequest,
        "object" | "embed" => ResourceType::Object,
        "report" => ResourceType::Ping,
        // A destination the spec names and the option vocabulary does not.
        // `Other` is a real adblock type, so this is an answer, not a shrug.
        "manifest" | "paintworklet" | "audioworklet" | "xslt" => ResourceType::Other,
        _ => return None,
    })
}

/// `Accept` is a *preference list*, so it is only read where the first-listed
/// type is unambiguous. `*/*` and `text/html,...` from a navigation both exist,
/// and treating a wildcard as evidence is how a wrong guess gets made.
fn from_accept(headers: &HeaderMap) -> Option<ResourceType> {
    let value = headers.get(ACCEPT)?.to_str().ok()?;
    let first = value.split(',').next()?.split(';').next()?.trim();
    let (kind, subtype) = first.split_once('/')?;
    Some(match (kind, subtype) {
        ("image", _) => ResourceType::Image,
        ("font", _) => ResourceType::Font,
        ("audio", _) | ("video", _) => ResourceType::Media,
        ("text", "css") => ResourceType::Stylesheet,
        ("text", "javascript") | ("application", "javascript") => ResourceType::Script,
        ("text", "html") | ("application", "xhtml+xml") => ResourceType::Document,
        _ => return None,
    })
}

/// Last resort, and the weakest: an extension is a naming convention, not a
/// declaration. Only extensions that are unambiguous in practice are mapped.
fn from_extension(path: &str) -> ResourceType {
    let file = path.rsplit('/').next().unwrap_or(path);
    let Some((_, extension)) = file.rsplit_once('.') else {
        return ResourceType::Unknown;
    };
    let mut lowered = [0u8; 8];
    let bytes = extension.as_bytes();
    if bytes.is_empty() || bytes.len() > lowered.len() {
        return ResourceType::Unknown;
    }
    for (slot, byte) in lowered.iter_mut().zip(bytes) {
        *slot = byte.to_ascii_lowercase();
    }
    match &lowered[..bytes.len()] {
        b"js" | b"mjs" => ResourceType::Script,
        b"css" => ResourceType::Stylesheet,
        b"png" | b"jpg" | b"jpeg" | b"gif" | b"webp" | b"svg" | b"ico" | b"bmp" | b"avif" => {
            ResourceType::Image
        }
        b"woff" | b"woff2" | b"ttf" | b"otf" | b"eot" => ResourceType::Font,
        b"mp3" | b"mp4" | b"webm" | b"ogg" | b"wav" | b"m4a" | b"mov" | b"m3u8" => {
            ResourceType::Media
        }
        b"html" | b"htm" => ResourceType::Document,
        b"json" => ResourceType::XmlHttpRequest,
        _ => ResourceType::Unknown,
    }
}

/// The absolute URL the matcher matches patterns against.
///
/// Rebuilt rather than taken from the request line: a transparent proxy
/// receives an origin-form target (`/path?query`), and URL patterns are written
/// against the whole address. The port is **kept** here — `||example.com^`
/// depends on `:` being a separator — which is why [`without_port`] is applied
/// to the host fields only.
pub fn absolute_url<B>(request: &Request<B>, authority: &str) -> String {
    let path_and_query = request.uri().path_and_query().map_or("/", |pq| pq.as_str());
    let mut url = String::with_capacity(7 + authority.len() + path_and_query.len());
    url.push_str("http://");
    url.push_str(authority);
    url.push_str(path_and_query);
    url
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    // ─── Port stripping (p2-03 review) ────────────────────────────────────

    #[test]
    fn a_port_is_stripped_from_the_host() {
        assert_eq!(request_host("example.com:8080"), "example.com");
        assert_eq!(request_host("example.com"), "example.com");
    }

    /// A bare IPv6 address is all colons; truncating at the last one would
    /// invent a host that was never asked for.
    #[test]
    fn an_ipv6_literal_survives_port_stripping() {
        assert_eq!(request_host("[2606:2800::1]:80"), "2606:2800::1");
        assert_eq!(request_host("[::1]"), "::1");
        assert_eq!(request_host("2606:2800::1"), "2606:2800::1");
    }

    /// `:` followed by something that is not a port is part of the host, not a
    /// separator.
    #[test]
    fn a_colon_that_is_not_a_port_is_left_alone() {
        assert_eq!(request_host("example.com:"), "example.com:");
        assert_eq!(request_host("example.com:http"), "example.com:http");
    }

    // ─── Referer → document host ──────────────────────────────────────────

    #[test]
    fn the_document_host_comes_from_the_referer_without_its_port() {
        let map = headers(&[("referer", "http://news.org:8080/article/1")]);
        assert_eq!(document_host(&map), Some("news.org"));
    }

    #[test]
    fn a_referer_with_no_path_or_scheme_still_yields_a_host() {
        assert_eq!(
            document_host(&headers(&[("referer", "https://news.org")])),
            Some("news.org")
        );
        assert_eq!(
            document_host(&headers(&[("referer", "news.org/x")])),
            Some("news.org")
        );
    }

    #[test]
    fn userinfo_in_a_referer_is_not_mistaken_for_the_host() {
        let map = headers(&[("referer", "http://user:pass@news.org/x")]);
        assert_eq!(document_host(&map), Some("news.org"));
    }

    #[test]
    fn no_referer_means_no_document_host() {
        assert_eq!(document_host(&HeaderMap::new()), None);
        assert_eq!(document_host(&headers(&[("referer", "")])), None);
    }

    // ─── Resource type ────────────────────────────────────────────────────

    #[test]
    fn sec_fetch_dest_is_trusted_over_everything_else() {
        let map = headers(&[
            ("sec-fetch-dest", "script"),
            ("accept", "image/webp,image/*"),
        ]);
        let uri: Uri = "/a/b.png".parse().unwrap();
        assert_eq!(resource_type(&map, &uri), ResourceType::Script);
    }

    #[test]
    fn sec_fetch_dest_empty_is_a_fetch_not_an_unknown() {
        let map = headers(&[("sec-fetch-dest", "empty")]);
        let uri: Uri = "/api/collect".parse().unwrap();
        assert_eq!(resource_type(&map, &uri), ResourceType::XmlHttpRequest);
    }

    #[test]
    fn accept_is_used_when_it_names_one_type_unambiguously() {
        let uri: Uri = "/a/b".parse().unwrap();
        for (accept, expected) in [
            ("image/webp,image/apng,*/*;q=0.8", ResourceType::Image),
            ("text/css,*/*;q=0.1", ResourceType::Stylesheet),
            ("text/html,application/xhtml+xml", ResourceType::Document),
            ("font/woff2,*/*", ResourceType::Font),
        ] {
            assert_eq!(
                resource_type(&headers(&[("accept", accept)]), &uri),
                expected,
                "{accept}"
            );
        }
    }

    /// `*/*` is the absence of a preference. Reading it as evidence is exactly
    /// the wrong guess `Unknown` exists to avoid.
    #[test]
    fn a_wildcard_accept_is_not_evidence() {
        let map = headers(&[("accept", "*/*")]);
        let uri: Uri = "/collect".parse().unwrap();
        assert_eq!(resource_type(&map, &uri), ResourceType::Unknown);
    }

    #[test]
    fn the_path_extension_is_the_last_resort() {
        let map = HeaderMap::new();
        for (path, expected) in [
            ("/assets/app.7f3c2b.js", ResourceType::Script),
            ("/a/b/style.CSS", ResourceType::Stylesheet),
            ("/img/header.jpg?w=1200", ResourceType::Image),
            ("/f/inter.woff2", ResourceType::Font),
            ("/v/clip.mp4", ResourceType::Media),
            ("/index.html", ResourceType::Document),
        ] {
            let uri: Uri = path.parse().unwrap();
            assert_eq!(resource_type(&map, &uri), expected, "{path}");
        }
    }

    /// The criterion the task names: a request with no hints stays `Unknown`
    /// rather than being guessed into a type.
    #[test]
    fn a_request_with_no_hints_stays_unknown() {
        let uri: Uri = "/pagead/collect".parse().unwrap();
        assert_eq!(
            resource_type(&HeaderMap::new(), &uri),
            ResourceType::Unknown
        );
        // A dot in a path segment that is not an extension must not fool it.
        let uri: Uri = "/v1.2/collect".parse().unwrap();
        assert_eq!(
            resource_type(&HeaderMap::new(), &uri),
            ResourceType::Unknown
        );
    }

    // ─── URL reconstruction ───────────────────────────────────────────────

    #[test]
    fn the_absolute_url_keeps_the_port_the_host_field_drops() {
        let request = Request::builder().uri("/a/b?c=d").body(()).unwrap();
        assert_eq!(
            absolute_url(&request, "example.com:8080"),
            "http://example.com:8080/a/b?c=d"
        );
    }

    #[test]
    fn an_empty_target_becomes_a_root_path() {
        let request = Request::builder().uri("/").body(()).unwrap();
        assert_eq!(absolute_url(&request, "example.com"), "http://example.com/");
    }
}
