// dummy sentry type that panics if used
// initial snippt by fakeshadow:
// https://gist.github.com/fakeshadow/8b69827803d219f3cccbe85b6d2892bf

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use actix_service::{Service, Transform};

pub struct Sentry;

impl Sentry {
    pub fn new() -> Self {
        Self {}
    }
}

impl<S, Req> Transform<S, Req> for Sentry
where
    S: Service<Req> + 'static,
    S::Future: 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Transform = FeatureMiddleware<S>;
    type InitError = ();
    #[allow(clippy::type_complexity)]
    type Future = Pin<Box<dyn Future<Output = Result<Self::Transform, Self::InitError>>>>;

    fn new_transform(&self, _: S) -> Self::Future {
        unimplemented!("dummy sentry middleware");
    }
}

pub struct FeatureMiddleware<S> {
    s: std::marker::PhantomData<S>,
}

impl<S, Req> Service<Req> for FeatureMiddleware<S>
where
    S: Service<Req>,
    S::Future: 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    #[allow(clippy::type_complexity)]
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        unimplemented!("dummy sentry middleware");
    }

    fn call(&self, _: Req) -> Self::Future {
        unimplemented!("dummy sentry middleware");
    }
}
