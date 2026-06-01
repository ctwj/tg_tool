use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/dist/"]
struct Assets;

/// Serve embedded static files from web/dist/
/// Returns index.html for unknown paths (SPA fallback)
pub async fn static_handler(req: Request<Body>) -> Response<Body> {
    let path = req.uri().path().trim_start_matches('/');

    // Skip API and WebSocket paths
    if path.starts_with("api/") || path.starts_with("ws") {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap();
    }

    // Try to find the exact file first
    if !path.is_empty() {
        if let Some(file) = Assets::get(path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(file.data.into_owned()))
                .unwrap();
        }
    }

    // SPA fallback: serve index.html
    if let Some(file) = Assets::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(file.data.into_owned()))
            .unwrap();
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Not Found - Frontend not built. Run: cd web && npm run build"))
        .unwrap()
}
