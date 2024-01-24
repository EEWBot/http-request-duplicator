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

use crate::duplicator::Priority;
use crate::model::{self};

async fn root() -> Html<&'static str> {
    Html("<h1>Http Request Duplicator</h1>")
}

#[derive(Deserialize, Debug)]
#[serde(transparent)]
struct DuplicateTargets {
    data: Vec<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
enum DuplicateResponse {
    EnqueuedNormally { id: String },
    Error,
}

async fn duplicate(
    state: Arc<model::AppState>,
    method: Method,
    mut headers: HeaderMap,
    body: Bytes,
    priority: Priority,
) -> (StatusCode, Json<DuplicateResponse>) {
    let Some(Ok(Ok(targets))) = headers
        .get("x-duplicate-targets")
        .map(|v| v.to_str().map(serde_json::from_str::<DuplicateTargets>))
    else {
        return (StatusCode::BAD_REQUEST, Json(DuplicateResponse::Error));
    };

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

async fn duplicate_low_priority(
    State(state): State<Arc<model::AppState>>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<DuplicateResponse>) {
    duplicate(state, method, headers, body, Priority::Low).await
}

async fn duplicate_high_priority(
    State(state): State<Arc<model::AppState>>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<DuplicateResponse>) {
    duplicate(state, method, headers, body, Priority::High).await
}

pub async fn run(s: &SocketAddr, state: Arc<model::AppState>) -> Result<(), hyper::Error> {
    let app = Router::new()
        .route("/", get(root))
        .route("/duplicate/high_priority", any(duplicate_high_priority))
        .route("/duplicate/low_priority", any(duplicate_low_priority))
        .with_state(state);

    axum::Server::bind(s).serve(app.into_make_service()).await
}
