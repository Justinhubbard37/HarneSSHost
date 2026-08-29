use crate::harness::adapter::{
    CandidateSource, DetectionContext, HarnessId, InstallationCandidate,
};

pub(crate) fn detection_context() -> DetectionContext {
    #[cfg(debug_assertions)]
    {
        use std::path::PathBuf;

        let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("upstream")
            .join("deepseek-harness");

        DetectionContext::new(vec![InstallationCandidate::new(
            HarnessId::new("deepseek"),
            CandidateSource::DevelopmentCheckout,
            checkout,
        )])
    }

    #[cfg(not(debug_assertions))]
    {
        DetectionContext::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_build_injects_only_the_known_development_candidate() {
        let context = detection_context();
        let candidates = context.candidates();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].adapter_id.as_str(), "deepseek");
        assert_eq!(candidates[0].source, CandidateSource::DevelopmentCheckout);
        assert!(candidates[0].path.ends_with("upstream/deepseek-harness"));
    }
}
