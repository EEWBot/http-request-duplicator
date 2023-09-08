use std::convert::Infallible;
use std::sync::atomic::Ordering;
use std::net::SocketAddr;
use std::time::Duration;

use clap::Parser;
use hyper::service::{make_service_fn, service_fn};
use hyper::Server;
use once_cell::sync::OnceCell;
use reqwest::Client;
use tokio::sync::mpsc;
use tokio::time::sleep;

mod counter;
mod channel;
mod http_handler;
mod request_processor;
mod model;

#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(long, env)]
    pub listen: SocketAddr,

    #[clap(long, env)]
    #[clap(default_value_t = 256)]
    pub parallels: usize,

    #[clap(long, env)]
    #[clap(default_value_t = 5)]
    pub timeout: u64,
}

#[tokio::main]
async fn main() {
    let c = Cli::parse();

    let (drop_tx, drop_rx) = mpsc::channel(1);
    let (low_priority_tx, low_priority_rx) = mpsc::unbounded_channel();
    let (high_priority_tx, high_priority_rx) = mpsc::unbounded_channel();

    channel::CHANNELS
        .set(channel::Channels {
            drop_low_priority_requests: drop_tx,
            low_priority_sock: low_priority_tx.clone(),
            high_priority_sock: high_priority_tx.clone(),
        })
        .unwrap();

    request_processor::LIMITER.add_permits(c.parallels);
    request_processor::CLIENT
        .set(
            Client::builder()
                .timeout(Duration::from_secs(c.timeout))
                .pool_max_idle_per_host(c.parallels)
                .build()
                .unwrap(),
        )
        .unwrap();

    tokio::spawn(async move {
        request_processor::event_loop(
            drop_rx,
            high_priority_rx,
            low_priority_rx,
        )
        .await;
    });

    let make_service =
        make_service_fn(
            |_conn| async move { Ok::<_, Infallible>(service_fn(http_handler::handle)) },
        );

    let server = Server::bind(&c.listen).serve(make_service);

    if let Err(e) = server.await {
        eprintln!("Server Error: {e}");
    }
}
