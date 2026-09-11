use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::runtime::Builder;
use tokio::sync::mpsc::Receiver;
use tokio::sync::{watch, OwnedSemaphorePermit};
use tokio::task::JoinSet;

use fah_common::connections::{ConnectionGauge, OpenConnection};

use crate::proxy::Proxy;

const RUNTIME_SHUTDOWN: Duration = Duration::from_secs(1);

pub(crate) struct Accepted<S> {
    pub(crate) stream: S,
    pub(crate) peer: SocketAddr,
    pub(crate) permit: OwnedSemaphorePermit,
    pub(crate) open: OpenConnection<ConnectionGauge>,
}

pub(crate) type Handoff = Accepted<std::net::TcpStream>;

pub(crate) type ProxyFactory = Arc<dyn Fn() -> Proxy + Send + Sync>;

pub(crate) fn spawn_domain(
    index: usize,
    inbox: Receiver<Handoff>,
    stop: watch::Receiver<bool>,
    drain: Duration,
    make_proxy: ProxyFactory,
) -> io::Result<JoinHandle<()>> {
    let (built, ready) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name(format!("fah-http-{index}"))
        .spawn(move || {
            let runtime = match Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = built.send(Err(err));
                    return;
                }
            };
            let _ = built.send(Ok(()));
            drop(built);
            runtime.block_on(serve_domain(inbox, stop, drain, make_proxy));
            runtime.shutdown_timeout(RUNTIME_SHUTDOWN);
        })?;
    ready.recv().unwrap_or_else(|_| {
        Err(io::Error::other(
            "HTTP domain thread exited before building its runtime",
        ))
    })?;
    Ok(thread)
}

async fn serve_domain(
    mut inbox: Receiver<Handoff>,
    mut stop: watch::Receiver<bool>,
    drain: Duration,
    make_proxy: ProxyFactory,
) {
    let proxy = Arc::new(make_proxy());
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            accepted = inbox.recv() => match accepted {
                Some(accepted) => serve_one(&mut tasks, &proxy, accepted),
                None => break,
            },
            _ = stop.changed() => {
                inbox.close();
                while let Some(accepted) = inbox.recv().await {
                    serve_one(&mut tasks, &proxy, accepted);
                }
                break;
            }
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
    let drained =
        tokio::time::timeout(drain, async { while tasks.join_next().await.is_some() {} }).await;
    if drained.is_err() {
        tracing::warn!(
            open = tasks.len(),
            "HTTP domain drain timed out; aborting the remaining connections"
        );
    }
    tasks.shutdown().await;
}

fn serve_one(tasks: &mut JoinSet<()>, proxy: &Arc<Proxy>, accepted: Handoff) {
    let Accepted {
        stream,
        peer,
        permit,
        open,
    } = accepted;
    let stream = match TcpStream::from_std(stream) {
        Ok(stream) => stream,
        Err(err) => {
            tracing::debug!(%peer, error = %err, "could not register a handed-off socket");
            return;
        }
    };
    let proxy = Arc::clone(proxy);
    tasks.spawn(async move {
        let _permit = permit;
        let _open = open;
        proxy.serve_connection(stream, peer).await;
    });
}
