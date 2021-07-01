use std::net::SocketAddr;

mod config;
mod error;
mod health;
mod http;
mod matcher;
mod peer_addr;
mod router;
mod server;
mod services;
mod trace;
mod upstream;

pub use error::{Error, Result};

use server::Server;

use crate::config::RuntimeConfig;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::SubscriberBuilder::default().init();

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

    let rtcfg = RuntimeConfig::new(&cfg)?;

    let (tx, watch) = drain::channel();

    let http_addr = rtcfg.http_addr;

    tokio::spawn(async move {
        let srv = Server::new(rtcfg.shared_data);
        let ret = srv.run(http_addr, watch).await;
        tracing::debug!(?ret, "http server done");
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("got ctrl_c, shutting down...")
        }
    }

    tx.drain().await;

    Ok(())
}
