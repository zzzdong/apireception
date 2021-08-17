mod adminapi;
mod config;
mod context;
mod error;
mod health;
mod http;
mod http_client;
mod matcher;
mod peer_addr;
mod plugins;
mod router;
mod server;
mod services;
mod trace;
mod upstream;

use std::process::exit;

pub use error::{Error, Result};

use server::Server;

use crate::{adminapi::AdminApi, config::RuntimeConfig};

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
    let cfg = config::Config::load("config.yaml")?;

    tracing::debug!(?cfg, "load config done");

    let rtcfg = RuntimeConfig::new(cfg)?;

    let (tx, watch) = drain::channel();

    rtcfg.start_watch_config();

    let rtcfg_cloned = rtcfg.clone();

    tokio::spawn(async move {
        let srv = Server::new(rtcfg_cloned.shared_data);
        let ret = srv.run(rtcfg_cloned.http_addr, watch).await;

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

    tokio::spawn(async move {
        let adminapi = AdminApi::new(rtcfg);
        match adminapi.run().await {
            Ok(_) => {
                tracing::info!("adminapi server done");
            }
            Err(err) => {
                tracing::error!(?err, "adminapi server error");
            }
        }
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("got ctrl_c, shutting down...")
        }
    }

    tx.drain().await;

    Ok(())
}
