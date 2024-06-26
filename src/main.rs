use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tokio::net::TcpListener;

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
    #[clap(default_value_t = 2)]
    pub timeout: u64,

    #[clap(long, env)]
    #[clap(default_value = "Unspecified")]
    pub identifier: String,

    #[clap(long, env)]
    #[clap(default_value_t = false)]
    pub notfound_negative_cache: bool,
}

fn create_client(timeout: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .read_timeout(Duration::from_secs(timeout))
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(Duration::from_secs(6 * 60 * 60))
        .http2_adaptive_window(true)
        .http2_keep_alive_interval(Duration::from_secs(60))
        .http2_keep_alive_while_idle(true)
        .http2_keep_alive_timeout(Duration::from_secs(6 * 60 * 60))
        .http2_prior_knowledge()
        .hickory_dns(true)
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

    tracing_subscriber::fmt().event_format(format).with_max_level(tracing::Level::INFO).init();

    let c = Cli::parse();

    let (enqueuer, negative_cache, runner) =
        duplicator::Builder::new((0..c.pool).map(|_| create_client(c.timeout)).collect())
            .global_limit(c.limiter)
            .retry_after(Duration::from_secs(c.retry_delay))
            .ttl(c.retry_count)
            .build();

    tokio::spawn(async move {
        runner.event_loop().await;
    });

    let state = Arc::new(model::AppState {
        enqueuer,
        negative_cache,
    });

    let listener = TcpListener::bind(c.listen).await.unwrap();
    http_handler::run(listener, state, &c.identifier).await.unwrap();
}
