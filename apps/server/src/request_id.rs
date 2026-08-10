use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

pub const CORRELATION_ID_HEADER: HeaderName = HeaderName::from_static("x-correlation-id");

tokio::task_local! {
    pub static REQUEST_CORRELATION_ID: String;
}

#[derive(Clone, Debug)]
pub struct RequestCorrelationId(pub String);

pub async fn attach_correlation_id(mut request: Request, next: Next) -> Response {
    let value = request
        .headers()
        .get(&CORRELATION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| Uuid::parse_str(value).is_ok())
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let header_value = HeaderValue::from_str(&value)
        .expect("a UUID correlation identifier is always a valid HTTP header value");

    request
        .extensions_mut()
        .insert(RequestCorrelationId(value.clone()));
    request
        .headers_mut()
        .insert(CORRELATION_ID_HEADER.clone(), header_value.clone());

    let mut response = REQUEST_CORRELATION_ID.scope(value, next.run(request)).await;
    response
        .headers_mut()
        .insert(CORRELATION_ID_HEADER.clone(), header_value);
    response
}
