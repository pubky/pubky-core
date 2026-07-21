mod batch_republisher;
mod publisher;
mod republish_summary;
mod republisher;
mod retrying_republisher;

pub use batch_republisher::{BatchRepublisher, BatchRepublisherError, BatchRepublisherSettings};
pub use republish_summary::RepublishSummary;

#[cfg(test)]
fn test_client_builder(testnet: &pkarr::mainline::Testnet) -> pkarr::ClientBuilder {
    let mut builder = pkarr::ClientBuilder::default();
    builder
        .no_default_network()
        .bootstrap(&testnet.bootstrap)
        .dht_report_policy(pkarr::dht::ReportPolicy::testnet());
    builder
}
