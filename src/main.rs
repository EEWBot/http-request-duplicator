use std::net::{SocketAddr, IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result as AHResult;
use clap::Parser;
use tokio::net::TcpStream;
use tokio::sync::{watch, Semaphore};
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::{pki_types::ServerName, RootCertStore};
use hickory_resolver::Resolver;
use hickory_resolver::config::*;

const ALPN_H2: &str = "h2";
const HTTP2_SETTINGS_MAX_CONCURRENT_STREAMS: usize = 100;
const WORKER_COUNT_PER_IPS: usize = 8;

#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(long, env)]
    pub listen: SocketAddr,

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

fn query_discord_ips() -> Vec<Ipv4Addr> {
    let resolver = Resolver::new(ResolverConfig::default(), ResolverOpts::default()).unwrap();

    let mut ips = vec![];
    let response = resolver.lookup_ip("discord.com").unwrap();
    ips.extend(response.iter().filter_map(|ip| {
        match ip {
            IpAddr::V4(ip) => Some(ip),
            _ => panic!("WTF!? discord.com provides IPv6 Addr"),
        }
    }));

    tracing::info!("I got {} ips in discord.com! {ips:?}", ips.len());

    ips
}

#[derive(Debug)]
struct Request {
    method: http::Method,
    headers: http::HeaderMap,
    body: bytes::Bytes,
}

impl Drop for Request {
    fn drop(&mut self) {
        tracing::info!("DONE!");
    }
}

#[derive(Debug)]
enum ClientMessage {
    Request((Vec<http::Uri>, Arc<Request>)),
}

async fn conn(addr: &Ipv4Addr, channel: &mut async_channel::Receiver<ClientMessage>) -> AHResult<()> {
    let tls_client_config = Arc::new({
        let root_store = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let mut c = tokio_rustls::rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        c.alpn_protocols.push(ALPN_H2.as_bytes().to_owned());

        c
    });

    let connector = TlsConnector::from(tls_client_config);

    let tcp = TcpStream::connect(format!("{}:443", addr)).await.unwrap();
    let dns_name = ServerName::try_from("discord.com").unwrap();
    let tls = connector.connect(dns_name, tcp).await?;

    {
        let (_, session) = tls.get_ref();

        let negotiated = session.alpn_protocol();
        let reference = Some(ALPN_H2.as_bytes());

        anyhow::ensure!(negotiated == reference, "Negotiated protocol is not HTTP/2");
    }

    let (mut client, mut connection) = h2::client::handshake(tls).await?;

    let mut ping_pong = connection.ping_pong().unwrap();

    let (tx, mut rx) = watch::channel(None);

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tx.send(Some(e)).unwrap();
        };
    });

    let semaphroe = Arc::new(Semaphore::new(HTTP2_SETTINGS_MAX_CONCURRENT_STREAMS));

    tracing::info!("Connection established!");

    loop {
        tokio::select! {
           mes = channel.recv() => {
                let ClientMessage::Request(req) = mes.unwrap() else {
                    panic!("??");
                };

                let (targets, method, headers, body) = (
                    &req.0,
                    &req.1.method,
                    &req.1.headers,
                    &req.1.body,
                );

                for target in targets {
                    let permit = semaphroe.clone().acquire_owned().await.unwrap();

                    let mut request = http::Request::builder().method(method).uri(target).body(()).unwrap();
                    *request.headers_mut() = headers.clone();

                    let (response, mut respond) = client.send_request(request, false)?;

                    let x = req.1.clone();

                    respond.reserve_capacity(body.len());
                    respond.send_data(body.clone(), true)?;

                    tokio::spawn(async move {
                        let status = response.await.unwrap().status();
                        drop(permit);
                        drop(x);

                        if !status.is_success() {
                            tracing::error!("Code: {}", status);
                        }
                    });
                }
                tracing::info!("SENT");
            },
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                tracing::debug!("ping");

                let ping = h2::Ping::opaque();
                ping_pong.ping(ping).await?;
            },
            err = rx.changed() => err?,
        }
    }
}

async fn client_loop(addr: &Ipv4Addr, mut channel: async_channel::Receiver<ClientMessage>) -> ! {
    loop {
        match conn(addr, &mut channel).await {
            Ok(()) => unreachable!(),
            Err(e) => tracing::error!("Connection Error occured {e}"),
        }
    }
}

fn initialize_workers(rx: async_channel::Receiver<ClientMessage>) {
    let ips = query_discord_ips();

    for n in 0..WORKER_COUNT_PER_IPS {
        for ip in &ips {
            let thread_name = format!("{}/{WORKER_COUNT_PER_IPS}@{ip}", n + 1);

            let rx = rx.clone();
            let ip = ip.clone();

            std::thread::Builder::new().name(thread_name).spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();

                rt.block_on(client_loop(&ip, rx));
            }).unwrap();

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    drop(rx);
}

fn main() {
    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(true)
        .compact();

    tracing_subscriber::fmt().event_format(format).with_max_level(tracing::Level::INFO).init();

    let (tx, rx) = async_channel::bounded::<ClientMessage>(256);
    initialize_workers(rx);

    loop {
        let mut f = std::fs::File::open("/home/ray/projects/yanorei32/discord-webhook-configurator/concat.txt").unwrap();
        use std::io::prelude::*;
        let mut buffer = String::new();
        f.read_to_string(&mut buffer).unwrap();

        const SCALER: usize = 4;

        let targets: Vec<http::Uri> = itertools::repeat_n(
            buffer.split('\n').filter_map(|v| v.parse().ok()),
            SCALER,
        ).flatten().collect();

        let mut headers = http::header::HeaderMap::new();
        headers.insert(http::header::CONTENT_TYPE, "application/json".parse().unwrap());
        headers.insert(http::header::USER_AGENT, "WebHookSender/0.1.0".parse().unwrap());
        headers.insert(http::header::HOST, "discord.com".parse().unwrap());

        let job_count = targets.len();
        let worker_count = tx.receiver_count();
        let chunk_size = usize::max(usize::div_ceil(job_count, worker_count), HTTP2_SETTINGS_MAX_CONCURRENT_STREAMS);
        // let chunk_size = usize::min(chunk_size, 10);

        let chunk_count = usize::div_ceil(job_count, chunk_size);

        let depth = usize::div_ceil(job_count, worker_count * HTTP2_SETTINGS_MAX_CONCURRENT_STREAMS);

        tracing::info!("Jobs: {job_count}, Worker: {worker_count}, Chunk: {chunk_size} x {chunk_count}, Depth: {depth}");

        std::thread::sleep(std::time::Duration::from_secs(3));

        tracing::info!("Start!");

        if true {
            let request = Arc::new(Request {
                method: http::method::Method::POST,
                headers: headers,
                body: bytes::Bytes::from(format!("{{\"content\": \"Hello World ({job_count})\"}}")),
            });

            for target_chunk in targets.chunks(chunk_size) {
                tx.send_blocking(ClientMessage::Request((
                    target_chunk.into(),
                    request.clone(),
                ))).unwrap();
            }

            let request_weak = Arc::downgrade(&request);
            drop(request);

            std::thread::Builder::new().spawn(move || {
                loop {
                    let count = request_weak.strong_count();
                    tracing::info!("Strongs: {count}");

                    if count == 0 {
                        break;
                    }

                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }).unwrap();
        }

        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
