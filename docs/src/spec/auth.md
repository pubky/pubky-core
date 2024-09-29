# Pubky Auth

Pubky Auth is a protocol for using user's [root key](../concepts/rootkey.md),
to authenticate themselves to a 3rd party app, and or authorize that app to access
resources on the user's [Homeserver](../concepts/homeserver.md).

### glossary
1. **Authenticator**: an application holding the Keypair used in authentication.
2. **Pubky**: the public key (pubky) identifying the user.
3. **Homeserver**: the public key (pubky) identifying the receiver of the authentication request, usually a server.
4. **3rd Party App**: an application trying to get authorized to access some resources belonging to the **Pubky**.
5. **Capabilities**: a list of strings specifying scopes and the actions that can be performed on them.
6. **HTTP relay**: an independent HTTP relay (or the backend of the 3rd Party App) forwarding the AuthToken to the frontend.  

## Flow

```mermaid
sequenceDiagram
    participant User
    participant Authenticator
    participant 3rd Party App 
    participant HTTP Relay
    participant Homeserver

    autonumber
    
    3rd Party App -->>3rd Party App : Generate a unique secret
    3rd Party App ->>+HTTP Relay: Subscribe
    note over 3rd Party App ,HTTP Relay: channel Id = hash(client secret)
    3rd Party App ->>Authenticator: Show QR code
    note over 3rd Party App ,Authenticator: required Capabilities,<br/>relay url, and client secret
    Authenticator-->>User: Display consent form
    User -->>Authenticator: Confirm consent
    Authenticator-->>Authenticator: Sign AuthToken & encrypt with client secret
    Authenticator->>HTTP Relay: Send encrypted AuthToken
    note over Authenticator ,HTTP Relay: channel Id = hash(client secret)
    HTTP Relay->>3rd Party App : Forward Encrypted AuthToken
    HTTP Relay->>-Authenticator: Ok
    3rd Party App -->>3rd Party App : Decrypt AuthToken & Resolve user's homeserver
    3rd Party App ->>+Homeserver: Send AuthToken
    Homeserver-->>Homeserver: Verify AuthToken
    Homeserver->>-3rd Party App : Return SessionId
    3rd Party App ->>+Homeserver: Request resources
    Homeserver-->>Homeserver: Check Session capabilities
    Homeserver ->>-3rd Party App: Ok
```

1. `3rd Party App` generates a unique (32 bytes) `cleint_secret`.
2. `3rd Party App` uses the `base64url(hash(client_secret))` as a `channlen_id` and subscribe to that channel on the `HTTP Relay` it is using.
3. `3rd Party App` formats a Pubky Auth url 
```
pubkyauth:///
   ?relay=<HTTP Relay base (without channel_id)>
   &caps=<required capabilities>
   &secret=<base64url(client_secret)>
```
 for example 
 ```
pubkyauth:///
  ?relay=https://demo.httprelay.io/link
  &caps=/pub/pubky.app/:rw,/pub/example.com/nested:rw
  &secret=mAa8kGmlrynGzQLteDVW6-WeUGnfvHTpEmbNerbWfPI
 ```
 and finally show that URL as a QR code to the user.
4. The `Authenticator` app scans that QR code, parse the URL, and show a consent form for the user..
5. The user decides whether or not to grant these capabilities to the `3rd Party App`.
6. If the user approves, the `Authenticator` then uses their Keypair, to sign an [AuthToken](#authtoken), then encrypt that token with the `client_secret`, then calculate the `channel_id` by hashing that secret, and send that encrypted token to the callback url, which is the `relay` + `channel_id`.
7. `HTTP Relay` forwards the encrypted AuthToken to the `3rd Party App` frontend.
8. And confirms the delivery with the `Authenticator`
9. `3rd Party App` decrypts the AuthToken using its `client_secret`, read the `pubky` in it, and send it to their `homeserver` to obtain a session.
10. `Homeserver` verifies the session and stores the corresponding `capabilities`.
11. `Homeserver` returns a session Id to the frontend to use in subsequent requests.
12. `3rd Party App` uses the session Id to access some resource at the Homeserver.
13. `Homeserver` checks the session capabilities to see if it is allowed to access that resource.
14. `Homeserver` responds to the `3rd Party App` with the resource.

## AuthToken encoding
```abnf
```abnf
AuthToken   = signature namespace version timestamp pubky capabilities

signature      = 64OCTET ; ed25519 signature over the rest of the token.
namespace      = %x50.55.42.4b.59.3a.41.55.54.48 ; "PUBKY:AUTH" in UTF-8 (10 bytes)
version        = 1*OCTET ; Version of the AuthToken for future proofing.
timestamp      = 8OCTET ; Big-endian UNIX timestamp in microseconds
pubky          = 32OCTET ; ed25519 public key of the user
capabilities   = *(capability *( "," capability ))

capability     = scope ":" actions ; Formatted as `/path/to/resource:rw`

scope          = absolute-path ; Absolute path, see RFC 3986
absolute-path  = 1*( "/" segment )
segment        = <segment, see [URI], Section 3.3>

actions      = 1*action
action        = "r" / "w" ; Read or write (more actions can be specified later)
```

## AuthToken verification

To verify a token, the `homeserver` should:
1. Check the 75th byte (version) and make sure it is `0` for this spec.
2. Deserialize the token
4. Verify that the `timestamp` is within a window from the local time, the default should be 45 seconds in the past, and 45 seconds in the future to handle latency and drifts.
5. Verify that the `pubky` is the signer of the `signature` over the rest of the serialized token after the signature (`serialized_token[65..]`).
7. To avoid reuse of the token, the `homeserver` should consider the `timestamp` and `pubky`  (`serialized_token[75..115]`) as a unique sortable ID, and store it in a sortable key value store, rejecting any token that has that same ID, and removing all IDs that start with a timestamp that is older than the window mentioned in step 4.

## Unhosted
Callback URLs work fine for full-stack applications, but there are many cases where you would want to develop and application without a backend, yet you still would like to let users sign in and bring their own backend, that is not a new concept, in fact [unhosted](https://unhosted.org/) applications is one of the main reasons we are developing Homeservers.

These applications will still need some [relay](https://httprelay.io/) to receive the `AuthToken` through when the `Authenticator` sends it to the callback URL, but since the `AuthToken` is a bearer token, these relays can intercept it and use it before the unhosted app.

That is why we need to encrypt the `AuthToken` with a key that the relay doesn't have access to, and only shared between the app and the `Authenticator` 

## Limitations

### No delegation
In version zero, the `pubky` IS the `issuer`, meaning that the `AuthToken` is signed by the same key of the `pubky`. This is to simplify the spec, until we have a reason to keep the `issuer` keys even more secure than being in a mobile app used rarely to authenticate a browser session once in a while.

Having an `issuer` that isn't exactly the `pubky` means the `issuer` themselves need a certificate of delegation signed by the `pubky`. The problem with that, is that you can either lookup that certificate on the Homeserver (making the verification process async and possibly taking too long to timeout) or do what most TLS apps do right now, and send the certificates chain with the token, but then you have to deal with the eternal problem of revocation, which basically also forces you to go lookup somewhere making the the verification process async and possibly taking too long to timeout.

### Expiration is out of scope
While the token itself can only be used for very brief period, it is immediately exchanged for another authentication mechanism (usually a session ID) and deciding the expiration date of that authentication, if any, is out of the scope of this spec.

The assumption here is that we are authorizing a session to the Homeserver, so the user can always access all active sessions and revoke any session that they don't like, from the `Authenticator` app.

Other services are free to choose their authentication system once the homeserver verifies the pubky auth token, whether that is a JWT or a Session with or without expiration and are free to allow the user to manage sessions the way they see fit.
