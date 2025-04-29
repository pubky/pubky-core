use std::sync::Arc;

use axum::{extract::Request, Router};
use tower_http::trace::{
    DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, OnFailure, OnRequest, OnResponse,
    TraceLayer,
};
use tracing::{Level, Span};

use crate::shared::PubkyHost;

pub fn with_trace_layer(router: Router, excluded_paths: &[&str]) -> Router {
    let excluded_paths = Arc::new(
        excluded_paths
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );

    router.layer(
        TraceLayer::new_for_http()
            .make_span_with(move |request: &Request| {
                if excluded_paths.contains(&request.uri().path().to_string()) {
                    // Skip logging for the noisy endpoint
                    tracing::span!(Level::INFO, "request", excluded = true)
                } else {
                    // Use the default span for other endpoints

                    let uri = if let Some(pubky_host) = request.extensions().get::<PubkyHost>() {
                        format!("pubky://{pubky_host}{}", request.uri())
                    } else {
                        request.uri().to_string()
                    };

                    tracing::span!(
                        Level::INFO,
                        "request",
                        method = %request.method(),
                        uri = ?uri,
                        version = ?request.version(),
                    )
                }
            })
            .on_request(|request: &Request, span: &Span| {
                // Skip logging for excluded spans
                if span.has_field("excluded") {
                    return;
                }
                // Use the default behavior for other spans
                DefaultOnRequest::new().on_request(request, span);
            })
            .on_response(
                |response: &axum::response::Response, latency: std::time::Duration, span: &Span| {
                    // Skip logging for excluded spans
                    if span.has_field("excluded") {
                        return;
                    }
                    // Use the default behavior for other spans
                    DefaultOnResponse::new().on_response(response, latency, span);
                },
            )
            .on_failure(
                |error: tower_http::classify::ServerErrorsFailureClass,
                 latency: std::time::Duration,
                 span: &Span| {
                    // Skip logging for excluded spans
                    if span.has_field("excluded") {
                        return;
                    }
                    // Use the default behavior for other spans
                    DefaultOnFailure::new().on_failure(error, latency, span);
                },
            ),
    )
}
