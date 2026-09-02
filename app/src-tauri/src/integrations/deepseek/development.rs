#[cfg(test)]
use crate::harness::adapter::DetectionContext;
use crate::harness::adapter::{CandidateSource, HarnessId, InstallationCandidate};
use crate::integrations::deepseek::adapter::DEEPSEEK_ADAPTER_ID;

pub(crate) fn candidates() -> Vec<InstallationCandidate> {
    let mut candidates = Vec::new();

    #[cfg(debug_assertions)]
    {
        use std::path::PathBuf;

        let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("upstream")
            .join("deepseek-harness");

        candidates.push(InstallationCandidate::new(
            HarnessId::new(DEEPSEEK_ADAPTER_ID),
            CandidateSource::DevelopmentCheckout,
            checkout,
        ));
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_build_injects_only_the_known_sibling_development_candidate() {
        let context = DetectionContext::new(candidates());
        let candidates = context.candidates();
        let expected = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("upstream")
            .join("deepseek-harness");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].adapter_id.as_str(), DEEPSEEK_ADAPTER_ID);
        assert_eq!(candidates[0].source, CandidateSource::DevelopmentCheckout);
        assert_eq!(candidates[0].path, expected);
    }
}
