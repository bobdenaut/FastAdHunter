use std::path::Path;

use axum::http::header::{CACHE_CONTROL, VARY};
use axum::http::{HeaderValue, Response, StatusCode};
use axum::Router;
use tower::Layer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::{MakeHeaderValue, SetResponseHeader, SetResponseHeaderLayer};

const ROOT: &str = "/web";

const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";
const ACCEPT_ENCODING: &str = "accept-encoding";
const SHELL: &str = "index.html";
const ASSETS: &str = "assets";
const READ_CHUNK: usize = 8 * 1024;

pub fn mounted() -> Router {
    router(Path::new(ROOT))
}

pub fn check_root() {
    report_missing_shell(Path::new(ROOT));
}

fn router(root: &Path) -> Router {
    let assets = ServeDir::new(root.join(ASSETS))
        .precompressed_br()
        .precompressed_gzip()
        .with_buf_chunk_size(READ_CHUNK);

    let shell = ServeDir::new(root)
        .precompressed_br()
        .precompressed_gzip()
        .with_buf_chunk_size(READ_CHUNK)
        .fallback(
            ServeFile::new(root.join(SHELL))
                .precompressed_br()
                .precompressed_gzip()
                .with_buf_chunk_size(READ_CHUNK),
        );

    Router::new()
        .nest_service("/assets", cached(IMMUTABLE, assets))
        .fallback_service(cached(REVALIDATE, shell))
        .layer(SetResponseHeaderLayer::if_not_present(
            VARY,
            HeaderValue::from_static(ACCEPT_ENCODING),
        ))
}

fn report_missing_shell(root: &Path) {
    if shell_missing(root) {
        tracing::error!(
            path = %root.join(SHELL).display(),
            "web UI missing: the dashboard will not load. /web is image content, \
             not a volume: a volume mounted over it hides the bundle the image ships"
        );
    }
}

fn shell_missing(root: &Path) -> bool {
    !root.join(SHELL).is_file()
}

#[derive(Clone, Copy)]
struct CacheControl(&'static str);

impl<B> MakeHeaderValue<Response<B>> for CacheControl {
    fn make_header_value(&mut self, response: &Response<B>) -> Option<HeaderValue> {
        let cacheable =
            response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED;
        cacheable.then(|| HeaderValue::from_static(self.0))
    }
}

fn cached<S>(value: &'static str, service: S) -> SetResponseHeader<S, CacheControl> {
    SetResponseHeaderLayer::overriding(CACHE_CONTROL, CacheControl(value)).layer(service)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use super::*;

    const SECRET: &str = "outside-the-web-root";
    const HASH: &str = "a1b2c3d4";

    fn fixture() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("web");
        let assets = root.join(ASSETS);
        std::fs::create_dir_all(&assets).unwrap();

        std::fs::write(dir.path().join("secret.txt"), SECRET).unwrap();
        std::fs::write(
            root.join(SHELL),
            "<!doctype html><title>FastAdHunter</title><p>fixture bundle 0.2.20",
        )
        .unwrap();
        std::fs::write(root.join(format!("{SHELL}.br")), "shell-brotli").unwrap();
        std::fs::write(root.join(format!("{SHELL}.gz")), "shell-gzip").unwrap();
        std::fs::write(root.join("favicon.ico"), [0u8; 4]).unwrap();

        std::fs::write(assets.join(format!("app.{HASH}.js")), "export const ok=1;").unwrap();
        std::fs::write(assets.join(format!("app.{HASH}.js.br")), "js-brotli").unwrap();
        std::fs::write(assets.join(format!("app.{HASH}.js.gz")), "js-gzip").unwrap();
        std::fs::write(assets.join(format!("app.{HASH}.css")), ":root{}").unwrap();
        std::fs::write(assets.join(format!("app.{HASH}.css.br")), "css-brotli").unwrap();
        std::fs::write(assets.join(format!("app.{HASH}.css.gz")), "css-gzip").unwrap();
        std::fs::write(assets.join(format!("sprite.{HASH}.svg")), "<svg/>").unwrap();
        std::fs::write(assets.join(format!("font.{HASH}.woff2")), [0u8; 4]).unwrap();
        std::fs::write(assets.join(format!("meta.{HASH}.json")), "{}").unwrap();

        (dir, root)
    }

    async fn send(root: &Path, uri: &str, headers: &[(&str, &str)]) -> Response<Body> {
        let mut builder = Request::builder().uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        router(root)
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn get(root: &Path, uri: &str) -> Response<Body> {
        send(root, uri, &[]).await
    }

    fn header(response: &Response<Body>, name: &str) -> String {
        response
            .headers()
            .get(name)
            .map(|value| value.to_str().unwrap().to_string())
            .unwrap_or_default()
    }

    async fn body(response: Response<Body>) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[tokio::test]
    async fn the_root_serves_the_shell() {
        let (_dir, root) = fixture();
        let response = get(&root, "/").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header(&response, "content-type"), "text/html");
        assert!(body(response).await.contains("fixture bundle"));
    }

    #[tokio::test]
    async fn the_shell_is_revalidatable_and_never_immutable() {
        let (_dir, root) = fixture();
        let response = get(&root, "/").await;

        assert_eq!(header(&response, "cache-control"), REVALIDATE);
        assert!(!header(&response, "cache-control").contains("immutable"));
    }

    #[tokio::test]
    async fn hashed_assets_are_immutable() {
        let (_dir, root) = fixture();
        let response = get(&root, &format!("/assets/app.{HASH}.js")).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header(&response, "cache-control"), IMMUTABLE);
    }

    #[tokio::test]
    async fn mime_types_cover_the_whole_bundle() {
        let (_dir, root) = fixture();
        let expected = [
            ("/", "text/html"),
            ("/favicon.ico", "image/x-icon"),
            (&format!("/assets/app.{HASH}.js"), "text/javascript"),
            (&format!("/assets/app.{HASH}.css"), "text/css"),
            (&format!("/assets/sprite.{HASH}.svg"), "image/svg+xml"),
            (&format!("/assets/font.{HASH}.woff2"), "font/woff2"),
            (&format!("/assets/meta.{HASH}.json"), "application/json"),
        ];

        for (uri, mime) in expected {
            let response = get(&root, uri).await;
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert!(
                header(&response, "content-type").starts_with(mime),
                "{uri}: {}",
                header(&response, "content-type")
            );
        }
    }

    #[tokio::test]
    async fn accept_encoding_selects_the_precompressed_sibling() {
        let (_dir, root) = fixture();
        let uri = format!("/assets/app.{HASH}.js");

        let brotli = send(&root, &uri, &[("accept-encoding", "br")]).await;
        assert_eq!(header(&brotli, "content-encoding"), "br");
        assert_eq!(header(&brotli, "content-type"), "text/javascript");
        assert_eq!(body(brotli).await, "js-brotli");

        let gzip = send(&root, &uri, &[("accept-encoding", "gzip")]).await;
        assert_eq!(header(&gzip, "content-encoding"), "gzip");
        assert_eq!(body(gzip).await, "js-gzip");

        let plain = get(&root, &uri).await;
        assert_eq!(header(&plain, "content-encoding"), "");
        assert_eq!(body(plain).await, "export const ok=1;");
    }

    #[tokio::test]
    async fn every_static_response_varies_on_accept_encoding() {
        let (_dir, root) = fixture();
        let uri = format!("/assets/app.{HASH}.css");

        for headers in [&[][..], &[("accept-encoding", "br")][..]] {
            let response = send(&root, &uri, headers).await;
            assert_eq!(header(&response, "vary"), ACCEPT_ENCODING);
        }
        assert_eq!(header(&get(&root, "/").await, "vary"), ACCEPT_ENCODING);
    }

    #[tokio::test]
    async fn if_modified_since_revalidates_without_an_etag() {
        let (_dir, root) = fixture();
        let uri = format!("/assets/app.{HASH}.js");

        let first = get(&root, &uri).await;
        let modified = header(&first, "last-modified");
        assert!(!modified.is_empty());
        assert_eq!(header(&first, "etag"), "");

        let second = send(&root, &uri, &[("if-modified-since", &modified)]).await;
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn an_unknown_path_falls_back_to_the_shell() {
        let (_dir, root) = fixture();
        let response = get(&root, "/lists/oisd-basic").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(body(response).await.contains("fixture bundle"));
    }

    #[tokio::test]
    async fn an_asset_larger_than_the_read_chunk_arrives_whole() {
        let (_dir, root) = fixture();
        let big = "x".repeat(READ_CHUNK * 3 + 17);
        std::fs::write(root.join(ASSETS).join(format!("big.{HASH}.js")), &big).unwrap();

        let response = get(&root, &format!("/assets/big.{HASH}.js")).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await, big);
    }

    #[tokio::test]
    async fn a_missing_asset_is_a_plain_404_not_the_shell() {
        let (_dir, root) = fixture();
        let response = get(&root, "/assets/gone.deadbeef.js").await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(header(&response, "cache-control"), "");
        assert!(!body(response).await.contains("fixture bundle"));
    }

    fn readable_escape_target(dir: &TempDir) -> PathBuf {
        let target = dir.path().join("secret.txt");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            SECRET,
            "the escape target must exist and be readable, or a 404 proves nothing"
        );
        target
    }

    #[tokio::test]
    async fn traversal_under_assets_is_refused_with_a_404() {
        let (dir, root) = fixture();
        readable_escape_target(&dir);

        let attempts = [
            "/assets/../../secret.txt",
            "/assets/..%2f..%2fsecret.txt",
            "/assets/%2e%2e%2f%2e%2e%2fsecret.txt",
            "/assets/%2Fsecret.txt",
            "/assets/..\\..\\secret.txt",
            "/assets/..%5c..%5csecret.txt",
        ];

        for uri in attempts {
            let response = get(&root, uri).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
            assert_ne!(body(response).await, SECRET, "{uri} escaped the web root");
        }
    }

    #[tokio::test]
    async fn traversal_under_the_root_resolves_to_the_shell_never_the_target() {
        let (dir, root) = fixture();
        readable_escape_target(&dir);

        let attempts = [
            "/../secret.txt",
            "/..%2fsecret.txt",
            "/%2e%2e%2fsecret.txt",
            "/%2Fsecret.txt",
            "/..\\secret.txt",
            "/..%5csecret.txt",
        ];

        for uri in attempts {
            let response = get(&root, uri).await;
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            let served = body(response).await;
            assert_ne!(served, SECRET, "{uri} escaped the web root");
            assert!(
                served.contains("fixture bundle"),
                "{uri} resolved to something other than the shell: {served}"
            );
        }
    }

    #[tokio::test]
    async fn the_spa_fallback_serves_the_precompressed_shell() {
        let (_dir, root) = fixture();

        for uri in ["/", "/lists/oisd-basic"] {
            let response = send(&root, uri, &[("accept-encoding", "br")]).await;
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(header(&response, "content-encoding"), "br", "{uri}");
            assert_eq!(header(&response, "content-type"), "text/html", "{uri}");
            assert_eq!(body(response).await, "shell-brotli", "{uri}");
        }
    }

    #[tokio::test]
    async fn the_public_mount_builds_and_carries_the_vary_layer() {
        let response = mounted()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(header(&response, "vary"), ACCEPT_ENCODING);
    }

    #[test]
    fn the_fixed_root_is_the_directory_the_image_ships() {
        let dockerfile = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("Dockerfile");
        let source = std::fs::read_to_string(&dockerfile).expect("Dockerfile");
        let copy = format!("COPY --from=frontend {ROOT} {ROOT}");

        assert!(
            source
                .lines()
                .any(|line| !line.trim_start().starts_with('#') && line.contains(&copy)),
            "the Dockerfile no longer copies the bundle to {ROOT}"
        );
    }

    #[test]
    fn a_web_root_without_a_shell_is_reported() {
        let (_dir, root) = fixture();
        assert!(!shell_missing(&root));

        let empty = tempfile::tempdir().unwrap();
        assert!(shell_missing(empty.path()));
        assert!(shell_missing(&empty.path().join("never-created")));
    }
}
