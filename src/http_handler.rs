use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::map_response_with_state,
    response::{Html, Response},
    routing::{any, get},
    Json, Router,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::model;

async fn root() -> Html<&'static str> {
    Html("<h1>HTTP Request Duplicator</h1>")
}

#[derive(Deserialize, Debug)]
#[serde(transparent)]
struct TargetUris {
    data: Vec<String>,
}

#[derive(Serialize, Debug)]
enum ErrorReason {
    InvalidTargets,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
enum DuplicateResponse {
    EnqueuedNormally { id: String },
    Error { reason: ErrorReason },
}

async fn duplicate(
    State(state): State<model::AppState>,
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

    let targets = targets.data;

    headers.remove("x-duplicate-targets");
    headers.remove("host");

    use crate::duplicator::*;
    let id = ulid::Ulid::new().to_string();

    tracing::info!("{} Enqueued", id);

    state.duplicator.duplicate(
        std::sync::Arc::new(Payload {
            body,
            headers,
            method,
            id: id.to_string(),
        }),
        targets,
        2,
    ).await;


    (
        StatusCode::ACCEPTED,
        Json(DuplicateResponse::EnqueuedNormally { id }),
    )
}

async fn negative_cache(
    State(state): State<model::AppState>,
) -> (StatusCode, Json<Vec<String>>) {
    (StatusCode::OK, Json(state.duplicator.banned().await))
}

async fn negative_cache_del(
    State(state): State<model::AppState>,
    headers: HeaderMap,
) -> StatusCode {
    let Some(Ok(Ok(targets))) = headers
        .get("x-delete-targets")
        .map(|v| v.to_str().map(serde_json::from_str::<TargetUris>))
    else {
        return StatusCode::BAD_REQUEST;
    };

    state.duplicator.purge_banned_urls(targets.data).await;

    StatusCode::OK
}

pub async fn run(
    s: TcpListener,
    state: model::AppState,
    identifier: &str,
) -> Result<(), std::io::Error> {
    let app = Router::new()
        .route("/", get(root))
        .route("/api/duplicate", any(duplicate))
        .route(
            "/api/negative_cache",
            get(negative_cache).delete(negative_cache_del),
        )
        .layer(map_response_with_state(
            identifier.to_string(),
            |State(identifier): State<String>, mut response: Response<_>| async {
                response.headers_mut().insert(
                    "x-identifier",
                    HeaderValue::from_maybe_shared(identifier).unwrap(),
                );

                response
            },
        ))
        .with_state(state);

    axum::serve(s, app.into_make_service()).await
}
