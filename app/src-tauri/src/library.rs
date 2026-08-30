use crate::harness::adapter::{
    CandidatePathClass, CompatibilityState, DetectedInstallation,
    DetectedInstallationClassification, DetectionReport, HarnessAdapter, HarnessId,
    InstallationProvenance, OfficialInterfaceFact, RejectedDetectionClassification,
    SupportedTopologyFact, VersionReport,
};
use crate::harness::capability::CapabilityManifest;
use crate::harness::deepseek::DEEPSEEK_ADAPTER_ID;
use crate::interface::HostSurfaceState;
use crate::runtime::domain::RuntimePhase;
use crate::state::AppState;
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HarnessLibraryDto {
    harnesses: Vec<HarnessCardDto>,
    surface: HostSurfaceState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct HarnessCardDto {
    id: String,
    display_name: String,
    description: String,
    state: HarnessCardStateDto,
    can_open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_message: Option<String>,
    has_details: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum HarnessCardStateDto {
    NotInstalled,
    Detected,
    SupportedNonBaseline,
    Ready,
    Starting,
    Open,
    Stopping,
    Failed,
    ProblemDetected,
    UnsupportedLocalInstallation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HarnessDetailsDto {
    id: String,
    display_name: String,
    description: String,
    official_interfaces: Vec<OfficialInterfaceFact>,
    supported_topologies: Vec<SupportedTopologyFact>,
    detection: DetectionDetailsDto,
    runtime: RuntimeDetailsDto,
    capability_manifest: CapabilityManifest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DetectionDetailsDto {
    status: DetectionStatusDto,
    classification: DetectionClassificationDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detected_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<CompatibilityState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance: Option<ProvenanceDetailsDto>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum DetectionStatusDto {
    Detected,
    NotInstalled,
    Invalid,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum DetectionClassificationDto {
    NotInstalled,
    ValidInstallation,
    ValidWslNative,
    NativeWindowsSupported,
    WrongVersion,
    WrongArchitecture,
    WindowsPathLeakage,
    AmbiguousOrUntrustedProvenance,
    Invalid,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvenanceDetailsDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    wsl_distribution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linux_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    architecture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filesystem: Option<String>,
    path_class: CandidatePathClass,
    installation_provenance: InstallationProvenance,
    track_a_baseline: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeDetailsDto {
    available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase: Option<RuntimePhase>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_code: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalInstallationState {
    NotInstalled,
    Ready(DetectedInstallationClassification),
    Problem,
    Unsupported,
}

pub(crate) fn build_harness_library(state: &AppState) -> HarnessLibraryDto {
    let harnesses = state
        .registry
        .list()
        .into_iter()
        .filter_map(|descriptor| {
            state.registry.get(&descriptor.id).map(|adapter| {
                let installation_state = inspect_card_state(adapter.as_ref(), state);
                let runtime_snapshot = state.runtime_snapshot(&descriptor.id);
                let card_state = match runtime_snapshot {
                    Some(snapshot) => derive_card_state(installation_state, snapshot.phase()),
                    None => derive_catalog_only_card_state(installation_state),
                };
                let can_open = matches!(installation_state, LocalInstallationState::Ready(_))
                    && runtime_snapshot.is_some_and(|snapshot| snapshot.can_open());
                let failure_message = (card_state == HarnessCardStateDto::Failed).then(|| {
                    if can_open {
                        "DeepSeek could not be opened. Use Open to try again."
                    } else {
                        "DeepSeek could not be opened."
                    }
                    .to_string()
                });

                HarnessCardDto {
                    id: descriptor.id.as_str().to_string(),
                    display_name: descriptor.display_name,
                    description: descriptor.description,
                    state: card_state,
                    can_open,
                    failure_message,
                    has_details: true,
                }
            })
        })
        .collect();

    let deepseek_snapshot = state
        .runtime_snapshot(&HarnessId::new(DEEPSEEK_ADAPTER_ID))
        .expect("the registered DeepSeek runtime snapshot must exist");
    let surface = state.runtime_controller.resolve_surface(deepseek_snapshot);

    HarnessLibraryDto { harnesses, surface }
}

pub(crate) fn build_harness_details(
    state: &AppState,
    harness_id: &str,
) -> Result<HarnessDetailsDto, LibraryError> {
    let id = HarnessId::new(harness_id);
    let adapter = state.registry.get(&id).ok_or(LibraryError::NotSupported)?;
    let runtime_snapshot = state.runtime_snapshot(&id);
    let descriptor = adapter.descriptor().clone();
    let (detection, version) = inspect_details(adapter.as_ref(), state);
    let capability_manifest = adapter.capability_manifest(version.as_ref());

    Ok(HarnessDetailsDto {
        id: descriptor.id.as_str().to_string(),
        display_name: descriptor.display_name,
        description: descriptor.description,
        official_interfaces: descriptor.official_interfaces,
        supported_topologies: descriptor.supported_topologies,
        detection,
        runtime: RuntimeDetailsDto {
            available: runtime_snapshot.is_some(),
            phase: runtime_snapshot.map(|snapshot| snapshot.phase()),
            failure_code: runtime_snapshot
                .and_then(|snapshot| snapshot.failure_code())
                .map(str::to_string),
        },
        capability_manifest,
    })
}

fn inspect_card_state(adapter: &dyn HarnessAdapter, state: &AppState) -> LocalInstallationState {
    match adapter.detect(&state.detection_context) {
        Ok(DetectionReport::Detected(installation)) => match adapter.version(&installation) {
            Ok(version) if version.compatibility == CompatibilityState::TestedVersionMatch => {
                LocalInstallationState::Ready(installation.classification())
            }
            Ok(_) => LocalInstallationState::Unsupported,
            Err(_) => LocalInstallationState::Problem,
        },
        Ok(DetectionReport::NotFound { .. }) => LocalInstallationState::NotInstalled,
        Ok(
            DetectionReport::Invalid { .. }
            | DetectionReport::Rejected { .. }
            | DetectionReport::Error { .. },
        )
        | Err(_) => LocalInstallationState::Problem,
    }
}

fn inspect_details(
    adapter: &dyn HarnessAdapter,
    state: &AppState,
) -> (DetectionDetailsDto, Option<VersionReport>) {
    match adapter.detect(&state.detection_context) {
        Ok(DetectionReport::Detected(installation)) => match adapter.version(&installation) {
            Ok(version) => {
                let compatibility = version.compatibility;
                let classification = if compatibility == CompatibilityState::UnverifiedVersion {
                    DetectionClassificationDto::WrongVersion
                } else {
                    detected_classification(installation.classification())
                };
                let message = detected_message(installation.classification(), compatibility);
                let provenance = provenance_details(&installation);
                (
                    DetectionDetailsDto {
                        status: DetectionStatusDto::Detected,
                        classification,
                        code: (compatibility == CompatibilityState::UnverifiedVersion)
                            .then(|| "harness.wrong-version".to_string()),
                        message: Some(message),
                        detected_version: Some(version.detected_version.clone()),
                        compatibility: Some(compatibility),
                        provenance,
                    },
                    Some(version),
                )
            }
            Err(error)
                if installation.classification()
                    != DetectedInstallationClassification::SourceCheckout =>
            {
                (
                    DetectionDetailsDto {
                        status: DetectionStatusDto::Detected,
                        classification: detected_classification(installation.classification()),
                        code: Some(error.code.to_string()),
                        message: Some(error.message),
                        detected_version: None,
                        compatibility: None,
                        provenance: provenance_details(&installation),
                    },
                    None,
                )
            }
            Err(error) => (
                detection_problem(
                    DetectionStatusDto::Error,
                    DetectionClassificationDto::Error,
                    error.code,
                    error.message,
                ),
                None,
            ),
        },
        Ok(DetectionReport::NotFound { code, message }) => (
            detection_problem(
                DetectionStatusDto::NotInstalled,
                DetectionClassificationDto::NotInstalled,
                code,
                message,
            ),
            None,
        ),
        Ok(DetectionReport::Invalid { code, message }) => (
            detection_problem(
                DetectionStatusDto::Invalid,
                DetectionClassificationDto::Invalid,
                code,
                message,
            ),
            None,
        ),
        Ok(DetectionReport::Rejected {
            classification,
            code,
            message,
        }) => (
            detection_problem(
                DetectionStatusDto::Invalid,
                rejected_classification(classification),
                code,
                message,
            ),
            None,
        ),
        Ok(DetectionReport::Error { code, message }) => (
            detection_problem(
                DetectionStatusDto::Error,
                DetectionClassificationDto::Error,
                code,
                message,
            ),
            None,
        ),
        Err(error) => (
            detection_problem(
                DetectionStatusDto::Error,
                DetectionClassificationDto::Error,
                error.code,
                error.message,
            ),
            None,
        ),
    }
}

fn detection_problem(
    status: DetectionStatusDto,
    classification: DetectionClassificationDto,
    code: &'static str,
    message: String,
) -> DetectionDetailsDto {
    DetectionDetailsDto {
        status,
        classification,
        code: Some(code.to_string()),
        message: Some(message),
        detected_version: None,
        compatibility: None,
        provenance: None,
    }
}

fn detected_classification(
    classification: DetectedInstallationClassification,
) -> DetectionClassificationDto {
    match classification {
        DetectedInstallationClassification::SourceCheckout => {
            DetectionClassificationDto::ValidInstallation
        }
        DetectedInstallationClassification::ValidWslNative => {
            DetectionClassificationDto::ValidWslNative
        }
        DetectedInstallationClassification::NativeWindowsSupported => {
            DetectionClassificationDto::NativeWindowsSupported
        }
    }
}

fn rejected_classification(
    classification: RejectedDetectionClassification,
) -> DetectionClassificationDto {
    match classification {
        RejectedDetectionClassification::WrongArchitecture => {
            DetectionClassificationDto::WrongArchitecture
        }
        RejectedDetectionClassification::WindowsPathLeakage => {
            DetectionClassificationDto::WindowsPathLeakage
        }
        RejectedDetectionClassification::AmbiguousOrUntrustedProvenance => {
            DetectionClassificationDto::AmbiguousOrUntrustedProvenance
        }
    }
}

fn detected_message(
    classification: DetectedInstallationClassification,
    compatibility: CompatibilityState,
) -> String {
    if compatibility == CompatibilityState::UnverifiedVersion {
        return "The detected harness version does not match the tested baseline.".to_string();
    }
    match classification {
        DetectedInstallationClassification::SourceCheckout => {
            "Source package metadata was detected successfully.".to_string()
        }
        DetectedInstallationClassification::ValidWslNative => {
            "A trusted WSL-native OpenCode installation matches the Track A baseline."
                .to_string()
        }
        DetectedInstallationClassification::NativeWindowsSupported => {
            "A native Windows OpenCode installation was detected; it is supported but is not the Track A baseline."
                .to_string()
        }
    }
}

fn provenance_details(installation: &DetectedInstallation) -> Option<ProvenanceDetailsDto> {
    let evidence = installation.executable_evidence()?;
    Some(ProvenanceDetailsDto {
        wsl_distribution: evidence.wsl_distribution.clone(),
        linux_user: evidence.linux_user.clone(),
        architecture: evidence.architecture.clone(),
        filesystem: evidence.filesystem.clone(),
        path_class: evidence.path_class,
        installation_provenance: evidence.provenance,
        track_a_baseline: installation.classification()
            == DetectedInstallationClassification::ValidWslNative,
    })
}

fn derive_card_state(
    installation: LocalInstallationState,
    runtime: RuntimePhase,
) -> HarnessCardStateDto {
    match installation {
        LocalInstallationState::NotInstalled => HarnessCardStateDto::NotInstalled,
        LocalInstallationState::Unsupported => HarnessCardStateDto::UnsupportedLocalInstallation,
        LocalInstallationState::Problem => HarnessCardStateDto::ProblemDetected,
        LocalInstallationState::Ready(_) if runtime == RuntimePhase::Failed => {
            HarnessCardStateDto::Failed
        }
        LocalInstallationState::Ready(_) => match runtime {
            RuntimePhase::Inactive => HarnessCardStateDto::Ready,
            RuntimePhase::Starting => HarnessCardStateDto::Starting,
            RuntimePhase::Ready => HarnessCardStateDto::Open,
            RuntimePhase::Stopping => HarnessCardStateDto::Stopping,
            RuntimePhase::Failed => HarnessCardStateDto::Failed,
        },
    }
}

fn derive_catalog_only_card_state(installation: LocalInstallationState) -> HarnessCardStateDto {
    match installation {
        LocalInstallationState::NotInstalled => HarnessCardStateDto::NotInstalled,
        LocalInstallationState::Unsupported => HarnessCardStateDto::UnsupportedLocalInstallation,
        LocalInstallationState::Problem => HarnessCardStateDto::ProblemDetected,
        LocalInstallationState::Ready(
            DetectedInstallationClassification::NativeWindowsSupported,
        ) => HarnessCardStateDto::SupportedNonBaseline,
        LocalInstallationState::Ready(_) => HarnessCardStateDto::Detected,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LibraryError {
    NotSupported,
}

impl LibraryError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::NotSupported => "harness.not-supported",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::adapter::DetectionContext;

    #[test]
    fn deepseek_remains_in_the_supported_catalog() {
        let state = AppState::new().unwrap();
        let library = build_harness_library(&state);

        assert_eq!(library.harnesses.len(), 2);
        assert_eq!(library.harnesses[0].id, "deepseek");
        assert_eq!(library.harnesses[0].display_name, "DeepSeek Harness");
        assert_eq!(library.harnesses[1].id, "opencode");
        assert_eq!(library.harnesses[1].display_name, "OpenCode");
    }

    #[test]
    fn supported_harness_remains_visible_when_not_locally_detected() {
        let state = AppState::with_detection_context(DetectionContext::default()).unwrap();
        let library = build_harness_library(&state);

        assert_eq!(library.harnesses.len(), 2);
        assert!(library
            .harnesses
            .iter()
            .all(|harness| harness.state == HarnessCardStateDto::NotInstalled));
    }

    #[test]
    fn catalog_detection_and_runtime_state_remain_independent() {
        let state = AppState::with_detection_context(DetectionContext::default()).unwrap();
        state
            .runtime_controller
            .set_snapshot_for_test(RuntimePhase::Starting, false, None);

        let library = build_harness_library(&state);

        assert_eq!(state.registry.list().len(), 2);
        assert_eq!(state.detection_context.candidates().len(), 0);
        assert_eq!(
            state
                .runtime_snapshot(&HarnessId::new("deepseek"))
                .map(|snapshot| snapshot.phase()),
            Some(RuntimePhase::Starting)
        );
        assert_eq!(
            library.harnesses[0].state,
            HarnessCardStateDto::NotInstalled
        );
    }

    #[test]
    fn derives_each_simple_card_state_without_frontend_inference() {
        assert_eq!(
            derive_card_state(LocalInstallationState::NotInstalled, RuntimePhase::Ready),
            HarnessCardStateDto::NotInstalled
        );
        assert_eq!(
            derive_card_state(
                LocalInstallationState::Ready(DetectedInstallationClassification::SourceCheckout,),
                RuntimePhase::Inactive,
            ),
            HarnessCardStateDto::Ready
        );
        assert_eq!(
            derive_card_state(
                LocalInstallationState::Ready(DetectedInstallationClassification::SourceCheckout,),
                RuntimePhase::Failed,
            ),
            HarnessCardStateDto::Failed
        );
        assert_eq!(
            derive_card_state(
                LocalInstallationState::Ready(DetectedInstallationClassification::SourceCheckout,),
                RuntimePhase::Starting,
            ),
            HarnessCardStateDto::Starting
        );
        assert_eq!(
            derive_card_state(
                LocalInstallationState::Ready(DetectedInstallationClassification::SourceCheckout,),
                RuntimePhase::Ready,
            ),
            HarnessCardStateDto::Open
        );
        assert_eq!(
            derive_card_state(
                LocalInstallationState::Ready(DetectedInstallationClassification::SourceCheckout,),
                RuntimePhase::Stopping,
            ),
            HarnessCardStateDto::Stopping
        );
        assert_eq!(
            derive_card_state(LocalInstallationState::Problem, RuntimePhase::Inactive),
            HarnessCardStateDto::ProblemDetected
        );
        assert_eq!(
            derive_card_state(LocalInstallationState::Unsupported, RuntimePhase::Inactive),
            HarnessCardStateDto::UnsupportedLocalInstallation
        );
    }

    #[test]
    fn phase_4c_verified_deepseek_open_action_tracks_live_runtime_and_cleanup_state() {
        let state = AppState::new().unwrap();

        let initial = build_harness_library(&state);
        assert_eq!(initial.harnesses[0].state, HarnessCardStateDto::Ready);
        assert!(initial.harnesses[0].can_open);

        for (phase, cleanup_complete, expected_state, expected_open) in [
            (
                RuntimePhase::Starting,
                false,
                HarnessCardStateDto::Starting,
                false,
            ),
            (RuntimePhase::Ready, false, HarnessCardStateDto::Open, true),
            (
                RuntimePhase::Stopping,
                false,
                HarnessCardStateDto::Stopping,
                false,
            ),
            (
                RuntimePhase::Failed,
                false,
                HarnessCardStateDto::Failed,
                false,
            ),
            (
                RuntimePhase::Failed,
                true,
                HarnessCardStateDto::Failed,
                true,
            ),
        ] {
            state.runtime_controller.set_snapshot_for_test(
                phase,
                cleanup_complete,
                Some("deepseek.test-failure"),
            );
            let library = build_harness_library(&state);
            assert_eq!(library.harnesses[0].state, expected_state);
            assert_eq!(library.harnesses[0].can_open, expected_open);
        }
    }

    #[test]
    fn default_library_payload_excludes_technical_and_sensitive_fields() {
        let state = AppState::new().unwrap();
        let json = serde_json::to_string(&build_harness_library(&state)).unwrap();

        assert!(json.contains("DeepSeek Harness"));
        for forbidden in [
            "sourceCommit",
            "evidenceVersion",
            "capabilityManifest",
            "detectedVersion",
            "compatibility",
            "canonicalPath",
            "processCommandLine",
            "environment",
            "authenticatedUrl",
            "token",
            "pid",
            "diagnostic",
        ] {
            assert!(!json.contains(forbidden), "unexpected field: {forbidden}");
        }
    }

    #[test]
    fn details_are_separate_sanitized_and_exclude_ownership() {
        let state = AppState::new().unwrap();
        let details = build_harness_details(&state, "deepseek").unwrap();
        let json = serde_json::to_string(&details).unwrap();

        assert!(json.contains("detectedVersion"));
        assert!(json.contains("capabilityManifest"));
        assert!(json.contains("sourceCommit"));
        for forbidden in [
            "canonicalPath",
            "C:\\\\",
            "upstream\\\\deepseek-harness",
            "processCommandLine",
            "environment",
            "authenticatedUrl",
            "\"token\":",
            "?token=",
            "abcdefghijklmnopqrstuvwxyzABCDEFGH123456789",
            "pid",
            "ownership",
            "generation",
        ] {
            assert!(!json.contains(forbidden), "unexpected field: {forbidden}");
        }

        let snapshot = state
            .runtime_snapshot(&HarnessId::new("deepseek"))
            .expect("DeepSeek runtime state must exist");
        assert_eq!(snapshot.phase(), RuntimePhase::Inactive);
    }

    #[test]
    fn opencode_details_do_not_assume_runtime_availability() {
        let state = AppState::with_detection_context(DetectionContext::default()).unwrap();
        let details = build_harness_details(&state, "opencode").unwrap();
        let json = serde_json::to_value(details).unwrap();

        assert_eq!(json["id"], "opencode");
        assert_eq!(json["detection"]["classification"], "notInstalled");
        assert_eq!(json["runtime"]["available"], false);
        assert!(json["runtime"].get("phase").is_none());
        assert_eq!(json["officialInterfaces"].as_array().unwrap().len(), 4);
        assert_eq!(json["supportedTopologies"].as_array().unwrap().len(), 2);
        assert!(state
            .runtime_snapshot(&HarnessId::new("opencode"))
            .is_none());
    }

    #[test]
    fn rejects_unknown_detail_requests_with_a_safe_code() {
        let state = AppState::new().unwrap();
        let error = build_harness_details(&state, "unknown").unwrap_err();

        assert_eq!(error.code(), "harness.not-supported");
    }
}
