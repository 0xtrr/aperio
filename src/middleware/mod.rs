pub mod request_tracking;

pub use request_tracking::{RequestTracking, get_request_metrics};

use actix_web::{
    http::header::{HeaderValue, CONTENT_SECURITY_POLICY, X_FRAME_OPTIONS, X_CONTENT_TYPE_OPTIONS, X_XSS_PROTECTION, STRICT_TRANSPORT_SECURITY},
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error,
};
use futures::future::{ok, Ready};
use std::future::Future;
use std::pin::Pin;

// Security Headers Middleware
pub struct SecurityHeaders;

impl<S, B> Transform<S, ServiceRequest> for SecurityHeaders
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = SecurityHeadersMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(SecurityHeadersMiddleware { service })
    }
}

pub struct SecurityHeadersMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for SecurityHeadersMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let fut = self.service.call(req);

        Box::pin(async move {
            let mut res = fut.await?;

            // Add security headers
            let headers = res.headers_mut();

            headers.insert(
                X_FRAME_OPTIONS,
                HeaderValue::from_static("DENY"),
            );
            headers.insert(
                X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            headers.insert(
                X_XSS_PROTECTION,
                HeaderValue::from_static("1; mode=block"),
            );
            headers.insert(
                CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("default-src 'self'"),
            );
            headers.insert(
                STRICT_TRANSPORT_SECURITY,
                HeaderValue::from_static("max-age=31536000; includeSubDomains"),
            );

            Ok(res)
        })
    }
}

// CORS Middleware (simplified version)
pub struct Cors {
    allowed_origins: Vec<String>,
}

impl Cors {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self { allowed_origins }
    }

    pub fn restrictive() -> Self {
        Self {
            allowed_origins: vec!["http://localhost:3000".to_string()],
        }
    }
}

impl Clone for Cors {
    fn clone(&self) -> Self {
        Self {
            allowed_origins: self.allowed_origins.clone(),
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for Cors
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = CorsMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(CorsMiddleware { service })
    }
}

pub struct CorsMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for CorsMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let fut = self.service.call(req);

        Box::pin(async move {
            let mut res = fut.await?;

            // Add CORS headers
            let headers = res.headers_mut();
            headers.insert(
                actix_web::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"), // Configure as needed
            );

            Ok(res)
        })
    }
}
