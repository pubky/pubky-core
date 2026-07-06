use anyhow::Result;
use pubky::{Keypair, PubkySigner, PublicKey};
use std::path::{Path, PathBuf};

pub const SAMPLE_RECOVERY_FILE: &str = "sample_recovery.key";

pub fn sample_recovery_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SAMPLE_RECOVERY_FILE)
}

pub fn decrypt_recovery_file(path: &Path, prompt: &str) -> Result<Keypair> {
    let recovery_file = std::fs::read(path)?;

    if let Ok(keypair) = pubky::recovery_file::decrypt_recovery_file(&recovery_file, "") {
        return Ok(keypair);
    }

    println!("{prompt}");
    let passphrase = rpassword::read_password()?;
    Ok(pubky::recovery_file::decrypt_recovery_file(
        &recovery_file,
        &passphrase,
    )?)
}

pub async fn ensure_testnet_signup(signer: &PubkySigner, homeserver: &PublicKey) -> Result<()> {
    match signer.signup(homeserver, None).await {
        Ok(()) => println!("Signed up to the testnet homeserver."),
        Err(pubky::Error::Request(pubky::errors::RequestError::Server { status, .. }))
            if status == reqwest::StatusCode::CONFLICT =>
        {
            println!("Testnet user already exists, continuing...");
            signer
                .pkdns()
                .publish_homeserver_force(Some(homeserver))
                .await?;
            println!("Published testnet homeserver record.");
        }
        Err(err) => return Err(err.into()),
    }

    Ok(())
}
