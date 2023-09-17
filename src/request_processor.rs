use crate::*;
use tokio::sync::mpsc::{Receiver, UnboundedReceiver};
use tokio::sync::Semaphore;

use crate::channel::*;
use crate::counter::*;
use crate::model::Priority;

pub static LIMITER: Semaphore = Semaphore::const_new(0);
pub static CLIENT: OnceCell<Client> = OnceCell::new();

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

    counter.queued.fetch_sub(1, Ordering::Relaxed);
    let limiter = LIMITER.acquire().await;
    let retry_tx = retry_tx.clone();

    tokio::spawn(async move {
        let _limiter = limiter;

        if request(&ctx).await.is_ok() {
            counter.succeed.fetch_add(1, Ordering::Relaxed);
        } else {
            counter.failed.fetch_add(1, Ordering::Relaxed);

            ctx.ttl -= 1;

            if ctx.ttl != 0 {
                sleep(Duration::from_secs(3)).await;
                counter.queued.fetch_add(1, Ordering::Relaxed);
                retry_tx.send(ctx).unwrap();
            } else {
                counter.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

pub async fn event_loop(
    mut drop_rx: Receiver<()>,
    mut high_priority: UnboundedReceiver<RequestContext>,
    mut low_priority: UnboundedReceiver<RequestContext>,
) {
    loop {
        tokio::select! {
            Some(_) = drop_rx.recv() => {
                let mut n = 0;
                while low_priority.try_recv().is_ok() {
                    n += 1;
                }
                println!("LOW PRIORITY QUEUE {n} DROPPED!");
            }
            Some(ctx) = high_priority.recv() => {
                process_request(ctx, Priority::High).await;
            }
            Some(ctx) = low_priority.recv(), if COUNTERS.get(Priority::High).queued.load(Ordering::Relaxed) == 0 => {
                process_request(ctx, Priority::Low).await;
            }
        }
    }
}
