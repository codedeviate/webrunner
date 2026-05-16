use axum::extract::State;
use axum::http::{header, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;

/// Insert `Strict-Transport-Security: <value>` into every response.
/// Applied only to the HTTPS service in `server::bind_and_serve`, so
/// the header never appears on plain-HTTP responses (per RFC 6797 §7.2).
pub async fn middleware(
    State(value): State<HeaderValue>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let mut response = next.run(req).await;
    response.headers_mut().insert(header::STRICT_TRANSPORT_SECURITY, value);
    response
}
