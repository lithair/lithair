use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::{Response, StatusCode};
use std::convert::Infallible;

/// Build a JSON error response with a uniform shape
/// {
///   "error": "snake_code",
///   "message": "Human readable detail"
/// }
type RespBody = BoxBody<Bytes, Infallible>;
type Resp = Response<RespBody>;

#[inline]
fn body_from<T: Into<Bytes>>(data: T) -> RespBody {
    Full::new(data.into()).boxed()
}

pub fn json_error(status: StatusCode, code: &str, message: &str) -> Resp {
    let body = format!(r#"{{"error":"{}","message":"{}"}}"#, code, message);
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(body_from(body))
        .unwrap()
}

/// 405 Method Not Allowed with Allow header
pub fn method_not_allowed(allowed: &str) -> Resp {
    let body = format!(r#"{{"error":"method_not_allowed","allow":"{}"}}"#, allowed);
    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .header("content-type", "application/json")
        .header("allow", allowed)
        .body(body_from(body))
        .unwrap()
}
