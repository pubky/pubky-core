use pkarr::{dns::rdata::SVCB, Keypair, SignedPacket};

use anyhow::Result;

use crate::Client;

impl Client {
    /// Publish the HTTPS record for `_pubky.<public_key>`.
    pub(crate) async fn publish_homeserver(&self, keypair: &Keypair, host: &str) -> Result<()> {
        // TODO: Before making public, consider the effect on other records and other mirrors

        let existing = self.pkarr.resolve_most_recent(&keypair.public_key()).await;

        let mut signed_packet_builder = SignedPacket::builder();

        if let Some(ref existing) = existing {
            for answer in existing.resource_records("_pubky") {
                if !answer.name.to_string().starts_with("_pubky") {
                    signed_packet_builder = signed_packet_builder.record(answer.to_owned());
                }
            }
        }

        let svcb = SVCB::new(0, host.try_into()?);

        let signed_packet = SignedPacket::builder()
            .https("_pubky".try_into().unwrap(), svcb, 60 * 60)
            .sign(keypair)?;

        self.pkarr
            .publish(&signed_packet, existing.map(|s| s.timestamp()))
            .await?;

        Ok(())
    }

    // pub(crate) resolve_icann_domain() {
    //
    //         let original_url = url.as_str();
    //         let mut url = Url::parse(original_url).expect("Invalid url in inner_request");
    //
    //         if url.scheme() == "pubky" {
    //             // TODO: use https for anything other than testnet
    //             url.set_scheme("http")
    //                 .expect("couldn't replace pubky:// with http://");
    //             url.set_host(Some(&format!("_pubky.{}", url.host_str().unwrap_or(""))))
    //                 .expect("couldn't map pubk://<pubky> to https://_pubky.<pubky>");
    //         }
    //
    //         let qname = url.host_str().unwrap_or("").to_string();
    //
    //         if PublicKey::try_from(original_url).is_ok() {
    //             let mut stream = self.pkarr.resolve_https_endpoints(&qname);
    //
    //             let mut so_far: Option<Endpoint> = None;
    //
    //             while let Some(endpoint) = stream.next().await {
    //                 if let Some(ref e) = so_far {
    //                     if e.domain() == "." && endpoint.domain() != "." {
    //                         so_far = Some(endpoint);
    //                     }
    //                 } else {
    //                     so_far = Some(endpoint)
    //                 }
    //             }
    //
    //             if let Some(e) = so_far {
    //                 url.set_host(Some(e.domain()))
    //                     .expect("coultdn't use the resolved endpoint's domain");
    //                 url.set_port(Some(e.port()))
    //                     .expect("coultdn't use the resolved endpoint's port");
    //
    //                 return self.http.request(method, url).fetch_credentials_include();
    //             } else {
    //                 // TODO: didn't find any domain, what to do?
    //             }
    //         }
    //
    //         self.http.request(method, url).fetch_credentials_include()
    // }
}
