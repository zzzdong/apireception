use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Future;
use hyper::server::conn::AddrStream;
use hyper::service::make_service_fn;
use hyper::{server::conn::Http, service::service_fn};
use hyper::{Body, Request, Response};
use tokio::io;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    select,
};
use tower::Service;
use tracing::Instrument;
use tracing::{debug, warn};

pub struct Server {}

impl Server {
    pub async fn run(self, addr: SocketAddr) -> anyhow::Result<()> {
        let service = service_fn(move |req: HyperRequest| {
            async move {
                // tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let resp = Response::builder().header("X-Req-Method", req.method().to_string());
                // .header("X-Remote-Addr", remote_addr.to_string());
                let body = Body::from("Hello, World!\n");

                Ok::<_, hyper::Error>(resp.body(body).unwrap())
            }
        });

        let http = Http::new().with_executor(TraceExecutor::new());

        let listener = TcpListener::bind("0.0.0.0:5000").await?;

        let (tx, watch) = drain::channel();

        let mut serve = ServeHttp::new(service, http, watch);

        loop {
            if let Ok((stream, remote_addr)) = listener.accept().await {
                let mut serve = serve.clone();
                tokio::spawn(async move {
                    let span = tracing::debug_span!("remote_addr", %remote_addr);
                    let _enter = span.enter();
                    let ret = serve.call(stream).await;
                    println!("ret={:?}", ret);
                });
            }
        }
    }
}

type HyperRequest = hyper::Request<hyper::Body>;
type HyperResponse = hyper::Response<hyper::Body>;
type HttpServer = hyper::server::conn::Http<TraceExecutor>;

#[derive(Clone, Debug, Default)]
pub struct TraceExecutor(());

impl TraceExecutor {
    pub fn new() -> Self {
        Self(())
    }
}

impl<F> hyper::rt::Executor<F> for TraceExecutor
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    #[inline]
    fn execute(&self, f: F) {
        tokio::spawn(f.in_current_span());
    }
}

#[derive(Clone, Debug)]
pub struct ServeHttp<S> {
    inner: S,
    server: HttpServer,
    drain: drain::Watch,
}

impl<S> ServeHttp<S> {
    pub fn new(svc: S, server: HttpServer, drain: drain::Watch) -> Self {
        ServeHttp {
            inner: svc,
            server,
            drain,
        }
    }
}

impl<I, S> Service<I> for ServeHttp<S>
where
    I: io::AsyncRead + io::AsyncWrite + PeerAddr + Send + Unpin + 'static,
    S: Service<HyperRequest, Response = HyperResponse, Error = hyper::Error>
        + Clone
        + Unpin
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = ();
    type Error = anyhow::Error;
    type Future = Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, io: I) -> Self::Future {
        let Self {
            server,
            inner,
            drain,
        } = self.clone();

        Box::pin(async move {
            let mut conn = server.serve_connection(io, inner);
            tokio::select! {
                res = &mut conn => {
                    debug!(?res, "The client is shutting down the connection");
                    res?
                }
                shutdown = drain.signaled() => {
                    debug!("The process is shutting down the connection");
                    Pin::new(&mut conn).graceful_shutdown();
                    shutdown.release_after(conn).await?;
                }
            }
            Ok(())
        })
    }
}

pub trait PeerAddr {
    fn peer_addr(&self) -> std::io::Result<SocketAddr>;
}

impl PeerAddr for tokio::net::TcpStream {
    fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        tokio::net::TcpStream::peer_addr(self)
    }
}

impl<T: PeerAddr> PeerAddr for tokio_rustls::client::TlsStream<T> {
    fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.get_ref().0.peer_addr()
    }
}

impl<T: PeerAddr> PeerAddr for tokio_rustls::server::TlsStream<T> {
    fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.get_ref().0.peer_addr()
    }
}
