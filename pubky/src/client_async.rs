use std::thread;

use bytes::Bytes;

use pkarr::{Keypair, PublicKey};
use pubky_common::session::Session;

use crate::{error::Result, PubkyClient};

pub struct PubkyClientAsync(PubkyClient);

impl PubkyClient {
    pub fn as_async(&self) -> PubkyClientAsync {
        PubkyClientAsync(self.clone())
    }
}

impl PubkyClientAsync {
    /// Async version of [PubkyClient::signup]
    pub async fn signup(&self, keypair: &Keypair, homeserver: &str) -> Result<()> {
        let (sender, receiver) = flume::bounded::<Result<()>>(1);

        let client = self.0.clone();
        let keypair = keypair.clone();
        let homeserver = homeserver.to_string();

        thread::spawn(move || sender.send(client.signup(&keypair, &homeserver)));

        receiver.recv_async().await?
    }

    /// Async version of [PubkyClient::session]
    pub async fn session(&self, pubky: &PublicKey) -> Result<Session> {
        let (sender, receiver) = flume::bounded::<Result<Session>>(1);

        let client = self.0.clone();
        let pubky = pubky.clone();

        thread::spawn(move || sender.send(client.session(&pubky)));

        receiver.recv_async().await?
    }

    /// Async version of [PubkyClient::signout]
    pub async fn signout(&self, pubky: &PublicKey) -> Result<()> {
        let (sender, receiver) = flume::bounded::<Result<()>>(1);

        let client = self.0.clone();
        let pubky = pubky.clone();

        thread::spawn(move || sender.send(client.signout(&pubky)));

        receiver.recv_async().await?
    }

    /// Async version of [PubkyClient::signin]
    pub async fn signin(&self, keypair: &Keypair) -> Result<()> {
        let (sender, receiver) = flume::bounded::<Result<()>>(1);

        let client = self.0.clone();
        let keypair = keypair.clone();

        thread::spawn(move || sender.send(client.signin(&keypair)));

        receiver.recv_async().await?
    }

    /// Async version of [PubkyClient::put]
    pub async fn put(&self, pubky: &PublicKey, path: &str, content: &[u8]) -> Result<()> {
        let (sender, receiver) = flume::bounded::<Result<()>>(1);

        let client = self.0.clone();
        let pubky = pubky.clone();
        let path = path.to_string();
        let content = content.to_vec();

        thread::spawn(move || sender.send(client.put(&pubky, &path, &content)));

        receiver.recv_async().await?
    }

    /// Async version of [PubkyClient::get]
    pub async fn get(&self, pubky: &PublicKey, path: &str) -> Result<Bytes> {
        let (sender, receiver) = flume::bounded::<Result<Bytes>>(1);

        let client = self.0.clone();
        let pubky = pubky.clone();
        let path = path.to_string();

        thread::spawn(move || sender.send(client.get(&pubky, &path)));

        receiver.recv_async().await?
    }
}
