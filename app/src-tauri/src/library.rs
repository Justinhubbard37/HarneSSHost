use crate::harness::adapter::{
    CompatibilityState, DetectionReport, HarnessAdapter, HarnessId, VersionReport,
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
    detection: DetectionDetailsDto,
    runtime: RuntimeDetailsDto,
    capability_manifest: CapabilityManifest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DetectionDetailsDto {
    status: DetectionStatusDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detected_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<CompatibilityState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum DetectionStatusDto {
    Detected,
    NotInstalled,
    Invalid,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeDetailsDto {
    phase: RuntimePhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_code: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalInstallationState {
    NotInstalled,
    Ready,
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
                    None => HarnessCardStateDto::ProblemDetected,
                };
                let can_open = installation_state == LocalInstallationState::Ready
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
    let runtime_snapshot = state
        .runtime_snapshot(&id)
        .ok_or(LibraryError::RuntimeStateUnavailable)?;
    let descriptor = adapter.descriptor().clone();
    let (detection, version) = inspect_details(adapter.as_ref(), state);
    let capability_manifest = adapter.capability_manifest(version.as_ref());

    Ok(HarnessDetailsDto {
        id: descriptor.id.as_str().to_string(),
        display_name: descriptor.display_name,
        description: descriptor.description,
        detection,
        runtime: RuntimeDetailsDto {
            phase: runtime_snapshot.phase(),
            failure_code: runtime_snapshot.failure_code().map(str::to_string),
        },
        capability_manifest,
    })
}

fn inspect_card_state(adapter: &dyn HarnessAdapter, state: &AppState) -> LocalInstallationState {
    match adapter.detect(&state.detection_context) {
        Ok(DetectionReport::Detected(installation)) => match adapter.version(&installation) {
            Ok(version) if version.compatibility == CompatibilityState::TestedVersionMatch => {
                LocalInstallationState::Ready
            }
            Ok(_) => LocalInstallationState::Unsupported,
            Err(_) => LocalInstallationState::Problem,
        },
        Ok(DetectionReport::NotFound { .. }) => LocalInstallationState::NotInstalled,
        Ok(DetectionReport::Invalid { .. } | DetectionReport::Error { .. }) | Err(_) => {
            LocalInstallationState::Problem
        }
    }
}

fn inspect_details(
    adapter: &dyn HarnessAdapter,
    state: &AppState,
) -> (DetectionDetailsDto, Option<VersionReport>) {
    match adapter.detect(&state.detection_context) {
        Ok(DetectionReport::Detected(installation)) => match adapter.version(&installation) {
            Ok(version) => (
                DetectionDetailsDto {
                    status: DetectionStatusDto::Detected,
                    code: None,
                    message: Some("Source package metadata was detected successfully.".to_string()),
                    detected_version: Some(version.detected_version.clone()),
                    compatibility: Some(version.compatibility),
                },
                Some(version),
            ),
            Err(error) => (
                detection_problem(DetectionStatusDto::Error, error.code, error.message),
                None,
            ),
        },
        Ok(DetectionReport::NotFound { code, message }) => (
            detection_problem(DetectionStatusDto::NotInstalled, code, message),
            None,
        ),
        Ok(DetectionReport::Invalid { code, message }) => (
            detection_problem(DetectionStatusDto::Invalid, code, message),
            None,
        ),
        Ok(DetectionReport::Error { code, message }) => (
            detection_problem(DetectionStatusDto::Error, code, message),
            None,
        ),
        Err(error) => (
            detection_problem(DetectionStatusDto::Error, error.code, error.message),
            None,
        ),
    }
}

fn detection_problem(
    status: DetectionStatusDto,
    code: &'static str,
    message: String,
) -> DetectionDetailsDto {
    DetectionDetailsDto {
        status,
        code: Some(code.to_string()),
        message: Some(message),
        detected_version: None,
        compatibility: None,
    }
}

fn derive_card_state(
    installation: LocalInstallationState,
    runtime: RuntimePhase,
) -> HarnessCardStateDto {
    match installation {
        LocalInstallationState::NotInstalled => HarnessCardStateDto::NotInstalled,
        LocalInstallationState::Unsupported => HarnessCardStateDto::UnsupportedLocalInstallation,
        LocalInstallationState::Problem => HarnessCardStateDto::ProblemDetected,
        LocalInstallationState::Ready if runtime == RuntimePhase::Failed => {
            HarnessCardStateDto::Failed
        }
        LocalInstallationState::Ready => match runtime {
            RuntimePhase::Inactive => HarnessCardStateDto::Ready,
            RuntimePhase::Starting => HarnessCardStateDto::Starting,
            RuntimePhase::Ready => HarnessCardStateDto::Open,
            RuntimePhase::Stopping => HarnessCardStateDto::Stopping,
            RuntimePhase::Failed => HarnessCardStateDto::Failed,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LibraryError {
    NotSupported,
    RuntimeStateUnavailable,
}

impl LibraryError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::NotSupported => "harness.not-supported",
            Self::RuntimeStateUnavailable => "harness.runtime-state-unavailable",
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

        assert_eq!(library.harnesses.len(), 1);
        assert_eq!(library.harnesses[0].id, "deepseek");
        assert_eq!(library.harnesses[0].display_name, "DeepSeek Harness");
    }

    #[test]
    fn supported_harness_remains_visible_when_not_locally_detected() {
        let state = AppState::with_detection_context(DetectionContext::default()).unwrap();
        let library = build_harness_library(&state);

        assert_eq!(library.harnesses.len(), 1);
        assert_eq!(
            library.harnesses[0].state,
            HarnessCardStateDto::NotInstalled
        );
    }

    #[test]
    fn catalog_detection_and_runtime_state_remain_independent() {
        let state = AppState::with_detection_context(DetectionContext::default()).unwrap();
        state
            .runtime_controller
            .set_snapshot_for_test(RuntimePhase::Starting, false, None);

        let library = build_harness_library(&state);

        assert_eq!(state.registry.list().len(), 1);
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
            derive_card_state(LocalInstallationState::Ready, RuntimePhase::Inactive),
            HarnessCardStateDto::Ready
        );
        assert_eq!(
            derive_card_state(LocalInstallationState::Ready, RuntimePhase::Failed),
            HarnessCardStateDto::Failed
        );
        assert_eq!(
            derive_card_state(LocalInstallationState::Ready, RuntimePhase::Starting),
            HarnessCardStateDto::Starting
        );
        assert_eq!(
            derive_card_state(LocalInstallationState::Ready, RuntimePhase::Ready),
            HarnessCardStateDto::Open
        );
        assert_eq!(
            derive_card_state(LocalInstallationState::Ready, RuntimePhase::Stopping),
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
    fn rejects_unknown_detail_requests_with_a_safe_code() {
        let state = AppState::new().unwrap();
        let error = build_harness_details(&state, "unknown").unwrap_err();

        assert_eq!(error.code(), "harness.not-supported");
    }
}
