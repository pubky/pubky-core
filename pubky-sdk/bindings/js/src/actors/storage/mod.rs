mod public;
mod session;
pub mod stats;

pub use public::PublicStorage;
pub use session::SessionStorage;

use crate::js_error::JsResult;
use futures_util::StreamExt;
use js_sys::Uint8Array;
use wasm_bindgen::JsValue;
use wasm_streams::ReadableStream;
use web_sys::{Headers, Response, ResponseInit};

pub(crate) fn response_to_web_response(resp: reqwest::Response) -> JsResult<Response> {
    let status = resp.status();
    let headers_map = resp.headers().clone();

    let stream = resp.bytes_stream().map(|chunk| match chunk {
        Ok(bytes) => Ok(JsValue::from(Uint8Array::from(bytes.as_ref()))),
        Err(err) => Err(JsValue::from_str(&err.to_string())),
    });

    let readable_stream = ReadableStream::from_stream(stream);
    let web_stream = readable_stream.into_raw();

    let js_headers = Headers::new()?;
    for (name, value) in headers_map.iter() {
        let value_str = value
            .to_str()
            .map_err(|_| JsValue::from_str("invalid header value"))?;
        js_headers.append(name.as_str(), value_str)?;
    }

    let init = ResponseInit::new();
    init.set_status(status.as_u16());
    if let Some(reason) = status.canonical_reason() {
        init.set_status_text(reason);
    }
    let headers_value: JsValue = js_headers.into();
    init.set_headers(&headers_value);

    Response::new_with_opt_readable_stream_and_init(Some(&web_stream), &init).map_err(Into::into)
}
