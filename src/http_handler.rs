use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::Html,
    routing::{any, get},
    Json, Router,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::model::{self};

async fn root() -> Html<&'static str> {
    Html("<h1>Http Request Duplicator</h1>")
}

#[derive(Deserialize, Debug)]
#[serde(transparent)]
struct TargetUris {
    data: Vec<String>,
}

#[derive(Serialize, Debug)]
enum ErrorReason {
    InvalidTargets,
    InvalidPriority,
    UnknownPriority,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
enum DuplicateResponse {
    EnqueuedNormally { id: String },
    Error { reason: ErrorReason },
}

async fn duplicate(
    State(state): State<Arc<model::AppState>>,
    method: Method,
    mut headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<DuplicateResponse>) {
    let Some(Ok(Ok(targets))) = headers
        .get("x-duplicate-targets")
        .map(|v| v.to_str().map(serde_json::from_str::<TargetUris>))
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(DuplicateResponse::Error {
                reason: ErrorReason::InvalidTargets,
            }),
        );
    };

    let Some(Ok(priority)) = headers.get("x-duplicate-priority").map(|v| v.to_str()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(DuplicateResponse::Error {
                reason: ErrorReason::InvalidPriority,
            }),
        );
    };

    let priority = match priority.to_ascii_lowercase().as_ref() {
        "low" => Priority::Low,
        "high" => Priority::High,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(DuplicateResponse::Error {
                    reason: ErrorReason::UnknownPriority,
                }),
            );
        }
    };

    headers.remove("x-duplicate-priority");
    headers.remove("x-duplicate-targets");
    headers.remove("host");

    use crate::duplicator::*;

    let id = state.enqueuer.enqueue(
        Payload {
            body,
            headers,
            method,
        },
        priority,
        &targets.data,
    );

    (
        StatusCode::ACCEPTED,
        Json(DuplicateResponse::EnqueuedNormally { id }),
    )
}

async fn negative_cache(
    State(state): State<Arc<model::AppState>>,
) -> (StatusCode, Json<Vec<String>>) {
    (StatusCode::OK, Json(state.negative_cache.list().await))
}

async fn negative_cache_del(
    State(state): State<Arc<model::AppState>>,
    headers: HeaderMap,
) -> StatusCode {
    let Some(Ok(Ok(targets))) = headers
        .get("x-delete-targets")
        .map(|v| v.to_str().map(serde_json::from_str::<TargetUris>))
    else {
        return StatusCode::BAD_REQUEST;
    };

    for target in targets.data {
        state.negative_cache.delete(&target).await;
    }

    StatusCode::OK
}

pub async fn run(s: &SocketAddr, state: Arc<model::AppState>) -> Result<(), hyper::Error> {
    let app = Router::new()
        .route("/", get(root))
        .route("/api/duplicate", any(duplicate))
        .route(
            "/api/negative_cache",
            get(negative_cache).delete(negative_cache_del),
        )
        .with_state(state);

    axum::Server::bind(s).serve(app.into_make_service()).await
}
