use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;

mod duplicator;
mod http_handler;
mod model;

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

    #[clap(long, env)]
    #[clap(default_value_t = false)]
    pub notfound_negative_cache: bool,
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
    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact();

    tracing_subscriber::fmt().event_format(format).init();

    let c = Cli::parse();

    let (enqueuer, runner) =
        duplicator::Builder::new((0..=10).map(|_| create_client(10)).collect())
            .global_limit(c.limiter)
            .retry_after(Duration::from_secs(c.retry_delay))
            .ttl(c.retry_count)
            .build();

    tokio::spawn(async move {
        runner.event_loop().await;
    });

    let state = Arc::new(model::AppState { enqueuer });

    http_handler::run(&c.listen, state).await.unwrap();
}
