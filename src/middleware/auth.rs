use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpResponse,
    body::EitherBody,
};
use futures::future::{ok, Ready};
use std::future::Future;
use std::pin::Pin;
use base64::{engine::general_purpose, Engine as _};
use crate::config::Config;

pub struct AuthMiddleware {
    config: Config,
}

impl AuthMiddleware {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl<S, B> Transform<S, ServiceRequest> for AuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Transform = AuthMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(AuthMiddlewareService {
            service,
            config: self.config.clone(),
        })
    }
}

pub struct AuthMiddlewareService<S> {
    service: S,
    config: Config,
}

impl<S, B> Service<ServiceRequest> for AuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        if let Some(password) = &self.config.security.auth_password {
            if let Some(auth_header) = req.headers().get("Authorization") {
                if let Ok(auth_str) = auth_header.to_str() {
                    if let Some(basic_auth) = auth_str.strip_prefix("Basic ") {
                        if let Ok(decoded) = general_purpose::STANDARD.decode(basic_auth) {
                            if let Ok(decoded_str) = String::from_utf8(decoded) {
                                if decoded_str == *password {
                                    // Authentication successful, continue with the request
                                    let fut = self.service.call(req);
                                    return Box::pin(async move {
                                        let res = fut.await?;
                                        Ok(res.map_into_left_body())
                                    });
                                }
                            }
                        }
                    }
                }
            }
            
            // Authentication failed, return unauthorized response
            Box::pin(async move {
                let response = HttpResponse::Unauthorized()
                    .insert_header(("WWW-Authenticate", "Basic realm=\"Aperio API\""))
                    .finish();
                Ok(req.into_response(response).map_into_right_body())
            })
        } else {
            // No auth password configured, allow all requests
            let fut = self.service.call(req);
            Box::pin(async move {
                let res = fut.await?;
                Ok(res.map_into_left_body())
            })
        }
    }
}