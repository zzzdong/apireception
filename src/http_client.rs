use std::sync::Arc;

use hyper::{client::HttpConnector, Body, Client};
use hyper_rustls::HttpsConnector;
use tower::Service;

use crate::{
    http::{HyperRequest, HyperResponse, ResponseFuture},
    upstream::LoadBalanceStrategy,
};

#[derive(Clone)]
pub struct GatewayClient {
    inner: hyper::Client<HttpsConnector<HttpConnector>, Body>,
    pub(crate) strategy: Arc<Box<dyn LoadBalanceStrategy>>,
}

impl GatewayClient {
    pub fn new(strategy: Arc<Box<dyn LoadBalanceStrategy>>) -> Self {
        let https = hyper_rustls::HttpsConnector::with_native_roots();

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
            let endpoint = req.uri().clone();
            strategy.on_send_request(&endpoint);
            let resp = inner.call(req).await;
            strategy.on_request_done(&endpoint);
            resp.map_err(Into::into)
        })
    }
}
