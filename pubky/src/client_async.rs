use std::thread;

use pkarr::Keypair;

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

        thread::spawn(move || {
            let result = client.signup(&keypair, &homeserver);
            sender.send(result)
        });

        receiver.recv_async().await?
    }
}
