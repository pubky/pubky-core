use crate::PublicKey;

/// The kind of authentication flow to perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthFlowKind {
    /// Sign in to an existing account.
    SignIn,
    /// Sign up for a new account.
    SignUp {
        /// The public key of the homeserver to sign up on.
        homeserver_public_key: Box<PublicKey>,
        /// The signup token to use for the signup flow.
        /// This is optional.
        signup_token: Option<String>,
    },
}

impl AuthFlowKind {
    /// Create a sign in flow.
    #[must_use]
    pub fn signin() -> Self {
        Self::SignIn
    }

    /// Create a sign up flow.
    /// # Arguments
    /// * `homeserver_public_key` - The public key of the homeserver to sign up on.
    /// * `signup_token` - The signup token to use for the signup flow. This is optional.
    #[must_use]
    pub fn signup(homeserver_public_key: PublicKey, signup_token: Option<String>) -> Self {
        Self::SignUp {
            homeserver_public_key: Box::new(homeserver_public_key),
            signup_token,
        }
    }
}
