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

use crate::config::RuntimeConfig;

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

    let http_addr = rtcfg.http_addr;
    let config = rtcfg.config.clone();
    let shared_data = rtcfg.shared_data.clone();
    let shared_data_cloned = shared_data.clone();

    let config_notify = rtcfg.start_watch_config();

    tokio::spawn(async move {
        let srv = Server::new(shared_data_cloned);
        let ret = srv.run(http_addr, watch).await;

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

    tokio::spawn(
        async move { adminapi::run(config.clone(), shared_data.clone(), config_notify).await },
    );

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("got ctrl_c, shutting down...")
        }
    }

    tx.drain().await;

    Ok(())
}
