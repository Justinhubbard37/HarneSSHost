use crate::harness::capability::CapabilityManifest;
use serde::Serialize;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub(crate) struct HarnessId(String);

impl HarnessId {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HarnessDescriptor {
    pub(crate) id: HarnessId,
    pub(crate) display_name: String,
    pub(crate) description: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CandidateSource {
    DevelopmentCheckout,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InstallationCandidate {
    pub(crate) adapter_id: HarnessId,
    pub(crate) source: CandidateSource,
    pub(crate) path: PathBuf,
}

impl InstallationCandidate {
    pub(crate) fn new(adapter_id: HarnessId, source: CandidateSource, path: PathBuf) -> Self {
        Self {
            adapter_id,
            source,
            path,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DetectionContext {
    candidates: Vec<InstallationCandidate>,
}

impl DetectionContext {
    pub(crate) fn new(candidates: Vec<InstallationCandidate>) -> Self {
        Self { candidates }
    }

    pub(crate) fn candidates_for<'a>(
        &'a self,
        adapter_id: &'a HarnessId,
    ) -> impl Iterator<Item = &'a InstallationCandidate> {
        self.candidates
            .iter()
            .filter(move |candidate| &candidate.adapter_id == adapter_id)
    }

    #[cfg(test)]
    pub(crate) fn candidates(&self) -> &[InstallationCandidate] {
        &self.candidates
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DetectedInstallation {
    adapter_id: HarnessId,
    source: CandidateSource,
    canonical_path: PathBuf,
}

impl DetectedInstallation {
    pub(crate) fn new(
        adapter_id: HarnessId,
        source: CandidateSource,
        canonical_path: PathBuf,
    ) -> Self {
        Self {
            adapter_id,
            source,
            canonical_path,
        }
    }

    pub(crate) fn adapter_id(&self) -> &HarnessId {
        &self.adapter_id
    }

    pub(crate) fn path(&self) -> &Path {
        &self.canonical_path
    }

    #[cfg(test)]
    pub(crate) fn source(&self) -> CandidateSource {
        self.source
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DetectionReport {
    Detected(DetectedInstallation),
    NotFound { code: &'static str, message: String },
    Invalid { code: &'static str, message: String },
    Error { code: &'static str, message: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TestedVersion {
    pub(crate) version: String,
    pub(crate) tag: String,
    pub(crate) source_commit: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CompatibilityState {
    TestedVersionMatch,
    UnverifiedVersion,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VersionReport {
    pub(crate) detected_version: String,
    pub(crate) tested_versions: Vec<TestedVersion>,
    pub(crate) compatibility: CompatibilityState,
    pub(crate) start_blocked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AdapterError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl AdapterError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl Display for AdapterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for AdapterError {}

pub(crate) trait HarnessAdapter: Send + Sync {
    fn descriptor(&self) -> &HarnessDescriptor;
    fn detect(&self, context: &DetectionContext) -> Result<DetectionReport, AdapterError>;
    fn version(&self, installation: &DetectedInstallation) -> Result<VersionReport, AdapterError>;
    fn capability_manifest(&self, version: Option<&VersionReport>) -> CapabilityManifest;
}
