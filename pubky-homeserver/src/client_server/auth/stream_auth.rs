//! Authentication state and revocation handling for private event streams.

use std::future;

use tokio::sync::broadcast;

use crate::shared::{HttpError, HttpResult};

use super::{
    revocation::AuthRevocationUnavailable, AuthRevocation, AuthRevocationService, AuthSession,
    AuthState,
};

/// Subscription state acquired before resolving middleware authentication.
/// Subscribing first ensures a revocation cannot be missed in that gap.
pub(crate) enum PendingStreamAuth {
    Public,
    Private(broadcast::Receiver<AuthRevocation>),
}

impl PendingStreamAuth {
    /// Take a revocation receiver for a private stream. A public stream is not
    /// tied to a credential, so it never waits on the listener.
    pub(crate) async fn subscribe(
        is_private: bool,
        service: &AuthRevocationService,
    ) -> Result<Self, AuthRevocationUnavailable> {
        if is_private {
            Ok(Self::Private(service.subscribe().await?))
        } else {
            Ok(Self::Public)
        }
    }

    pub(crate) async fn authorize(
        self,
        session: Option<AuthSession>,
        auth_state: &AuthState,
    ) -> HttpResult<StreamAuth> {
        let mut stream_auth = match (self, session) {
            (Self::Private(revocation_rx), Some(session)) => {
                auth_state.validate_private_stream_session(&session).await?;
                StreamAuth::Private {
                    session: Box::new(session),
                    revocation_rx,
                }
            }
            (Self::Private(_), None) => return Err(HttpError::unauthorized()),
            (Self::Public, _) => StreamAuth::Public,
        };

        if stream_auth.is_valid(auth_state).await {
            Ok(stream_auth)
        } else {
            Err(HttpError::unauthorized())
        }
    }
}

/// Authentication state held for the lifetime of an event stream.
///
/// Public streams do not wait for auth revocations. Private streams hold the
/// credential that authorized them and their local revocation receiver.
pub(crate) enum StreamAuth {
    Public,
    Private {
        session: Box<AuthSession>,
        revocation_rx: broadcast::Receiver<AuthRevocation>,
    },
}

enum RevocationCheck {
    Clear,
    Close,
    Revalidate,
}

impl StreamAuth {
    fn pending_revocation(&mut self) -> RevocationCheck {
        let Self::Private {
            session,
            revocation_rx,
        } = self
        else {
            return RevocationCheck::Clear;
        };

        loop {
            match revocation_rx.try_recv() {
                Ok(revocation) if revocation.matches(session) => return RevocationCheck::Close,
                Ok(_) => continue,
                Err(broadcast::error::TryRecvError::Empty) => return RevocationCheck::Clear,
                // Revalidate this stream's own credential instead of
                // disconnecting every private stream after an unrelated burst.
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    return RevocationCheck::Revalidate
                }
                Err(broadcast::error::TryRecvError::Closed) => return RevocationCheck::Close,
            }
        }
    }

    /// Drain buffered revocations before another event is emitted. A lagged
    /// receiver only closes this stream when its own credential is invalid.
    pub(crate) async fn is_valid(&mut self, auth_state: &AuthState) -> bool {
        loop {
            match self.pending_revocation() {
                RevocationCheck::Clear => return true,
                RevocationCheck::Close => return false,
                RevocationCheck::Revalidate => {
                    let Self::Private { session, .. } = self else {
                        unreachable!("public streams cannot require revalidation");
                    };

                    if let Err(error) = auth_state.validate_private_stream_session(session).await {
                        tracing::warn!(
                            ?error,
                            "auth revocation receiver lagged and private stream validation failed"
                        );
                        return false;
                    }
                }
            }
        }
    }

    /// Wait for the next auth-revocation signal and return whether the stream
    /// must close. Public streams wait forever because they are unaffected.
    pub(crate) async fn handle_next_revocation(&mut self, auth_state: &AuthState) -> bool {
        let received = match self {
            Self::Private { revocation_rx, .. } => revocation_rx.recv().await,
            Self::Public => future::pending().await,
        };

        match received {
            Ok(revocation) if self.matches(&revocation) => {
                tracing::debug!("closing private event stream after auth revocation");
                true
            }
            Ok(_) => false,
            Err(broadcast::error::RecvError::Lagged(_)) => {
                if self.is_valid(auth_state).await {
                    false
                } else {
                    tracing::warn!("auth revocation receiver lagged and stream session is no longer valid; closing private event stream");
                    true
                }
            }
            Err(broadcast::error::RecvError::Closed) => {
                tracing::warn!("auth revocation receiver closed; closing private event stream");
                true
            }
        }
    }

    fn matches(&self, revocation: &AuthRevocation) -> bool {
        match self {
            Self::Private { session, .. } => revocation.matches(session),
            Self::Public => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_server::auth::cookie::persistence::{SessionEntity, SessionSecret};
    use pubky_common::{capabilities::Capabilities, crypto::Keypair};

    fn cookie_session() -> AuthSession {
        AuthSession::Cookie(SessionEntity {
            id: 1,
            secret: SessionSecret::random(),
            user_id: 1,
            user_pubkey: Keypair::random().public_key(),
            capabilities: Capabilities::default(),
            created_at: chrono::Utc::now().naive_utc(),
        })
    }

    #[test]
    fn buffered_revocation_closes_a_private_stream_before_historical_replay() {
        let (sender, receiver) = broadcast::channel(1);
        sender.send(AuthRevocation::CookieSession(1)).unwrap();
        let mut stream_auth = StreamAuth::Private {
            session: Box::new(cookie_session()),
            revocation_rx: receiver,
        };

        assert!(matches!(
            stream_auth.pending_revocation(),
            RevocationCheck::Close
        ));
    }

    #[test]
    fn lagged_revocation_receiver_requests_revalidation() {
        let (sender, receiver) = broadcast::channel(1);
        sender.send(AuthRevocation::CookieSession(2)).unwrap();
        sender.send(AuthRevocation::CookieSession(3)).unwrap();
        let mut stream_auth = StreamAuth::Private {
            session: Box::new(cookie_session()),
            revocation_rx: receiver,
        };

        assert!(matches!(
            stream_auth.pending_revocation(),
            RevocationCheck::Revalidate
        ));
    }
}
