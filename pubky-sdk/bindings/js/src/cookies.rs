// ## Why inline JS here?
// Rust + wasm-bindgen can *technically* mirror the same logic by reflecting on `globalThis.fetch`, constructing `Closure`s around the callback-based `getAllCookies` / `removeCookie` APIs, and wiring them into a `Promise`. I experimented with that approach first, but the result was brittle for a few reasons:
// 1. `fetch-cookie` exposes its `cookieJar` field and the underlying `tough-cookie` store only dynamically. Expressing Node\'s duck-typed callbacks with `wasm-bindgen` requires lots of `JsValue` casting, manual `Closure` lifetimes, and extra error handling that is hard to read and easy to leak.
// 2. Browser support needs a completely different code path (`document.cookie`), so the Rust version would still have to toggle on `cfg(target_arch = "wasm32")` and dive back into JS APIs there.
// 3. wasm-bindgen already emits a JS "snippet" file for inline glue. Hooking into that mechanism means we can write a small, idiomatic async function in JS while keeping the Rust surface area clean (`async fn clear_session_cookie(...)`).

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(inline_js = r#"
export async function removeSessionCookie(name) {
  const global = globalThis;
  const fetchImpl = global.fetch;

  if (fetchImpl && fetchImpl.cookieJar) {
    const jar = fetchImpl.cookieJar;
    const getAllCookies =
      typeof jar.getAllCookies === "function"
        ? jar.getAllCookies.bind(jar)
        : jar.store && typeof jar.store.getAllCookies === "function"
          ? jar.store.getAllCookies.bind(jar.store)
          : undefined;

    if (typeof getAllCookies === "function") {
      const cookies = await new Promise((resolve, reject) => {
        getAllCookies((err, items) => {
          if (err) {
            reject(err);
            return;
          }

          if (!Array.isArray(items)) {
            resolve([]);
            return;
          }

          resolve(items);
        });
      });

      const removals = cookies
        .filter((cookie) => cookie && cookie.key === name)
        .map((cookie) => {
          const store = jar.store;
          if (
            !store ||
            typeof store.removeCookie !== "function" ||
            typeof cookie.domain !== "string" ||
            typeof cookie.path !== "string"
          ) {
            return Promise.resolve();
          }

          return new Promise((resolve, reject) => {
            store.removeCookie(cookie.domain, cookie.path, cookie.key, (err) => {
              if (err) {
                reject(err);
              } else {
                resolve();
              }
            });
          });
        });

      if (removals.length > 0) {
        await Promise.all(removals);
      }
    }
  }

  if (typeof document !== "undefined" && typeof document.cookie === "string") {
    const attributes = ["Max-Age=0", "Path=/", "SameSite=Lax"];

    try {
      if (global.location && global.location.protocol === "https:") {
        attributes.push("Secure");
      }
    } catch (_) {
      // Ignore access errors (e.g. when location is unavailable).
    }

    document.cookie = `${name}=; ${attributes.join("; ")}`;
  }
}
"#)]
extern "C" {
    #[wasm_bindgen(catch, js_name = "removeSessionCookie")]
    pub async fn remove_session_cookie_js(name: &str) -> Result<(), JsValue>;
}

/// Remove the session cookie for the provided user id from the current runtime.
///
/// In Node.js environments this targets the `fetch-cookie` jar that wraps the
/// global `fetch` implementation (see `node-header.cjs`). In browser contexts it
/// falls back to clearing `document.cookie` for the given name.
#[cfg(target_arch = "wasm32")]
pub async fn clear_session_cookie(name: &str) -> Result<(), JsValue> {
    remove_session_cookie_js(name).await
}
