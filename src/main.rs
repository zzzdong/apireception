mod adminapi;
mod config;
mod context;
mod error;
mod forwarder;
mod health;
mod http;
mod load_balance;
mod matcher;
mod peer_addr;
mod plugins;
mod router;
mod runtime;
mod server;
mod services;
mod trace;
mod upstream;

use std::process::exit;

pub use error::{Error, Result};

use hyper::http::uri::Scheme;
use server::Server;

use crate::{adminapi::AdminApi, runtime::RuntimeConfig};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    match run().await {
        Ok(_) => {
            println!("server run done, exit...");
        }
        Err(e) => {
            println!("server run error: {:?}", e);
        }
    }
}

async fn run() -> Result<()> {
    let cfg = config::Config::load_file("config/config.yaml")?;

    tracing::debug!(?cfg, "load config done");

    let (drain_tx, drain_rx) = drain::channel();
    let rtcfg = RuntimeConfig::new(cfg, drain_rx).await?;

    rtcfg.start_watch_config();

    let rtcfg_cloned = rtcfg.clone();

    // Serve HTTP
    tokio::spawn(async move {
        let srv = Server::new(Scheme::HTTP, rtcfg_cloned.shared_data);
        let ret = srv.run(rtcfg_cloned.http_addr, rtcfg_cloned.watch).await;

        match ret {
            Ok(_) => {
                tracing::info!("http server done");
            }
            Err(err) => {
                tracing::error!(?err, "http server error");
                exit(1);
            }
        }
    });

    // TODO: add serve https
    let rtcfg_cloned = rtcfg.clone();

    if rtcfg_cloned.config.admin.enable {
        let adminapi_addr = rtcfg.adminapi_addr.unwrap();
        tokio::spawn(async move {
            let adminapi = AdminApi::new(rtcfg_cloned);
            match adminapi.run(adminapi_addr).await {
                Ok(_) => {
                    tracing::info!("adminapi server done");
                }
                Err(err) => {
                    tracing::error!(?err, "adminapi server error");
                }
            }
        });
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("got ctrl_c, shutting down...");
            let _shutdown = rtcfg.watch.ignore_signaled();
        }
    }

    drain_tx.drain().await;

    Ok(())
}
