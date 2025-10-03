//! WASM wrapper for `pubky::AuthToken`.
//!
//! This type represents a signed, time-bound authentication token produced by a Pubky **Signer**.
//! In browser/Node apps, you’ll most often get it from `AuthFlow.awaitToken()` when you only
//! need to authenticate a user (prove control of a key) without establishing a homeserver session.
//
//  JS quick look:
//
//    import { AuthFlow } from "@synonymdev/pubky";
//
//    const flow = pubky.startAuthFlow("", relay); // no capabilities => auth-only
//    const token = await flow.awaitToken();       // <- AuthToken
//
//    // Who just authenticated?
//    console.log(token.publicKey().z32());
//
//    // Optional: send to your backend and verify there
//    const bytes = token.toBytes();               // Uint8Array
//    // server -> AuthToken.verify(bytes)
//
//  NOTE: `AuthToken.verify(bytes)` does **not** require network access. It checks:
//    - version, signature, allowed timestamp window, replay protection (if using `AuthVerifier` on server).
//

use js_sys::Array;
use wasm_bindgen::prelude::*;

use crate::js_error::JsResult;
use crate::wrappers::keys::PublicKey;

/// AuthToken: signed, time-bound proof of key ownership.
///
/// Returned by [`AuthFlow.awaitToken()`] on the 3rd-party app side when doing **authentication-only**
/// flows (no homeserver session). You can inspect who authenticated and which capabilities were
/// requested, or serialize the token and send it to a backend to verify.
///
/// ### Typical usage
/// ```js
/// // Start an auth-only flow (no capabilities)
/// const flow = pubky.startAuthFlow("", relay);
///
/// // Wait for the signer to approve; returns an AuthToken
/// const token = await flow.awaitToken();
///
/// // Identify the user
/// console.log(token.publicKey().z32());
///
/// // Optionally forward to a server for verification:
/// await fetch("/api/verify", { method: "POST", body: token.toBytes() });
/// ```
///
/// ### Binary format
/// `AuthToken` serializes to a canonical binary (postcard) form; use [`AuthToken.toBytes()`] to get a
/// `Uint8Array`, and [`AuthToken.verify()`] to parse + verify on the server.
#[wasm_bindgen]
pub struct AuthToken(pub(crate) pubky::AuthToken);

#[wasm_bindgen]
impl AuthToken {
    // ---------------------------------------------------------------------
    // Constructors / statics
    // ---------------------------------------------------------------------

    /// Parse and verify an `AuthToken` from its canonical bytes.
    ///
    /// - Verifies version, timestamp freshness window, and signature.
    /// - Throws on invalid/expired/unknown version.
    ///
    /// Use this on your server after receiving `Uint8Array` from the client.
    ///
    /// ```js
    /// import { AuthToken } from "@synonymdev/pubky";
    ///
    /// export async function POST(req) {
    ///   const bytes = new Uint8Array(await req.arrayBuffer());
    ///   const token = AuthToken.verify(bytes); // throws on failure
    ///   return new Response(token.publicKey().z32(), { status: 200 });
    /// }
    /// ```
    #[wasm_bindgen(js_name = "verify")]
    pub fn verify_js(bytes: js_sys::Uint8Array) -> JsResult<AuthToken> {
        let vec = bytes.to_vec();
        let token = pubky::AuthToken::verify(&vec)?; // maps to PubkyJsError on failure
        Ok(AuthToken(token))
    }

    /// Deserialize an `AuthToken` **without** verification.
    ///
    /// Most apps should call [`AuthToken.verify()`]. This is provided for tooling or diagnostics
    /// where you want to inspect the structure first.
    ///
    /// Throws if the bytes cannot be parsed as a valid serialized token.
    #[wasm_bindgen(js_name = "fromBytes")]
    pub fn from_bytes(bytes: js_sys::Uint8Array) -> JsResult<AuthToken> {
        let vec = bytes.to_vec();
        let token = pubky::AuthToken::deserialize(&vec)?; // parse only
        Ok(AuthToken(token))
    }

    // ---------------------------------------------------------------------
    // Instance methods
    // ---------------------------------------------------------------------

    /// Returns the **public key** that authenticated with this token.
    ///
    /// Use `.z32()` on the returned `PublicKey` to get the string form.
    ///
    /// @example
    /// const who = sessionInfo.publicKey.z32();
    #[wasm_bindgen(js_name = "publicKey", getter)]
    pub fn public_key(&self) -> PublicKey {
        // `pubky::PublicKey` implements `Clone`
        PublicKey(self.0.public_key().clone())
    }

    /// Returns the **capabilities** requested by the flow at the time this token was signed.
    ///
    /// Most auth-only flows pass an empty string to `startAuthFlow("", relay)`, so this will
    /// commonly be an empty array.
    ///
    /// Returns: `string[]`, where each item is the canonical entry `"<scope>:<actions>"`.
    ///
    /// Example entry: `"/pub/my.app/:rw"`
    #[wasm_bindgen(getter)]
    pub fn capabilities(&self) -> Array {
        let arr = Array::new();
        for cap in self.0.capabilities().iter() {
            arr.push(&JsValue::from_str(&cap.to_string()));
        }
        arr
    }

    /// Serialize the token to a `Uint8Array` in its **canonical** (postcard) binary format.
    ///
    /// Use this to send the token to a backend for verification.
    ///
    /// ```js
    /// const bytes = token.toBytes();
    /// await fetch("/api/verify", { method: "POST", body: bytes });
    /// ```
    #[wasm_bindgen(js_name = "toBytes")]
    pub fn to_bytes(&self) -> js_sys::Uint8Array {
        let bytes = self.0.serialize();
        js_sys::Uint8Array::from(bytes.as_slice())
    }
}
