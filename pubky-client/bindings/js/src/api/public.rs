//! Wasm bindings for the /pub/ api

use wasm_bindgen::prelude::*;

use crate::js_result::JsResult;

use super::super::constructor::Client;

#[wasm_bindgen]
impl Client {
    /// Returns a list of Pubky urls (as strings).
    ///
    /// - `url`:     The Pubky url (string) to the directory you want to list its content.
    /// - `cursor`:  Either a full `pubky://` Url (from previous list response),
    ///                 or a path (to a file or directory) relative to the `url`
    /// - `reverse`: List in reverse order
    /// - `limit`    Limit the number of urls in the response
    /// - `shallow`: List directories and files, instead of flat list of files.
    #[wasm_bindgen]
    pub async fn list(
        &self,
        url: &str,
        cursor: Option<String>,
        reverse: Option<bool>,
        limit: Option<u16>,
        shallow: Option<bool>,
    ) -> JsResult<Vec<String>> {
        let mut builder = self
            .0
            .list(url)?
            .reverse(reverse.unwrap_or(false))
            .limit(limit.unwrap_or(u16::MAX))
            .shallow(shallow.unwrap_or(false));

        if let Some(cursor_val) = &cursor {
            builder = builder.cursor(cursor_val);
        }

        let urls = builder.send().await?;
        Ok(urls)
    }
}
