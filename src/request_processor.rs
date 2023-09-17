use std::time::Duration;

use once_cell::sync::OnceCell;
use tokio::sync::mpsc::{Receiver, UnboundedReceiver};
use tokio::sync::Semaphore;
use tokio::time::sleep;

use crate::channel::{RequestContext, CHANNELS};
use crate::counter::COUNTERS;
use crate::model::Priority;

pub static LIMITER: Semaphore = Semaphore::const_new(0);
pub static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();

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

async fn process_request(mut ctx: RequestContext, p: Priority) {
    let counter = COUNTERS.get(p);
    let retry_tx = CHANNELS.get().unwrap().get_queue(p);

    counter.resolve();
    let limiter = LIMITER.acquire().await;
    let retry_tx = retry_tx.clone();

    tokio::spawn(async move {
        let _limiter = limiter;

        if request(&ctx).await.is_ok() {
            counter.succeed();
        } else {
            ctx.ttl -= 1;

            if ctx.ttl != 0 {
                sleep(Duration::from_secs(3)).await;
                counter.enqueue();
                retry_tx.send(ctx).unwrap();
            } else {
                counter.failed();
            }
        }
    });
}

pub async fn event_loop(
    mut flush_rx: Receiver<()>,
    mut high_priority: UnboundedReceiver<RequestContext>,
    mut low_priority: UnboundedReceiver<RequestContext>,
) {
    loop {
        tokio::select! {
            Some(_) = flush_rx.recv() => {
                let mut n = 0;
                while low_priority.try_recv().is_ok() {
                    n += 1;
                }
                COUNTERS.get(Priority::Low).resolve_n(n);
                println!("LOW PRIORITY QUEUE {n} FLUSHED!");
            }
            Some(ctx) = high_priority.recv() => {
                process_request(ctx, Priority::High).await;
            }
            Some(ctx) = low_priority.recv(), if COUNTERS.get(Priority::High).is_queue_empty()  => {
                process_request(ctx, Priority::Low).await;
            }
        }
    }
}
