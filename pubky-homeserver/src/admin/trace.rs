use axum::{extract::Request, Router};
use tower_http::trace::{
    DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, OnFailure, OnRequest, OnResponse,
    TraceLayer,
};
use tracing::{Level, Span};

pub fn with_trace_layer(router: Router) -> Router {

    router.layer(
        TraceLayer::new_for_http()
            .make_span_with(move |request: &Request| {
                let uri = request.uri().to_string();
                tracing::span!(
                    Level::INFO,
                    "request",
                    method = %request.method(),
                    uri = ?uri,
                    version = ?request.version(),
                )
            })
            .on_request(|request: &Request, span: &Span| {
                // Use the default behavior for other spans
                DefaultOnRequest::new().on_request(request, span);
            })
            .on_response(
                |response: &axum::response::Response, latency: std::time::Duration, span: &Span| {
                    // Use the default behavior for other spans
                    DefaultOnResponse::new().on_response(response, latency, span);
                },
            )
            .on_failure(
                |error: tower_http::classify::ServerErrorsFailureClass,
                 latency: std::time::Duration,
                 span: &Span| {
                    // Use the default behavior for other spans
                    DefaultOnFailure::new().on_failure(error, latency, span);
                },
            ),
    )
}
