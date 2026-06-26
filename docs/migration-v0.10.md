# Migrating to v0.10

This guide is for applications upgrading from `v0.9.x` that already use cookie-based SDK authentication and want that behavior to keep working.

Cookie auth still exists in v0.10, but the SDK now names the cookie-compatible APIs explicitly. The main migration is to replace the old generic signer methods with the new `*Cookie` methods where your app expects a cookie-backed session.

Some cookie-specific APIs are documented or marked as deprecated because cookie auth is now the compatibility path. That is expected for this migration.

## Summary

For cookie-auth applications:

- JS: replace `signer.signup(...)` with `signer.signupCookie(...)` when you need the returned `Session`.
- JS: replace `signer.signin()` with `signer.signinCookie()`.
- JS: replace `signer.signinBlocking()` with `signer.signinCookieBlocking()`.
- Rust: replace `signer.signup(...)` with `signer.signup_cookie(...)` when you need the returned `PubkySession`.
- Rust: replace `signer.signin()` with `signer.signin_cookie()`.
- Rust: replace `signer.signin_blocking()` with `signer.signin_cookie_blocking()`.
- Rust: replace explicit `PubkyAuthFlow` references with `PubkyCookieAuthFlow`.
- JS: replace `startAuthFlow` with `startCookieAuthFlow`.
- JS: replace `resumeAuthFlow` with `resumeCookieAuthFlow`.
- Rust: replace `start_auth_flow` with `start_cookie_auth_flow`.
- Rust: replace `resume_auth_flow` with `resume_cookie_auth_flow`.
- Rust: replace `session.export_secret()` with `session.as_cookie().and_then(|cookie| cookie.export_secret())`.
- Rust: if your code uses cookie metadata fields from `session.info()`, use `session.as_cookie().unwrap().session_info()`.
- Rust: prefer `session.as_cookie().unwrap().write_secret_file(...)` over deprecated `session.write_secret_file(...)`.

## JavaScript SDK

### Direct Signup

In `v0.9.x`, `signup` created the account and returned a cookie-backed session.

```js
// v0.9.x
const session = await signer.signup(homeserver, signupToken);
```

In v0.10, `signup` no longer returns a session, it only signs up. To keep the old cookie behavior, use `signupCookie`.

```js
// v0.10 cookie-compatible
const session = await signer.signupCookie(homeserver, signupToken);
```

If the signup token is optional in your code, keep passing `null` or `undefined` as before.

```js
const session = await signer.signupCookie(homeserver, null);
```

### Direct Signin

In `v0.9.x`, `signin` returned a cookie-backed session and did not take arguments.

```js
// v0.9.x
const session = await signer.signin();
```

In v0.10, use `signinCookie` to keep cookie auth.

```js
// v0.10 cookie-compatible
const session = await signer.signinCookie();
```

For blocking signin:

```js
// v0.9.x
const session = await signer.signinBlocking();

// v0.10 cookie-compatible
const session = await signer.signinCookieBlocking();
```

### Auth Flow / QR Login

If your existing JS app uses `startAuthFlow`, rename it to `startCookieAuthFlow`.

```js
const flow = pubky.startCookieAuthFlow(
  "/pub/my-cool-app/:rw",
  AuthFlowKind.signin(),
  relay,
);

renderQr(flow.authorizationUrl);
const session = await flow.awaitApproval();
```

This remains the cookie auth flow. Do not switch this code to `startGrantAuthFlow` unless you are intentionally changing auth models.

If your app resumes a pending cookie auth flow, rename `resumeAuthFlow` to `resumeCookieAuthFlow`.

```js
const flow = pubky.resumeCookieAuthFlow(savedAuthorizationUrl);
```

### Session Persistence

For browser cookie sessions, `session.export()` still works for the legacy metadata snapshot. The browser must still have the HTTP-only session cookie.

```js
const exported = session.export();

const restored = await pubky.restoreSession(exported);
```

You can also use the explicit cookie view:

```js
const cookie = session.cookie;
if (!cookie) {
  throw new Error("Expected a cookie-backed session");
}

const exported = cookie.export();
const restored = await pubky.restoreSession(exported);
```

In Node.js, the SDK can capture the cookie secret. If your application needs to persist a restartable cookie session, use `session.cookie.exportSecret()` and restore it with `pubky.restoreSession(...)`.

```js
const cookie = session.cookie;
if (!cookie) {
  throw new Error("Expected a cookie-backed session");
}

const secret = await cookie.exportSecret();
const restored = await pubky.restoreSession(secret);
```

Browsers cannot export HTTP-only cookie secrets. In browser apps, keep using the metadata export plus the browser cookie jar.

### JS Checklist

- If your code expects `signup` to return a session, change it to `signupCookie`.
- If your code calls `signin()` with no arguments, change it to `signinCookie()`.
- If your code calls `signinBlocking()` with no arguments, change it to `signinCookieBlocking()`.
- If your code uses `startAuthFlow`, change it to `startCookieAuthFlow`.
- If your code uses `resumeAuthFlow`, change it to `resumeCookieAuthFlow`.
- If your code persists browser sessions with `session.export()`, it can keep doing so for cookie-backed sessions.
- If your code persists Node.js cookie secrets, prefer `session.cookie.exportSecret()`.

## Rust SDK

### Direct Signup

In `v0.9.x`, `signup` created the account and returned a cookie-backed `PubkySession`.

```rust
// v0.9.x
let session = signer.signup(&homeserver, None).await?;
```

In v0.10, `signup` no longer returns a session. To keep the old cookie behavior, use `signup_cookie`.

```rust
// v0.10 cookie-compatible
let session = signer.signup_cookie(&homeserver, None).await?;
```

With a signup token:

```rust
let session = signer
    .signup_cookie(&homeserver, Some("INVITE-123"))
    .await?;
```

### Direct Signin

In `v0.9.x`, `signin` returned a cookie-backed session and did not take arguments.

```rust
// v0.9.x
let session = signer.signin().await?;
```

In v0.10, use `signin_cookie` to keep cookie auth.

```rust
// v0.10 cookie-compatible
let session = signer.signin_cookie().await?;
```

For blocking signin:

```rust
// v0.9.x
let session = signer.signin_blocking().await?;

// v0.10 cookie-compatible
let session = signer.signin_cookie_blocking().await?;
```

### Auth Flow / QR Login

If your app starts auth flows through the `Pubky` facade, rename `start_auth_flow` to `start_cookie_auth_flow`.

```rust
let caps = Capabilities::builder()
    .read_write("/pub/my-cool-app/")
    .finish();

let flow = pubky.start_cookie_auth_flow(&caps, AuthFlowKind::signin())?;
println!("Scan to sign in: {}", flow.authorization_url());

let session = flow.await_approval().await?;
```

If your code names the flow type directly, rename it from `PubkyAuthFlow` to `PubkyCookieAuthFlow`.

```rust
// v0.9.x
use pubky::PubkyAuthFlow;

// v0.10 cookie-compatible
use pubky::PubkyCookieAuthFlow;
```

Builder usage changes the same way.

```rust
// v0.9.x
let flow = PubkyAuthFlow::builder(&caps, AuthFlowKind::signin())
    .relay(relay)
    .start()?;

// v0.10 cookie-compatible
let flow = PubkyCookieAuthFlow::builder(&caps, AuthFlowKind::signin())
    .relay(relay)
    .start()?;
```

This remains the cookie auth flow. Do not switch this code to `start_grant_auth_flow` unless you are intentionally changing auth models.

If your app resumes a pending cookie auth flow, rename `resume_auth_flow` to `resume_cookie_auth_flow`.

```rust
let flow = pubky.resume_cookie_auth_flow(saved_authorization_url)?;
```

### Session Persistence

Cookie-specific session operations now live behind `session.as_cookie()`.

In `v0.9.x`, native Rust apps could export a cookie secret directly from `PubkySession`.

```rust
// v0.9.x
let secret = session.export_secret();
```

In v0.10, export the secret through the cookie view.

```rust
// v0.10 cookie-compatible
let secret = session
    .as_cookie()
    .and_then(|cookie| cookie.export_secret())
    .expect("expected an exportable cookie-backed session");
```

Restore cookie secret tokens with `Pubky::restore_session` or the existing `PubkySession::import_secret` API.

```rust
let restored = pubky.restore_session(&secret).await?;
```

In `v0.9.x`, native Rust apps could write `.sess` files directly from `PubkySession`.

```rust
// v0.9.x
session.write_secret_file("alice.sess")?;
```

In v0.10, `write_secret_file` still exists as a deprecated compatibility method. Prefer the cookie view so the cookie-only operation is explicit.

```rust
// v0.10 cookie-compatible
session
    .as_cookie()
    .expect("expected a cookie-backed session")
    .write_secret_file("alice.sess")?;
```

Reading `.sess` files still works through the `Pubky` facade.

```rust
let restored = pubky.session_from_file("alice.sess").await?;
```

The direct legacy methods `session.export()`, `session.write_secret_file(...)`, `PubkySession::import_secret(...)`, and `PubkySession::from_secret_file(...)` still exist for compatibility, but cookie-specific code should prefer `session.as_cookie()` so it cannot accidentally run against a non-cookie-backed session.

### Session Info

In `v0.9.x`, `session.info()` returned the common cookie session record type, so cookie metadata fields were available directly.

```rust
// v0.9.x
let created_at = session.info().created_at();
let bytes = session.info().serialize();
```

In v0.10, `session.info()` returns minimal auth-agnostic metadata.

```rust
// v0.10 auth-agnostic
let public_key = session.info().public_key();
let capabilities = session.info().capabilities();
```

To keep using cookie-specific fields such as `created_at()` or the old binary serialization format, use the cookie view.

```rust
// v0.10 cookie-compatible
let cookie_record = session
    .as_cookie()
    .expect("expected a cookie-backed session")
    .session_info();

let created_at = cookie_record.created_at();
let bytes = cookie_record.serialize();
```

The old common type was `pubky_common::session::SessionInfo`. The cookie-specific replacement is `pubky_common::session::CookieSessionRecord`, re-exported by the SDK as `pubky::CookieSessionRecord`.

### Deep Link Parsing

If your Rust app implements an authenticator and parses legacy signin or signup deep links directly, the parameter accessors changed.

Use `params()` instead of the old direct methods.

```rust
let params = deep_link.params();

let caps = &params.capabilities;
let relay = &params.relay;
let secret = &params.secret;
```

For signup deep links:

```rust
let params = signup_deep_link.params();

let homeserver = &params.homeserver;
let signup_token = params.signup_token.as_deref();
```

If your code matches on `DeepLink`, make sure it has a fallback or handles all variants. v0.10 adds extra variants, so exhaustive matches written against `v0.9.x` may fail to compile.

### Rust Checklist

- If your code expects `signup` to return `PubkySession`, change it to `signup_cookie`.
- If your code calls `signin()` with no arguments, change it to `signin_cookie()`.
- If your code calls `signin_blocking()` with no arguments, change it to `signin_cookie_blocking()`.
- If your code imports `PubkyAuthFlow`, change it to `PubkyCookieAuthFlow`.
- If your code uses `PubkyAuthFlow::builder`, change it to `PubkyCookieAuthFlow::builder`.
- If your code uses `start_auth_flow`, change it to `start_cookie_auth_flow`.
- If your code uses `resume_auth_flow`, change it to `resume_cookie_auth_flow`.
- If your code uses `session.export_secret()`, change it to `session.as_cookie().and_then(|cookie| cookie.export_secret())`.
- If your code uses `session.write_secret_file(...)`, change it to `session.as_cookie().unwrap().write_secret_file(...)`.
- If your code needs cookie metadata fields, use `session.as_cookie().unwrap().session_info()`.

## Capability Path Matching

v0.10 tightens capability path matching. A trailing slash is significant.

Directory scopes should end with `/`.

```text
/pub/app/:rw covers /pub/app/file.txt
/pub/app:rw only covers /pub/app
```

If your app intends to grant access to everything under an app directory, use a trailing slash.

```rust
let caps = Capabilities::builder()
    .read_write("/pub/my-cool-app/")
    .finish();
```

```js
const caps = "/pub/my-cool-app/:rw";
```

## What Changed But Is Not Required For Cookie Auth

v0.10 introduces additional auth APIs and session views, but cookie-auth applications do not need to adopt them to keep working.

For a cookie-auth migration, avoid changing working code to:

- JS `startGrantAuthFlow(...)`.
- Rust `start_grant_auth_flow(...)`.
- Grant session management APIs.
- Grant secret export/import APIs.

Use those only when you intentionally migrate away from cookie auth.
