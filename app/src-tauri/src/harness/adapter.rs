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
    pub(crate) official_interfaces: Vec<OfficialInterfaceFact>,
    pub(crate) supported_topologies: Vec<SupportedTopologyFact>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum OfficialInterfaceKind {
    TerminalUi,
    Web,
    DesktopApp,
    IdeExtension,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OfficialInterfaceFact {
    pub(crate) kind: OfficialInterfaceKind,
    pub(crate) source_reference: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExecutionTopology {
    WslNative,
    NativeWindows,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SupportedTopologyFact {
    pub(crate) topology: ExecutionTopology,
    pub(crate) supported: bool,
    pub(crate) track_a_baseline: bool,
    pub(crate) source_reference: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CandidateSource {
    DevelopmentCheckout,
    WslDistribution,
    NativeWindows,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CandidatePathClass {
    LinuxNative,
    WindowsMounted,
    WindowsNative,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutableFormat {
    Elf64,
    WindowsPe,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(test), allow(dead_code))] // Foreign evidence is injected by later acquisition/provenance gates.
pub(crate) enum InstallationProvenance {
    HarnessHostOwned,
    NativeWindows,
    Foreign,
    Ambiguous,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutableCandidateEvidence {
    pub(crate) wsl_distribution: Option<String>,
    pub(crate) linux_user: Option<String>,
    pub(crate) executable_path: String,
    pub(crate) canonical_path: Option<String>,
    pub(crate) architecture: Option<String>,
    pub(crate) filesystem: Option<String>,
    pub(crate) path_class: CandidatePathClass,
    pub(crate) format: ExecutableFormat,
    pub(crate) regular_file: bool,
    pub(crate) symlink: bool,
    pub(crate) owner: Option<String>,
    pub(crate) group_or_world_writable: Option<bool>,
    pub(crate) reported_version: Option<String>,
    pub(crate) provenance: InstallationProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InstallationCandidate {
    pub(crate) adapter_id: HarnessId,
    pub(crate) source: CandidateSource,
    pub(crate) path: PathBuf,
    executable_evidence: Option<ExecutableCandidateEvidence>,
}

impl InstallationCandidate {
    pub(crate) fn new(adapter_id: HarnessId, source: CandidateSource, path: PathBuf) -> Self {
        Self {
            adapter_id,
            source,
            path,
            executable_evidence: None,
        }
    }

    pub(crate) fn executable(
        adapter_id: HarnessId,
        source: CandidateSource,
        evidence: ExecutableCandidateEvidence,
    ) -> Self {
        Self {
            adapter_id,
            source,
            path: PathBuf::from(&evidence.executable_path),
            executable_evidence: Some(evidence),
        }
    }

    pub(crate) fn executable_evidence(&self) -> Option<&ExecutableCandidateEvidence> {
        self.executable_evidence.as_ref()
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
    classification: DetectedInstallationClassification,
    executable_evidence: Option<Box<ExecutableCandidateEvidence>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DetectedInstallationClassification {
    SourceCheckout,
    ValidWslNative,
    NativeWindowsSupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RejectedDetectionClassification {
    WrongArchitecture,
    WindowsPathLeakage,
    AmbiguousOrUntrustedProvenance,
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
            classification: DetectedInstallationClassification::SourceCheckout,
            executable_evidence: None,
        }
    }

    pub(crate) fn executable(
        adapter_id: HarnessId,
        source: CandidateSource,
        classification: DetectedInstallationClassification,
        evidence: ExecutableCandidateEvidence,
    ) -> Self {
        Self {
            adapter_id,
            source,
            canonical_path: PathBuf::from(
                evidence
                    .canonical_path
                    .as_deref()
                    .unwrap_or(&evidence.executable_path),
            ),
            classification,
            executable_evidence: Some(Box::new(evidence)),
        }
    }

    pub(crate) fn adapter_id(&self) -> &HarnessId {
        &self.adapter_id
    }

    pub(crate) fn path(&self) -> &Path {
        &self.canonical_path
    }

    pub(crate) fn classification(&self) -> DetectedInstallationClassification {
        self.classification
    }

    pub(crate) fn executable_evidence(&self) -> Option<&ExecutableCandidateEvidence> {
        self.executable_evidence.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn source(&self) -> CandidateSource {
        self.source
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DetectionReport {
    Detected(DetectedInstallation),
    NotFound {
        code: &'static str,
        message: String,
    },
    Invalid {
        code: &'static str,
        message: String,
    },
    Rejected {
        classification: RejectedDetectionClassification,
        code: &'static str,
        message: String,
    },
    Error {
        code: &'static str,
        message: String,
    },
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
