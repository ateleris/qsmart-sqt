//! Bearer-token authentication middleware (SRS-IF-006).
//!
//! The `OpenAPI` contract declares `bearerAuth` globally. This middleware
//! enforces it on every route except `GET /api/v1/health` (the unauthenticated
//! liveness probe). A request without a valid `Authorization: Bearer <token>`
//! header is rejected with `401` and a structured [`ErrorResponse`] body.

use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sqt_types::{ApiError, ErrorResponse};

/// Axum middleware: require `Authorization: Bearer <expected>` on the request.
///
/// The expected token is supplied as middleware state via
/// [`axum::middleware::from_fn_with_state`]. Requests that present the exact
/// token proceed; everything else gets a `401`.
pub async fn require_bearer(
    State(expected): State<Arc<String>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if presented_token(&req) == Some(expected.as_str()) {
        next.run(req).await
    } else {
        unauthorized()
    }
}

/// Extract the bearer token from the `Authorization` header, if present and
/// well-formed (`Bearer ` prefix, surrounding whitespace trimmed).
fn presented_token(req: &Request<Body>) -> Option<&str> {
    req.headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: ApiError {
                code: "UNAUTHORIZED".into(),
                message: "missing or invalid bearer token".into(),
                details: None,
                timestamp: crate::now_iso(),
            },
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::Router;
    use axum::routing::get;
    use tower::ServiceExt as _;

    async fn ok() -> &'static str {
        "ok"
    }

    /// A router with one protected route behind the bearer middleware and one
    /// unauthenticated `/health` route — mirroring the real server layout.
    fn app(token: &str) -> Router {
        let protected = Router::new().route("/protected", get(ok)).route_layer(
            axum::middleware::from_fn_with_state(Arc::new(token.to_string()), require_bearer),
        );
        Router::new().route("/health", get(ok)).merge(protected)
    }

    fn get_req(uri: &str, auth: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().uri(uri);
        if let Some(a) = auth {
            b = b.header("authorization", a);
        }
        b.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn protected_route_without_header_returns_401() {
        let resp = app("secret").oneshot(get_req("/protected", None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_route_with_wrong_token_returns_401() {
        let resp = app("secret").oneshot(get_req("/protected", Some("Bearer nope"))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_route_with_correct_token_passes() {
        let resp =
            app("secret").oneshot(get_req("/protected", Some("Bearer secret"))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn protected_route_with_non_bearer_scheme_returns_401() {
        let resp =
            app("secret").oneshot(get_req("/protected", Some("Basic secret"))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn health_route_is_unauthenticated() {
        let resp = app("secret").oneshot(get_req("/health", None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
