use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tokio::sync::mpsc;

mod http_handler;
mod model;
mod request_sender;

#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(long, env)]
    pub listen: SocketAddr,

    #[clap(long, env)]
    #[clap(default_value_t = 128)]
    pub parallels: usize,

    #[clap(long, env)]
    #[clap(default_value_t = 5)]
    pub timeout: u64,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let c = Cli::parse();

    let (drop_tx, drop_rx) = mpsc::channel(1);
    let (low_priority_tx, low_priority_rx) = mpsc::unbounded_channel();
    let (high_priority_tx, high_priority_rx) = mpsc::unbounded_channel();

    let state = Arc::new(model::AppState {
        channels: model::Channels::new(&high_priority_tx, &low_priority_tx, &drop_tx),
        counters: model::Counters::new(),
        log: model::Log::new(),
    });

    let sender = request_sender::RequestSender::new(
        state.clone(),
        reqwest::Client::builder()
            .timeout(Duration::from_secs(c.timeout))
            .pool_max_idle_per_host(c.parallels)
            .http3_prior_knowledge()
            .brotli(true)
            .build()
            .unwrap(),
        c.parallels,
    );

    tokio::spawn(async move {
        sender
            .event_loop(drop_rx, high_priority_rx, low_priority_rx)
            .await;
    });

    http_handler::run(&c.listen, state).await.unwrap();
}
