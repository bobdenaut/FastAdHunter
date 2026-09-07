use std::future::Future;
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

use crate::connections::OpenConnection;
use crate::https::TlsProxy;
use crate::proxy::Proxy;

const RUNTIME_SHUTDOWN: Duration = Duration::from_secs(1);

pub(crate) struct Accepted<S> {
    pub(crate) stream: S,
    pub(crate) peer: SocketAddr,
    pub(crate) permit: OwnedSemaphorePermit,
    pub(crate) open: OpenConnection,
}

impl<S> Accepted<S> {
    pub(crate) fn serve<F, Fut>(self, serve: F) -> impl Future<Output = ()>
    where
        F: FnOnce(S, SocketAddr) -> Fut,
        Fut: Future<Output = ()>,
    {
        let Accepted {
            stream,
            peer,
            permit,
            open,
        } = self;
        let served = serve(stream, peer);
        async move {
            let _permit = permit;
            let _open = open;
            served.await;
        }
    }
}

impl Accepted<std::net::TcpStream> {
    pub(crate) fn register(self) -> Option<Accepted<TcpStream>> {
        let Accepted {
            stream,
            peer,
            permit,
            open,
        } = self;
        match TcpStream::from_std(stream) {
            Ok(stream) => Some(Accepted {
                stream,
                peer,
                permit,
                open,
            }),
            Err(err) => {
                tracing::debug!(%peer, error = %err, "could not register a handed-off socket");
                None
            }
        }
    }
}

impl Accepted<TcpStream> {
    pub(crate) fn detach(self) -> Option<Accepted<std::net::TcpStream>> {
        let Accepted {
            stream,
            peer,
            permit,
            open,
        } = self;
        match stream.into_std() {
            Ok(stream) => Some(Accepted {
                stream,
                peer,
                permit,
                open,
            }),
            Err(err) => {
                tracing::debug!(%peer, error = %err, "could not detach an accepted socket");
                None
            }
        }
    }
}

pub(crate) enum Handoff {
    Http(Accepted<std::net::TcpStream>),
    Https(Accepted<std::net::TcpStream>, Arc<TlsProxy>),
}

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
            handoff = inbox.recv() => match handoff {
                Some(handoff) => serve_one(&mut tasks, &proxy, handoff),
                None => break,
            },
            _ = stop.changed() => {
                inbox.close();
                while let Some(handoff) = inbox.recv().await {
                    serve_one(&mut tasks, &proxy, handoff);
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

fn serve_one(tasks: &mut JoinSet<()>, proxy: &Arc<Proxy>, handoff: Handoff) {
    match handoff {
        Handoff::Http(accepted) => {
            let Some(accepted) = accepted.register() else {
                return;
            };
            let proxy = Arc::clone(proxy);
            tasks.spawn(accepted.serve(move |stream, peer| proxy.serve_connection(stream, peer)));
        }
        Handoff::Https(accepted, tls) => {
            let Some(accepted) = accepted.register() else {
                return;
            };
            tasks.spawn(accepted.serve(move |stream, peer| tls.serve_connection(stream, peer)));
        }
    }
}
