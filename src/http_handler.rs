use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use axum::{
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::Html,
    routing::{get, on, post, MethodFilter},
    Json, Router,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::model::{self, Priority};

async fn root() -> Html<&'static str> {
    Html("<h1>Http Request Duplicator</h1>")
}

#[derive(Deserialize, Debug)]
#[serde(transparent)]
struct DuplicateTargets {
    data: Vec<String>,
}

#[derive(Serialize, Debug)]
enum DuplicateResponse {
    Ok,
    Error,
}

async fn duplicate(
    state: Arc<model::AppState>,
    method: Method,
    mut headers: HeaderMap,
    body: Bytes,
    priority: Priority,
) -> (StatusCode, Json<DuplicateResponse>) {
    let Some(Ok(Ok(targets))) = headers.get("x-duplicate-targets").map(|v| {
        v.to_str()
            .map(|s| serde_json::from_str::<DuplicateTargets>(s))
    }) else {
        return (StatusCode::BAD_REQUEST, Json(DuplicateResponse::Error));
    };

    headers.remove("x-duplicate-targets");
    headers.remove("host");

    let delayed_clone_objects =
        Arc::new(RwLock::new(model::ReadonlySharedObjectsBetweenContexts {
            headers,
            method,
        }));

    for target in targets.data.into_iter() {
        let context = model::RequestContext {
            target,
            readonly_objects: Arc::clone(&delayed_clone_objects),
            body: body.clone(),
            ttl: 3,
        };

        state.counters.get(priority).enqueue();
        state.channels.get_queue(priority).send(context).unwrap();
    }

    (StatusCode::ACCEPTED, Json(DuplicateResponse::Ok))
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

async fn flush_low_priority(State(state): State<Arc<model::AppState>>) -> StatusCode {
    let _ = state.channels.flush_low_priority_queue.send(()).await;
    StatusCode::ACCEPTED
}

pub async fn run(s: &SocketAddr, state: Arc<model::AppState>) -> Result<(), hyper::Error> {
    let app = Router::new()
        .route("/", get(root))
        .route("/flush/low_priority", post(flush_low_priority))
        .route(
            "/duplicate/high_priority",
            on(MethodFilter::all(), duplicate_high_priority),
        )
        .route(
            "/duplicate/low_priority",
            on(MethodFilter::all(), duplicate_low_priority),
        )
        .with_state(state);

    axum::Server::bind(s).serve(app.into_make_service()).await
}
