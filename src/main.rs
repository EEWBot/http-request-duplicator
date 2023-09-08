use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::{atomic::AtomicUsize, Arc, RwLock};
use std::time::Duration;

use bytes::Bytes;
use clap::Parser;
use hyper::service::{make_service_fn, service_fn};
use hyper::{body, Body, HeaderMap, Method, Request, Response, Server};
use once_cell::sync::OnceCell;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::mpsc::{self, Sender, UnboundedSender};
use tokio::sync::Semaphore;
use tokio::time::sleep;

#[derive(Deserialize, Debug)]
#[serde(transparent)]
pub struct Targets {
    pub data: Vec<String>,
}

#[derive(Debug, Parser)]
struct Cli {
    #[clap(long, env)]
    listen: SocketAddr,

    #[clap(long, env)]
    #[clap(default_value_t = 256)]
    parallels: usize,

    #[clap(long, env)]
    #[clap(default_value_t = 5)]
    timeout: u64,
}

#[derive(Debug, Clone)]
struct ReadonlySharedObjectsBetweenContexts {
    headers: HeaderMap,
    method: Method,
}

#[derive(Debug)]
struct RequestContext {
    target: String,
    readonly_objects: Arc<RwLock<ReadonlySharedObjectsBetweenContexts>>,
    body: Bytes,
    ttl: usize,
}

#[derive(Debug)]
struct Counter {
    queued: AtomicUsize,
    succeed: AtomicUsize,
    failed: AtomicUsize,
}

#[derive(Debug)]
struct Counters {
    high_priority: Counter,
    low_priority: Counter,
}

impl Counters {
    const fn new() -> Self {
        Self {
            high_priority: Counter {
                queued: AtomicUsize::new(0),
                succeed: AtomicUsize::new(0),
                failed: AtomicUsize::new(0),
            },
            low_priority: Counter {
                queued: AtomicUsize::new(0),
                succeed: AtomicUsize::new(0),
                failed: AtomicUsize::new(0),
            },
        }
    }
}

#[derive(Debug)]
struct Shared {
    high_priority_sock: UnboundedSender<RequestContext>,
    low_priority_sock: UnboundedSender<RequestContext>,
    drop_low_priority_requests: Sender<()>,
}

static COUNTERS: Counters = Counters::new();
static LIMITER: Semaphore = Semaphore::const_new(0);
static SHARED: OnceCell<Shared> = OnceCell::new();
static CLIENT: OnceCell<Client> = OnceCell::new();

async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let (mut parts, body) = req.into_parts();

    let shared = SHARED.get().unwrap();

    if let Some(Ok("true")) = parts
        .headers
        .get("x-clear-low-priority-queue")
        .map(|v| v.to_str())
    {
        shared.drop_low_priority_requests.send(()).await.unwrap();
    }

    let high_priority = matches!(
        parts.headers.get("x-high-priority").map(|v| v.to_str()),
        Some(Ok("true"))
    );

    let Some(Ok(s)) = parts.headers.get("x-duplicate-targets").map(|v| v.to_str()) else {
        return Ok(Response::new(Body::from("Do nothing")));
    };

    let Ok(targets) = serde_json::from_str::<Targets>(s) else {
        return Ok(Response::new(Body::from(
            "Failed to parse x-duplicate-targets",
        )));
    };

    let Ok(body_bytes) = body::to_bytes(body).await.map(|v| v.to_vec()) else {
        return Ok(Response::new(Body::from("Failed to read body")));
    };

    parts.headers.remove("x-duplicate-targets");
    parts.headers.remove("x-clear-low-priority-queue");
    parts.headers.remove("x-high-priority");
    parts.headers.remove("host");

    let body_bytes = Bytes::from(body_bytes);

    let delayed_clone_objects = Arc::new(RwLock::new(ReadonlySharedObjectsBetweenContexts {
        headers: parts.headers,
        method: parts.method,
    }));

    for target in targets.data.into_iter() {
        let context = RequestContext {
            target,
            readonly_objects: delayed_clone_objects.clone(),
            body: body_bytes.clone(),
            ttl: 3,
        };

        if high_priority {
            COUNTERS
                .high_priority
                .queued
                .fetch_add(1, Ordering::Relaxed);
            shared.high_priority_sock.send(context).unwrap();
        } else {
            COUNTERS.low_priority.queued.fetch_add(1, Ordering::Relaxed);
            shared.low_priority_sock.send(context).unwrap();
        }
    }

    Ok(Response::new(Body::from("OK")))
}

async fn request(ctx: &RequestContext) -> Result<(), ()> {
    let client = CLIENT.get().unwrap();

    // delayed clone
    let shared = ctx.readonly_objects.read().unwrap().clone();

    match client
        .request(shared.method, &ctx.target)
        .headers(shared.headers)
        .body(reqwest::Body::from(ctx.body.clone()))
        .send()
        .await
    {
        Err(e) => {
            println!("{} -> {}", e, ctx.target);
            Err(())
        }
        Ok(resp) => {
            if !(200..=299).contains(&resp.status().as_u16()) {
                println!("{} -> {}", resp.status(), ctx.target);
                return Err(());
            }
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() {
    let c = Cli::parse();

    let (drop_tx, mut drop_rx) = mpsc::channel(1);
    let (low_priority_tx, mut low_priority_rx) = mpsc::unbounded_channel();
    let (high_priority_tx, mut high_priority_rx) = mpsc::unbounded_channel();

    SHARED
        .set(Shared {
            drop_low_priority_requests: drop_tx,
            low_priority_sock: low_priority_tx.clone(),
            high_priority_sock: high_priority_tx.clone(),
        })
        .unwrap();

    LIMITER.add_permits(c.parallels);

    CLIENT
        .set(
            Client::builder()
                .timeout(Duration::from_secs(c.timeout))
                .pool_max_idle_per_host(c.parallels)
                .build()
                .unwrap(),
        )
        .unwrap();

    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(1)).await;
            let current = c.parallels - LIMITER.available_permits();
            let limit = c.parallels;
            let queue_low = COUNTERS.low_priority.queued.load(Ordering::Relaxed);
            let queue_high = COUNTERS.high_priority.queued.load(Ordering::Relaxed);
            println!("Workers: {current}/{limit}, Queue: {queue_high}+{queue_low}");
        }
    });

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(_) = drop_rx.recv() => {
                    let mut n = 0;
                    while low_priority_rx.try_recv().is_ok() {
                        n += 1;
                    }
                    println!("LOW PRIORITY QUEUE {n} DROPPED!");
                }
                Some(mut ctx) = high_priority_rx.recv() => {
                    COUNTERS.high_priority.queued.fetch_sub(1, Ordering::Relaxed);
                    let limiter = LIMITER.acquire().await;
                    let high_priority_tx = high_priority_tx.clone();

                    tokio::spawn(async move {
                        let _limiter = limiter;

                        if request(&ctx).await.is_ok() {
                            COUNTERS.high_priority.succeed.fetch_add(1, Ordering::Relaxed);

                        } else {
                            COUNTERS.high_priority.failed.fetch_add(1, Ordering::Relaxed);

                            ctx.ttl -= 1;
                            if ctx.ttl != 0 {
                                println!("Retry! TTL: {}", ctx.ttl - 1);
                                sleep(Duration::from_secs(3)).await;
                                COUNTERS.high_priority.queued.fetch_add(1, Ordering::Relaxed);
                                high_priority_tx.send(ctx).unwrap();
                            }
                        }
                    });
                }
                Some(mut ctx) = low_priority_rx.recv(), if COUNTERS.high_priority.queued.load(Ordering::Relaxed) == 0 => {
                    COUNTERS.low_priority.queued.fetch_sub(1, Ordering::Relaxed);
                    let limiter = LIMITER.acquire().await;
                    let low_priority_tx = low_priority_tx.clone();

                    tokio::spawn(async move {
                        let _limiter = limiter;

                        if request(&ctx).await.is_ok() {
                            COUNTERS.low_priority.succeed.fetch_add(1, Ordering::Relaxed);

                        } else {
                            COUNTERS.low_priority.failed.fetch_add(1, Ordering::Relaxed);

                            ctx.ttl -= 1;
                            if ctx.ttl != 0 {
                                println!("Retry! TTL: {}", ctx.ttl - 1);
                                sleep(Duration::from_secs(3)).await;
                                COUNTERS.low_priority.queued.fetch_add(1, Ordering::Relaxed);
                                low_priority_tx.send(ctx).unwrap();
                            }
                        }
                    });
                }
            }
        }
    });

    let make_service =
        make_service_fn(|_conn| async move { Ok::<_, Infallible>(service_fn(handle)) });

    let server = Server::bind(&c.listen).serve(make_service);

    if let Err(e) = server.await {
        eprintln!("Server Error: {e}");
    }
}
