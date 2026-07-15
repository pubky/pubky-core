use super::{republisher::RepublishError, retrying_republisher::RepublishInfo};

pub(super) type RepublishResult = Result<RepublishInfo, RepublishError>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepublishSummary {
    total_count: usize,
    success_count: usize,
    publishing_failed_count: usize,
    missing_count: usize,
}

impl RepublishSummary {
    pub(super) fn record(&mut self, result: RepublishResult) {
        self.total_count += 1;

        match result {
            Ok(_) => self.success_count += 1,
            Err(RepublishError::PublishFailed(_)) => self.publishing_failed_count += 1,
            Err(RepublishError::Missing) => self.missing_count += 1,
        }
    }

    pub(crate) fn merge(mut self, other: Self) -> Self {
        self.total_count += other.total_count;
        self.success_count += other.success_count;
        self.publishing_failed_count += other.publishing_failed_count;
        self.missing_count += other.missing_count;
        self
    }

    /// Number of republish attempts.
    pub fn len(&self) -> usize {
        self.total_count
    }

    pub fn is_empty(&self) -> bool {
        self.total_count == 0
    }

    /// Number of successfully published keys.
    pub fn success_count(&self) -> usize {
        self.success_count
    }

    /// Number of keys that failed to publish.
    pub fn publishing_failed_count(&self) -> usize {
        self.publishing_failed_count
    }

    /// Number of keys that are missing and could not be republished.
    pub fn missing_count(&self) -> usize {
        self.missing_count
    }
}

#[cfg(test)]
mod tests {
    use super::{RepublishError, RepublishInfo, RepublishSummary};
    use crate::republishers::pkarr_republisher::publisher::PublishError;

    #[test]
    fn records_and_merges_republish_outcomes() {
        let mut summary = RepublishSummary::default();
        summary.record(Ok(RepublishInfo::new(3, 1)));

        let mut other = RepublishSummary::default();
        other.record(Err(RepublishError::Missing));
        other.record(Err(RepublishError::PublishFailed(
            PublishError::InsufficientlyPublished {
                published_nodes_count: 1,
            },
        )));

        let summary = summary.merge(other);

        assert_eq!(summary.len(), 3);
        assert_eq!(summary.success_count(), 1);
        assert_eq!(summary.missing_count(), 1);
        assert_eq!(summary.publishing_failed_count(), 1);
    }
}
