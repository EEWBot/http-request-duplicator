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
    #[clap(default_value_t = 4)]
    pub pool: usize,

    #[clap(long, env)]
    #[clap(default_value_t = 1024)]
    pub limiter: usize,

    #[clap(long, env)]
    #[clap(default_value_t = 3)]
    pub retry_count: usize,

    #[clap(long, env)]
    #[clap(default_value_t = 1)]
    pub retry_delay: u64,

    #[clap(long, env)]
    #[clap(default_value_t = 5)]
    pub timeout: u64,
}


fn create_client(timeout: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .http3_prior_knowledge()
        .brotli(true)
        .build()
        .unwrap()
}


#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let c = Cli::parse();

    let (low_priority_tx, low_priority_rx) = mpsc::unbounded_channel();
    let (high_priority_tx, high_priority_rx) = mpsc::unbounded_channel();

    let state = Arc::new(model::AppState {
        channels: model::Channels::new(&high_priority_tx, &low_priority_tx),
        counters: model::Counters::new(),
        log: model::Log::new(),
        retry_count: c.retry_count,
    });

    let sender = request_sender::RequestSender::new(
        state.clone(),
        (0..c.pool).map(|_| create_client(c.timeout)).collect(),
        c.limiter,
        c.retry_delay,
    );

    tokio::spawn(async move {
        sender
            .event_loop(high_priority_rx, low_priority_rx)
            .await;
    });

    http_handler::run(&c.listen, state).await.unwrap();
}
