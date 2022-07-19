use std::net::SocketAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;
use drain::Watch;
use hyper::server::conn::Http;
use tokio::net::TcpListener;
use tower::Service;
use tracing::Instrument;

use crate::config::SharedData;
use crate::services::{ConnService, GatewayService};
use crate::trace::TraceExecutor;

pub struct Server {
    shared_data: SharedData,
}

impl Server {
    pub fn new(shared_data: SharedData) -> Self {
        Server { shared_data }
    }

    pub async fn run(self, addr: SocketAddr, watch: Watch) -> crate::Result<()> {
        let Server { shared_data } = self;

        let http_svc = GatewayService::new(shared_data);

        let http = Http::new().with_executor(TraceExecutor::new());

        let listener = TcpListener::bind(addr).await?;

        tracing::info!("server listen on {:?}", addr);

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
