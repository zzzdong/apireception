use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use arc_swap::ArcSwap;
use drain::Watch;
use futures::Future;
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

use crate::config::SharedData;
use crate::services::{ConnService, HttpService};
use crate::trace::TraceExecutor;

pub struct Server {
    shared_data: Arc<ArcSwap<SharedData>>,
}

impl Server {
    pub fn new(shared_data: Arc<ArcSwap<SharedData>>) -> Self {
        Server {
            shared_data
        }
    }

    pub async fn run(self, addr: SocketAddr, watch: Watch) -> crate::Result<()> {
        let Server { shared_data } = self;

        let http_svc = HttpService::new(shared_data);

        let http = Http::new().with_executor(TraceExecutor::new());

        let listener = TcpListener::bind(addr).await?;

        let conn_svc = ConnService::new(http_svc, http, watch.clone());

        loop {
            tokio::select! {
                ret = listener.accept() => {
                    match ret {
                        Ok((stream, remote_addr)) => {
                            let mut conn_svc = conn_svc.clone();
                            let span = tracing::debug_span!("connection", %remote_addr);
                            let _enter = span.enter();
                            let fut = async move {
                                let ret = Service::call(&mut conn_svc, stream).await;
                                tracing::debug!(?ret, "handle connection done");
                            };
                            tokio::spawn(fut.in_current_span());
                        }
                        Err(e) => {
                            tracing::error!("accept failed, {:?}", e);
                        }
                    }
                }
                _shutdown = watch.clone().signaled() => {
                    tracing::info!("stoping accept");
                    break;
                }
            }
        }

        Ok(())
    }
}
