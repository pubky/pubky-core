// use std::time::Duration;

// use pkarr::{
//     Keypair, PublicKey, SignedPacket, Timestamp,
//     dns::rdata::{RData, SVCB},
// };

// use super::core::PubkyAgent;
// use crate::{
//     agent::state::{Keyed, sealed::Sealed},
//     errors::{Error, PkarrError, Result},
// };

// #[derive(Debug, Clone, Copy)]
// pub(crate) enum PublishStrategy {
//     Force,
//     IfOlderThan,
// }

// /// Agent-scoped PKDNS (Pkarr) view.
// /// - On `Keyless`: read helpers (resolve current homeserver).
// /// - On `Keyed`:  read + publish helpers (sign and republish `_pubky`).
// #[derive(Debug, Clone, Copy)]
// pub struct Pkdns<'a, S: Sealed>(&'a PubkyAgent<S>);

// impl<S: Sealed> PubkyAgent<S> {
//     pub fn pkdns(&self) -> Pkdns<'_, S> {
//         Pkdns(self)
//     }
// }

// // -------------------- Shared (Keyed & Keyless) --------------------

// impl<'a, S: Sealed> Pkdns<'a, S> {
//     /// Resolve current homeserver host for any pubky via Pkarr.
//     pub async fn get_homeserver(&self, pubky: &PublicKey) -> Option<String> {
//         let packet = self.0.client.pkarr().resolve_most_recent(pubky).await?;
//         extract_host_from_packet(&packet)
//     }
// }

// // -------------------- Keyed-only (publishing) --------------------

// impl<'a> Pkdns<'a, Keyed> {
//     /// Publish `_pubky` record **forcing** a refresh.
//     /// If `host_override` is None, we reuse the host found in the existing record (if any).
//     pub async fn publish_homeserver_force(&self, host_override: Option<&PublicKey>) -> Result<()> {
//         self.publish_homeserver(host_override, PublishStrategy::Force)
//             .await
//     }

//     /// Publish `_pubky` record **only if stale/missing**.
//     /// If `host_override` is None, we reuse the host found in the existing record (if any).
//     pub async fn publish_homeserver_if_stale(
//         &self,
//         host_override: Option<&PublicKey>,
//     ) -> Result<()> {
//         self.publish_homeserver(host_override, PublishStrategy::IfOlderThan)
//             .await
//     }

//     // /// Convenience: resolve the current host for this agent’s pubky and republish if stale.
//     // pub async fn republish_self(&self) -> Result<()> {
//     //     let pk = self.0.require_pubky()?;
//     //     let Some(host) = self.get_homeserver(&pk).await else {
//     //         return Ok(());
//     //     };
//     //     let host_pk = PublicKey::try_from(host.as_str())?;
//     //     self.publish_homeserver_if_stale(Some(&host_pk)).await
//     // }

//     // ---- internals ----
//     async fn publish_homeserver(
//         &self,
//         host_override: Option<&PublicKey>,
//         mode: PublishStrategy,
//     ) -> Result<()> {
//         let kp: &Keypair = self.0.keypair.get();
//         let pubky = kp.public_key();

//         // 1) Resolve the most recent record once.
//         let existing = self.0.client.pkarr().resolve_most_recent(&pubky).await;

//         // 2) Decide host string to publish.
//         let host_str = match determine_host(host_override, existing.as_ref()) {
//             Some(h) => h,
//             None => return Ok(()), // nothing to do
//         };

//         // 3) Age check (for IfOlderThan).
//         if matches!(mode, PublishStrategy::IfOlderThan) {
//             if let Some(ref record) = existing {
//                 let elapsed = Timestamp::now() - record.timestamp();
//                 let age = Duration::from_micros(elapsed.as_u64());
//                 if age <= self.0.client.max_record_age() {
//                     return Ok(());
//                 }
//             }
//         }

//         // 4) Publish with small retry loop on retryable pkarr errors.
//         for attempt in 1..=3 {
//             match self
//                 .publish_homeserver_inner(kp, &host_str, existing.clone())
//                 .await
//             {
//                 Ok(()) => return Ok(()),
//                 Err(e) => {
//                     if let Error::Pkarr(pk) = &e {
//                         if pk.is_retryable() && attempt < 3 {
//                             continue;
//                         }
//                     }
//                     return Err(e);
//                 }
//             }
//         }

//         Ok(())
//     }

//     async fn publish_homeserver_inner(
//         &self,
//         keypair: &Keypair,
//         host: &str,
//         existing: Option<SignedPacket>,
//     ) -> Result<()> {
//         // Keep previous records that are *not* `_pubky.*`, then write `_pubky` HTTPS/SVCB.
//         let mut builder = SignedPacket::builder();
//         if let Some(ref packet) = existing {
//             for answer in packet.resource_records("_pubky") {
//                 if !answer.name.to_string().starts_with("_pubky") {
//                     builder = builder.record(answer.to_owned());
//                 }
//             }
//         }

//         let svcb = SVCB::new(0, host.try_into().map_err(PkarrError::from)?);
//         let signed_packet = builder
//             .https("_pubky".try_into().unwrap(), svcb, 60 * 60)
//             .sign(keypair)
//             .map_err(PkarrError::from)?;

//         self.0
//             .client
//             .pkarr()
//             .publish(&signed_packet, existing.map(|s| s.timestamp()))
//             .await
//             .map_err(PkarrError::from)?;

//         Ok(())
//     }
// }

// /// Extract `_pubky` SVCB/HTTPS target from a signed Pkarr packet.
// fn extract_host_from_packet(packet: &SignedPacket) -> Option<String> {
//     packet
//         .resource_records("_pubky")
//         .find_map(|rr| match &rr.rdata {
//             RData::SVCB(svcb) => Some(svcb.target.to_string()),
//             RData::HTTPS(https) => Some(https.0.target.to_string()),
//             _ => None,
//         })
// }

// fn determine_host(
//     override_host: Option<&PublicKey>,
//     dht_packet: Option<&SignedPacket>,
// ) -> Option<String> {
//     if let Some(host) = override_host {
//         return Some(host.to_string());
//     }
//     dht_packet.and_then(extract_host_from_packet)
// }
