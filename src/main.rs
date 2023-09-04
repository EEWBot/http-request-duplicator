use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use hyper::service::{make_service_fn, service_fn};
use hyper::{body, Body, Request, Response, Server};
use reqwest::Client;
use serde::Deserialize;
use clap::Parser;

#[derive(Deserialize, Debug)]
#[serde(transparent)]
pub struct Targets {
    pub data: Vec<String>,
}

#[derive(Debug, Parser)]
struct Cli {
    #[clap(long, env)]
    listen: SocketAddr,
}

async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let (mut parts, body) = req.into_parts();

    let Ok(body_bytes) = body::to_bytes(body).await.map(|v| v.to_vec()) else {
        return Ok(Response::new(Body::from("Failed to read body")));
    };

    let Some(Ok(s)) = parts.headers.get("x-duplicate-targets").map(|v| v.to_str()) else {
        return Ok(Response::new(Body::from(
            "Failed to read x-duplicate-targets",
        )));
    };

    let Ok(targets) = serde_json::from_str::<Targets>(s) else {
        return Ok(Response::new(Body::from(
            "Failed to parse x-duplicate-targets",
        )));
    };

    parts.headers.remove("x-duplicate-targets");
    parts.headers.remove("host");

    let body_bytes = bytes::Bytes::from(body_bytes);

    for target in targets.data.into_iter() {
        let method = parts.method.clone();
        let headers = parts.headers.clone();
        let body_bytes = body_bytes.clone();

        tokio::spawn(async move {
            let client = Client::new();

            match client
                .request(method, &target)
                .headers(headers)
                .body(reqwest::Body::from(body_bytes))
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                Err(e) => {
                    println!("{} -> {target}", e);
                }
                Ok(resp) => {
                    println!("{} -> {target}", resp.status());
                }
            }
        });
    }

    Ok(Response::new(Body::from("OK")))
}

#[tokio::main]
async fn main() {
    let c = Cli::parse();
    let addr = SocketAddr::from(c.listen);

    let make_service =
        make_service_fn(|_conn| async move { Ok::<_, Infallible>(service_fn(handle)) });

    let server = Server::bind(&addr).serve(make_service);

    if let Err(e) = server.await {
        eprintln!("Server Error: {e}");
    }
}
