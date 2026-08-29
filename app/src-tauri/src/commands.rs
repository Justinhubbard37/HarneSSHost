use crate::harness::adapter::{
    CompatibilityState, DetectionReport, HarnessAdapter, HarnessDescriptor, TestedVersion,
};
use crate::harness::capability::CapabilityManifest;
use crate::interface::{HostSurfaceState, InterfaceFacts, InterfaceResolver};
use crate::state::AppState;
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostSnapshotDto {
    product_name: String,
    interface_state: HostSurfaceState,
    harnesses: Vec<HarnessSnapshotDto>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum DetectionStatusDto {
    Detected,
    NotFound,
    Invalid,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct HarnessSnapshotDto {
    id: String,
    display_name: String,
    detection_status: DetectionStatusDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostic_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sanitized_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detected_version: Option<String>,
    tested_versions: Vec<TestedVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<CompatibilityState>,
    start_blocked: bool,
    capability_manifest: CapabilityManifest,
}

#[tauri::command]
pub(crate) fn get_host_snapshot(state: tauri::State<'_, AppState>) -> HostSnapshotDto {
    build_host_snapshot(state.inner())
}

fn build_host_snapshot(state: &AppState) -> HostSnapshotDto {
    let harnesses = state
        .registry
        .list()
        .into_iter()
        .filter_map(|descriptor| {
            state
                .registry
                .get(&descriptor.id)
                .map(|adapter| inspect_adapter(adapter.as_ref(), descriptor, state))
        })
        .collect();

    HostSnapshotDto {
        product_name: "HarneSSHost".to_string(),
        interface_state: state
            .interface_resolver
            .resolve(InterfaceFacts::NoHarnessActive),
        harnesses,
    }
}

fn inspect_adapter(
    adapter: &dyn HarnessAdapter,
    descriptor: HarnessDescriptor,
    state: &AppState,
) -> HarnessSnapshotDto {
    match adapter.detect(&state.detection_context) {
        Ok(DetectionReport::Detected(installation)) => match adapter.version(&installation) {
            Ok(version) => {
                let manifest = adapter.capability_manifest(Some(&version));
                HarnessSnapshotDto {
                    id: descriptor.id.as_str().to_string(),
                    display_name: descriptor.display_name,
                    detection_status: DetectionStatusDto::Detected,
                    diagnostic_code: None,
                    sanitized_message: Some(
                        "Source package metadata was detected successfully.".to_string(),
                    ),
                    detected_version: Some(version.detected_version.clone()),
                    tested_versions: version.tested_versions.clone(),
                    compatibility: Some(version.compatibility),
                    start_blocked: version.start_blocked,
                    capability_manifest: manifest,
                }
            }
            Err(error) => error_snapshot(
                adapter,
                descriptor,
                DetectionStatusDto::Error,
                error.code,
                error.message,
            ),
        },
        Ok(DetectionReport::NotFound { code, message }) => error_snapshot(
            adapter,
            descriptor,
            DetectionStatusDto::NotFound,
            code,
            message,
        ),
        Ok(DetectionReport::Invalid { code, message }) => error_snapshot(
            adapter,
            descriptor,
            DetectionStatusDto::Invalid,
            code,
            message,
        ),
        Ok(DetectionReport::Error { code, message }) => error_snapshot(
            adapter,
            descriptor,
            DetectionStatusDto::Error,
            code,
            message,
        ),
        Err(error) => error_snapshot(
            adapter,
            descriptor,
            DetectionStatusDto::Error,
            error.code,
            error.message,
        ),
    }
}

fn error_snapshot(
    adapter: &dyn HarnessAdapter,
    descriptor: HarnessDescriptor,
    status: DetectionStatusDto,
    code: &'static str,
    message: String,
) -> HarnessSnapshotDto {
    let manifest = adapter.capability_manifest(None);
    HarnessSnapshotDto {
        id: descriptor.id.as_str().to_string(),
        display_name: descriptor.display_name,
        detection_status: status,
        diagnostic_code: Some(code.to_string()),
        sanitized_message: Some(message),
        detected_version: None,
        tested_versions: vec![manifest.evidence_version.clone()],
        compatibility: None,
        start_blocked: true,
        capability_manifest: manifest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_is_sanitized_and_reports_the_development_baseline() {
        let state = AppState::new().unwrap();
        let snapshot = build_host_snapshot(&state);
        let json = serde_json::to_string(&snapshot).unwrap();

        assert_eq!(snapshot.product_name, "HarneSSHost");
        assert_eq!(snapshot.harnesses.len(), 1);
        assert_eq!(
            snapshot.harnesses[0].compatibility,
            Some(CompatibilityState::TestedVersionMatch)
        );
        assert!(json.contains("0.1.2-alpha.1"));
        assert!(json.contains("testedVersionMatch"));
        assert!(!json.contains("canonicalPath"));
        assert!(!json.contains("upstream"));
        assert!(!json.contains("C:\\\\"));
    }
}
