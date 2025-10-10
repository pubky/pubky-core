use crate::js_error::JsResult;
use futures_util::StreamExt;
use js_sys::Uint8Array;
use wasm_bindgen::JsValue;
use wasm_streams::ReadableStream;
use web_sys::{Headers, Response, ResponseInit};

pub(crate) async fn apply_list_options(
    mut builder: pubky::ListBuilder<'_>,
    cursor: Option<String>,
    reverse: Option<bool>,
    limit: Option<u16>,
    shallow: Option<bool>,
) -> JsResult<Vec<String>> {
    if let Some(cursor) = cursor {
        builder = builder.cursor(&cursor);
    }
    if let Some(reverse) = reverse {
        builder = builder.reverse(reverse);
    }
    if let Some(limit) = limit {
        builder = builder.limit(limit);
    }
    if let Some(shallow) = shallow {
        builder = builder.shallow(shallow);
    }

    let entries = builder.send().await?;
    let urls = entries
        .into_iter()
        .map(|entry| entry.to_pubky_url())
        .collect();
    Ok(urls)
}

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
