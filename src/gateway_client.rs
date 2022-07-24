use std::sync::Arc;

use hyper::{client::HttpConnector, Body, Client};
use hyper_rustls::HttpsConnector;
use tower::Service;

use crate::{
    http::{HyperRequest, HyperResponse, ResponseFuture},
    load_balance::{LoadBalanceStrategy, Context},
};

#[derive(Clone)]
pub struct GatewayClient {
    inner: hyper::Client<HttpsConnector<HttpConnector>, Body>,
    pub(crate) strategy: Arc<Box<dyn LoadBalanceStrategy>>,
}

impl GatewayClient {
    pub fn new(strategy: Arc<Box<dyn LoadBalanceStrategy>>) -> Self {
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();

        let inner: Client<_, hyper::Body> = Client::builder().build(https);

        GatewayClient { inner, strategy }
    }
}

impl Service<HyperRequest> for GatewayClient {
    type Response = HyperResponse;
    type Error = crate::Error;
    type Future = ResponseFuture;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: HyperRequest) -> Self::Future {
        let GatewayClient {
            mut inner,
            strategy,
        } = self.clone();

        Box::pin(async move {
            let ctx = Context {
                remote_addr: &"127.0.0.1:80".parse::<SocketAddr>().unwrap(),
                upstream_addrs: &endpoints[..],
                req: &req,
            };
            
            // select endpoint

            
            
            // check request
            if req.uri().authority().is_none() {
                return upstream_unavailable();
            }


            append_proxy_headers(&mut req, &ctx.request_info);

            // set host, use upstream host
            let host = req.uri().host().expect("get host failed");
            let host = HeaderValue::from_str(host).expect("HeaderValue failed");
            req.headers_mut().insert(HOST, host);



            let mut parts = req.uri().clone().into_parts();
            parts.scheme = Some(upstream.scheme.clone());

            let authority = strategy.select_endpoint(&ctx);
            let authority =
                authority.and_then(|authority| Authority::try_from(authority.as_str()).ok());
            parts.authority = authority;

            *req.uri_mut() = Uri::from_parts(parts).expect("build uri failed");

            let endpoint = req.uri().clone();
            strategy.on_send_request(&endpoint);
            let resp = inner.call(req).await;
            strategy.on_request_done(&endpoint);
            resp.map_err(Into::into)
        })
    }
}
