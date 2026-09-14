//! Static delivery of the dashboard bundle (`--ui-dir`).
//!
//! Until this module existed the bundle was handed to a bare
//! `ServeDir::new(ui_dir).fallback(ServeFile::new(index))`, which sets no
//! `Cache-Control` at all and compresses nothing. `ServeDir` does emit a
//! `Last-Modified`, so a browser would revalidate rather than re-download —
//! but that is still a request per asset per page load, on a bundle that
//! code-splits into a dozen chunks, and every one of them travelled
//! uncompressed.
//!
//! The split below is the one Vite's output makes possible. Everything under
//! `assets/` carries a content hash in its filename, so a given name can never
//! change meaning and may be cached for a year. Everything else — `index.html`
//! above all — must revalidate, because a stale `index.html` names asset files
//! from an older build and renders a blank page after every upgrade. That is
//! the failure mode a blanket `max-age` would introduce, so the two are
//! deliberately served by two different stacks rather than one with an
//! exception.
//!
//! Compression is on-the-fly rather than precompressed-on-disk
//! (`ServeDir::precompressed_gzip`): Vite emits no `.gz`/`.br` siblings, and
//! the immutable caching above means a given asset is compressed once per
//! client rather than once per request. Only the static services are wrapped —
//! the API router is not, because compressing an SSE stream buffers it, and
//! the runners page and the console are both SSE. (tower-http's
//! `DefaultPredicate` declines `text/event-stream` and bodies of 32 bytes or
//! fewer on its own, so that exclusion is belt-and-braces rather than the only
//! thing standing between the console and a buffered stream.)

use axum::Router;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use std::path::Path;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

/// `Cache-Control` for content-hashed build output.
///
/// A year is the maximum `max-age` browsers honour in practice, and
/// `immutable` additionally suppresses the revalidation request a user-agent
/// would otherwise send on a forced reload.
pub const ASSET_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// `Cache-Control` for a request to `/assets/…` that finds no file.
///
/// A 404 is cacheable when it carries explicit freshness (RFC 9111 §3), so
/// handing one the year above is a real hazard rather than a curiosity: during
/// a rolling upgrade a browser holding the new `index.html` can ask an old
/// replica for a new chunk, and cache the miss until the user clears site data.
/// The split images make that reachable — `croniq-ui` and `croniq-server` roll
/// independently (issue #655).
///
/// `no-store` rather than `no-cache`: there is nothing here worth revalidating.
pub const MISSING_ASSET_CACHE_CONTROL: &str = "no-store";

/// `Cache-Control` for the document and everything else not content-hashed.
///
/// `no-cache` permits storing the response but requires revalidation before
/// reuse, which is what keeps a client from booting an old `index.html` after
/// an upgrade. It is not `no-store`: a 304 is much cheaper than a re-download.
pub const DOCUMENT_CACHE_CONTROL: &str = "no-cache";

/// Build the router that serves a built dashboard out of `ui_dir`.
///
/// Mount with `app.nest("/assets", …)` for the hashed half and
/// `app.fallback_service(…)` for the rest — see [`mount`].
fn assets_router(ui_dir: &Path) -> Router {
    Router::new()
        .fallback_service(ServeDir::new(ui_dir.join("assets")))
        // Not `SetResponseHeaderLayer`: it cannot see the status, and this
        // header must not be applied to a miss. See
        // [`MISSING_ASSET_CACHE_CONTROL`].
        .layer(axum::middleware::map_response(cache_control_by_status))
        .layer(CompressionLayer::new())
}

/// Pick the asset `Cache-Control` from the status the file service produced.
///
/// A year for a file that was served — 2xx, and 304, where the stored response
/// is being confirmed and its freshness should be refreshed with it. `no-store`
/// for everything else, which in practice is the 404 for a chunk this replica
/// does not have.
async fn cache_control_by_status(mut response: Response) -> Response {
    let served = response.status().is_success() || response.status() == StatusCode::NOT_MODIFIED;
    let value = if served {
        ASSET_CACHE_CONTROL
    } else {
        MISSING_ASSET_CACHE_CONTROL
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    response
}

/// The document half: `index.html`, `/icons/*`, the web manifest, and the SPA
/// fallback that answers every client-side route with `index.html`.
fn document_router(ui_dir: &Path) -> Router {
    let index = ui_dir.join("index.html");
    Router::new()
        .fallback_service(ServeDir::new(ui_dir).fallback(ServeFile::new(&index)))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static(DOCUMENT_CACHE_CONTROL),
        ))
        .layer(CompressionLayer::new())
}

/// Attach the dashboard to `app`.
///
/// `nest` takes precedence over `fallback_service`, so a request for
/// `/assets/index-abc123.js` is answered by the immutable stack even though
/// the document stack's `ServeDir` is rooted one level up and could serve the
/// same file. The order matters and is asserted in the tests below.
pub fn mount(app: Router, ui_dir: &Path) -> Router {
    app.nest("/assets", assets_router(ui_dir))
        .fallback_service(document_router(ui_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    /// A minimal `ui_dir` shaped like a Vite build.
    ///
    /// The asset is padded past 32 bytes deliberately: tower-http's
    /// `DefaultPredicate` declines to compress bodies at or below that size,
    /// so a two-line fixture would make the compression assertions pass or
    /// fail for a reason unrelated to what they test.
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.html"),
            "<!doctype html><html></html>",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        std::fs::write(
            dir.path().join("assets").join("index-abc123.js"),
            "console.log('a reasonably sized module body so the compression predicate applies')",
        )
        .unwrap();
        dir
    }

    async fn get(dir: &Path, uri: &str) -> axum::http::Response<Body> {
        let app = mount(Router::new(), dir);
        app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    fn cache_control(res: &axum::http::Response<Body>) -> &str {
        res.headers()
            .get(header::CACHE_CONTROL)
            .expect("Cache-Control present")
            .to_str()
            .unwrap()
    }

    #[tokio::test]
    async fn hashed_assets_are_immutable_for_a_year() {
        let dir = fixture();
        let res = get(dir.path(), "/assets/index-abc123.js").await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(cache_control(&res), ASSET_CACHE_CONTROL);
    }

    /// A hashed chunk this build does not have must not be cached at all.
    ///
    /// The header layer used to be a `SetResponseHeaderLayer` over the whole
    /// `ServeDir`, which cannot see the status — so the 404 came back with a
    /// year of `immutable`. A 404 carrying explicit freshness is cacheable
    /// (RFC 9111 section 3), and during a rolling upgrade a browser holding the
    /// new `index.html` can ask an old replica for a new chunk. Caching that
    /// miss for a year leaves a broken dashboard until site data is cleared
    /// (issue #655).
    #[tokio::test]
    async fn a_missing_asset_is_not_cached() {
        let dir = fixture();
        let res = get(dir.path(), "/assets/index-doesnotexist.js").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert_eq!(cache_control(&res), MISSING_ASSET_CACHE_CONTROL);
        assert!(
            !cache_control(&res).contains("immutable"),
            "a miss must never carry the immutable header"
        );
    }

    /// The nginx image answers the same request, and it is a text file the
    /// compiler never sees — so the two halves are asserted equal here, the
    /// way the CSP already is in `api::hardening`.
    #[test]
    fn nginx_does_not_cache_a_missing_asset_either() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docker/ui/nginx.conf");
        let conf = std::fs::read_to_string(path)
            .expect("docker/ui/nginx.conf must exist — it is the other half of this policy");

        assert!(
            conf.contains(&format!(
                r#"add_header Cache-Control "{MISSING_ASSET_CACHE_CONTROL}""#
            )),
            "docker/ui/nginx.conf must answer a missing asset with {MISSING_ASSET_CACHE_CONTROL}"
        );
        assert!(
            !conf.contains(&format!(
                r#"add_header Cache-Control "{ASSET_CACHE_CONTROL}" always"#
            )),
            "`always` on the asset Cache-Control extends it to the 404 — that is              the bug in #655, and nginx applies add_header to 2xx/3xx without it"
        );
    }

    #[tokio::test]
    async fn the_document_must_revalidate() {
        let dir = fixture();
        let res = get(dir.path(), "/index.html").await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(cache_control(&res), DOCUMENT_CACHE_CONTROL);
    }

    /// The regression this module exists to prevent: a client-side route is
    /// answered with `index.html`, and that response must revalidate. If it
    /// ever inherited the immutable policy, every upgrade would strand clients
    /// on a cached document naming asset files that no longer exist.
    #[tokio::test]
    async fn the_spa_fallback_must_revalidate() {
        let dir = fixture();
        let res = get(dir.path(), "/jobs/nightly-backup").await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(cache_control(&res), DOCUMENT_CACHE_CONTROL);
    }

    /// `nest("/assets")` must win over the document stack's `ServeDir`, which
    /// is rooted at `ui_dir` and can reach the same file by the same path.
    #[tokio::test]
    async fn the_nested_assets_stack_wins_over_the_document_fallback() {
        let dir = fixture();
        let res = get(dir.path(), "/assets/index-abc123.js").await;
        assert_ne!(cache_control(&res), DOCUMENT_CACHE_CONTROL);
    }

    #[tokio::test]
    async fn assets_are_compressed_when_the_client_asks() {
        let dir = fixture();
        let app = mount(Router::new(), dir.path());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/assets/index-abc123.js")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            res.headers()
                .get(header::CONTENT_ENCODING)
                .map(|v| v.to_str().unwrap()),
            Some("gzip")
        );
    }

    /// A client that does not advertise an encoding still gets the file.
    #[tokio::test]
    async fn compression_is_negotiated_not_forced() {
        let dir = fixture();
        let res = get(dir.path(), "/assets/index-abc123.js").await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(header::CONTENT_ENCODING).is_none());
    }
}
