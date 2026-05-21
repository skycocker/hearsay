//! Serve the embedded frontend build. In release builds rust-embed packs
//! `../../ui-frontend/dist` into the binary; in debug builds it reads from
//! disk so `npm run build` changes show up without a Rust rebuild.
//!
//! Anything that isn't an asset and isn't an API/WS route falls back to
//! `index.html`, so SPA hash-routing works out of the box.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../ui-frontend/dist"]
struct Assets;

pub async fn serve(req: Request<Body>) -> Response {
    let uri = req.uri().clone();
    match serve_path(&uri) {
        Some(resp) => resp,
        None => fallback_index(),
    }
}

fn serve_path(uri: &Uri) -> Option<Response> {
    let raw = uri.path().trim_start_matches('/');
    let path = if raw.is_empty() { "index.html" } else { raw };
    let file = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some(
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref().to_string())],
            file.data,
        )
            .into_response(),
    )
}

fn fallback_index() -> Response {
    match Assets::get("index.html") {
        Some(file) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            file.data,
        )
            .into_response(),
        None => {
            // Frontend not built yet. Send something helpful instead of a
            // blank 404 — `cargo run` works before `npm run build`.
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                Body::from(
                    "<!doctype html><meta charset='utf-8'><title>hearsay</title>\
                     <body style='font-family:system-ui;padding:2rem;color:#333'>\
                     <h2>hearsay daemon is running.</h2>\
                     <p>The web UI hasn't been built yet. Run:</p>\
                     <pre>cd ui-frontend && npm install && npm run build</pre>\
                     <p>Then refresh.</p>\
                     <p>API is live at <a href='/api/health'>/api/health</a>.</p>",
                ),
            )
                .into_response()
        }
    }
}
