use crate::{
    body::{boxed, BoxBody},
    server::NamedService,
};
use http::{Request, Response};
use hyper::Body;
use pin_project::pin_project;
use std::{
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::ServiceExt;
use tower_service::Service;

/// A [`Service`] router.
#[derive(Debug, Default, Clone)]
pub struct Routes {
    router: axum::Router,
}

impl Routes {
    pub(crate) fn new<S>(svc: S) -> Self
    where
        S: Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + 'static,
        S::Future: Send + 'static,
        S::Error: Into<crate::Error> + Send,
    {
        let router = axum::Router::new().fallback(unimplemented);
        Self { router }.add_service(svc)
    }

    pub(crate) fn add_service<S>(mut self, svc: S) -> Self
    where
        S: Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible>
            + NamedService
            + Clone
            + Send
            + 'static,
        S::Future: Send + 'static,
        S::Error: Into<crate::Error> + Send,
    {
        // inject the GrpcMethod extension value if it is gRPC request.
        let svc = svc.map_request(|mut req: Request<_>| {
            S::grpc_method(req.uri().path()).and_then(|val| req.extensions_mut().insert(val));
            req
        });
        let svc = svc.map_response(|res| res.map(axum::body::boxed));
        self.router = self
            .router
            .route_service(&format!("/{}/*rest", S::NAME), svc);
        self
    }

    pub(crate) fn prepare(self) -> Self {
        Self {
            // this makes axum perform update some internals of the router that improves perf
            // see https://docs.rs/axum/latest/axum/routing/struct.Router.html#a-note-about-performance
            router: self.router.with_state(()),
        }
    }
}

async fn unimplemented() -> impl axum::response::IntoResponse {
    let status = http::StatusCode::OK;
    let headers = [("grpc-status", "12"), ("content-type", "application/grpc")];
    (status, headers)
}

impl Service<Request<Body>> for Routes {
    type Response = Response<BoxBody>;
    type Error = crate::Error;
    type Future = RoutesFuture;

    #[inline]
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        RoutesFuture(self.router.call(req))
    }
}

#[pin_project]
pub struct RoutesFuture(#[pin] axum::routing::future::RouteFuture<Body, Infallible>);

impl fmt::Debug for RoutesFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RoutesFuture").finish()
    }
}

impl Future for RoutesFuture {
    type Output = Result<Response<BoxBody>, crate::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match futures_util::ready!(self.project().0.poll(cx)) {
            Ok(res) => Ok(res.map(boxed)).into(),
            Err(err) => match err {},
        }
    }
}
