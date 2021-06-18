use std::net::SocketAddr;

use hyper::server::conn::AddrStream;
use hyper::service::make_service_fn;
use hyper::{server::conn::Http, service::service_fn};
use hyper::{Body, Request, Response};
use mlua::{Error, Function, Lua, Result as LuaResult, Table, UserData, UserDataMethods};
use socket2::{Domain, Socket, Type};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    select,
};

use anyhow::Result;

use crossbeam::channel::Receiver;

struct LuaRequest(SocketAddr, Request<Body>);

impl UserData for LuaRequest {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("remote_addr", |_lua, req, ()| Ok((req.0).to_string()));
        methods.add_method("method", |_lua, req, ()| Ok((req.1).method().to_string()));
        methods.add_async_function("delay", |_, secs: u64| async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            Ok(())
        });
    }
}

pub struct Server {
    // lua: &'static Lua,
// handler: Function<'a>,
}

impl Server {
    pub fn new() -> Server {
        Self {}
    }

    pub fn run_in_thread(rx: Receiver<(std::net::TcpStream, SocketAddr)>) {
        // Configure a runtime that runs everything on the current thread
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");

        // Combine it with a `LocalSet,  which means it can spawn !Send futures...
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, Self::run(rx)).unwrap();
    }

    pub async fn run(rx: Receiver<(std::net::TcpStream, SocketAddr)>) -> LuaResult<()> {
        let http = Http::new().with_executor(LocalExec);

        let lua = Lua::new().into_static();
        let handler: Function = lua
            .load(
                r#"
        function(req)
            req.delay(1);

            return {
                status = 200,
                headers = {
                    ["X-Req-Method"] = req:method(),
                    ["X-Remote-Addr"] = req:remote_addr(),
                },
                body = "Hello, World!\n"
            }
        end
    "#,
            )
            .eval()
            .unwrap();

        loop {
            while let Ok((stream, remote_addr)) = rx.recv() {
                let lua = lua.clone();
                let handler = handler.clone();

                stream.set_nonblocking(true).unwrap();
                stream.set_nodelay(true).unwrap();

                let stream = TcpStream::from_std(stream).unwrap();
                let http = http.clone();

                tokio::task::spawn_local(async move {
                    let ret = http.serve_connection(
                        stream,
                        service_fn(move |req| {
                            let lua = lua.clone();
                            let handler = handler.clone();

                            println!("req from {:?}", remote_addr);

                            async move {
                                tracing::trace!("request from {:?}", remote_addr);

                                // let lua_req = LuaRequest(remote_addr, req);

                                // let lua_resp: Table = handler.call_async(lua_req).await?;

                                // let body = lua_resp
                                //     .get::<_, Option<String>>("body")?
                                //     .unwrap_or_default();

                                // let mut resp = Response::builder()
                                //     .status(lua_resp.get::<_, Option<u16>>("status")?.unwrap_or(200));

                                // if let Some(headers) = lua_resp.get::<_, Option<Table>>("headers")? {
                                //     for pair in headers.pairs::<String, String>() {
                                //         let (h, v) = pair?;
                                //         resp = resp.header(&h, v);
                                //     }
                                // }

                                // Ok::<_, Error>(resp.body(Body::from(body)).unwrap())

                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                let resp = Response::builder()
                                    .header("X-Req-Method", req.method().to_string())
                                    .header("X-Remote-Addr", remote_addr.to_string());
                                let body = Body::from("Hello, World!\n");

                                Ok::<_, hyper::Error>(resp.body(body).unwrap())
                            }
                        }),
                    );

                    if let Err(e) = ret.await {
                        tracing::error!("serve_connection error: {:?}", e);
                    }
                });
            }
        }
    }

    pub fn run_good_server() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .on_thread_start(|| {
                println!("thread started");
            })
            .build()
            .unwrap();

        rt.block_on(async { Self::good_server().await }).unwrap();
    }

    async fn good_server() -> anyhow::Result<()> {
        let http = Http::new();

        let listener = TcpListener::bind("0.0.0.0:5000").await?;

        loop {
            if let Ok((stream, remote_addr)) = listener.accept().await {
                let http = http.clone();
                tokio::spawn(async move {
                    tracing::info!("new tcp connection from {:?}", remote_addr);

                    let ret = http.serve_connection(
                        stream,
                        service_fn(move |req| async move {
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            let resp = Response::builder()
                                .header("X-Req-Method", req.method().to_string())
                                .header("X-Remote-Addr", remote_addr.to_string());
                            let body = Body::from("Hello, World!\n");

                            Ok::<_, hyper::Error>(resp.body(body).unwrap())
                        }),
                    );

                    if let Err(e) = ret.await {
                        tracing::error!("serve_connection error: {:?}", e);
                    }
                });
            }
        }

        Ok(())
    }

    async fn good_server2() -> Result<()> {
        let service = make_service_fn(|socket: &AddrStream| async {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                async move {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let resp = Response::builder().header("X-Req-Method", req.method().to_string());
                    // .header("X-Remote-Addr", remote_addr.to_string());
                    let body = Body::from("Hello, World!\n");

                    Ok::<_, hyper::Error>(resp.body(body).unwrap())
                }
            }))
        });

        let server = hyper::Server::bind(&"0.0.0.0:5000".parse::<std::net::SocketAddr>().unwrap())
            .serve(service);

        server.await?;

        Ok(())
    }
}

// Since the Server needs to spawn some background tasks, we needed
// to configure an Executor that can spawn !Send futures...
#[derive(Clone, Copy, Debug)]
struct LocalExec;

impl<F> hyper::rt::Executor<F> for LocalExec
where
    F: std::future::Future + 'static, // not requiring `Send`
{
    fn execute(&self, fut: F) {
        // This will spawn into the currently running `LocalSet`.
        tokio::task::spawn_local(fut);
    }
}
