use std::{convert::TryFrom, net::SocketAddr, sync::Arc};

use headers::HeaderValue;
use hyper::{
    client::HttpConnector,
    header::HOST,
    http::uri::{Authority, Scheme},
    Body, Client, Uri,
};
use hyper_rustls::HttpsConnector;
use tower::Service;

use crate::{
    config::Endpoint,
    context::RequestInfo,
    http::{upstream_unavailable, HyperRequest, HyperResponse, ResponseFuture},
    load_balance::{Context, LoadBalanceStrategy},
};

#[derive(Clone)]
pub struct HttpClient {
    client: hyper::Client<HttpsConnector<HttpConnector>, Body>,
    pub(crate) strategy: Arc<Box<dyn LoadBalanceStrategy>>,
}

impl HttpClient {
    pub fn new(strategy: Arc<Box<dyn LoadBalanceStrategy>>) -> Self {
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();

        let inner: Client<_, hyper::Body> = Client::builder().build(https);

        HttpClient {
            client: inner,
            strategy,
        }
    }

    pub async fn do_forward<'a>(
        &mut self,
        context: &'a Context<'a>,
        mut req: HyperRequest,
        endpoint: &'a str,
    ) -> Result<HyperResponse, hyper::Error> {
        let mut parts = req.uri().clone().into_parts();

        let authority = Authority::try_from(endpoint).unwrap();
        parts.authority = Some(authority);

        *req.uri_mut() = Uri::from_parts(parts).expect("build uri failed");

        self.strategy.on_send_request(endpoint);
        let resp = Service::call(&mut self.client, req).await;
        self.strategy.on_request_done(endpoint);

        resp
    }
}

#[derive(Debug, Clone)]
pub struct ForwardInfo {
    pub overwrite_host: bool,
    pub upstream_scheme: Scheme,
    pub upstream_endpoints: Vec<Endpoint>,
}

#[derive(Clone)]
pub struct Fowarder {
    client: HttpClient,
    forward_info: ForwardInfo,
}

impl Fowarder {
    pub fn new(client: HttpClient, forward_info: ForwardInfo) -> Self {
        Fowarder {
            client,
            forward_info,
        }
    }
}

impl Service<HyperRequest> for Fowarder {
    type Response = HyperResponse;
    type Error = crate::Error;
    type Future = ResponseFuture;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.client.client.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, mut req: HyperRequest) -> Self::Future {
        let Fowarder {
            mut client,
            forward_info,
        } = self.clone();

        let info = req
            .extensions()
            .get::<RequestInfo>()
            .expect("RequestInfo must exist")
            .clone();

        info.append_proxy_headers(&mut req);

        Box::pin(async move {
            if forward_info.overwrite_host {
                // set host, use upstream host
                let host = req.uri().host().expect("get host failed");
                let host = HeaderValue::from_str(host).expect("HeaderValue failed");
                req.headers_mut().insert(HOST, host);
            }

            let mut parts = req.uri().clone().into_parts();
            parts.scheme = Some(forward_info.upstream_scheme.clone());
            *req.uri_mut() = Uri::from_parts(parts).expect("build uri failed");

            let ctx = Context {
                remote_addr: &info.remote_addr,
                upstream_addrs: &forward_info.upstream_endpoints[..],
            };

            let endpoint = client.strategy.select_endpoint(&ctx);

            let resp = client.do_forward(&ctx, req, endpoint).await;

            resp.map_err(Into::into)
        })
    }
}
