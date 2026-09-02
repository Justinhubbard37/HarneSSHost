use crate::harness::adapter::{
    AdapterError, CandidatePathClass, CandidateSource, CompatibilityState, DetectedInstallation,
    DetectedInstallationClassification, DetectionContext, DetectionReport,
    ExecutableCandidateEvidence, ExecutableFormat, ExecutionTopology, HarnessAdapter,
    HarnessDescriptor, HarnessId, InstallationCandidate, InstallationProvenance,
    OfficialInterfaceFact, OfficialInterfaceKind, RejectedDetectionClassification,
    SupportedTopologyFact, TestedVersion, VersionReport,
};
use crate::harness::capability::{
    CapabilityDomain, CapabilityEntry, CapabilityEvidence, CapabilityManifest, CapabilityStatus,
    EvidenceProvenance,
};
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::process::Command;

pub(crate) const OPENCODE_ADAPTER_ID: &str = "opencode";
pub(crate) const TESTED_VERSION: &str = "1.18.25";
pub(crate) const TESTED_TAG: &str = "v1.18.25";
pub(crate) const TESTED_SOURCE_COMMIT: &str = "cb7d8b2f5e44876ef98b661dc10590c915af3a9f";
pub(crate) const SELECTED_WSL_DISTRIBUTION: &str = "Ubuntu";
pub(crate) const SELECTED_LINUX_USER: &str = "heathen";
pub(crate) const OWNED_EXECUTABLE_PATH: &str = "/home/heathen/.opencode/bin/opencode";

const OPENCODE_DOCS: &str = "https://opencode.ai/docs/";
const OPENCODE_TUI_DOCS: &str = "https://opencode.ai/docs/tui/";
const OPENCODE_WEB_DOCS: &str = "https://opencode.ai/docs/web/";
const OPENCODE_WINDOWS_WSL_DOCS: &str = "https://opencode.ai/docs/windows-wsl/";

pub(crate) struct OpenCodeAdapter {
    descriptor: HarnessDescriptor,
}

impl OpenCodeAdapter {
    pub(crate) fn new() -> Self {
        Self {
            descriptor: HarnessDescriptor {
                id: HarnessId::new(OPENCODE_ADAPTER_ID),
                display_name: "OpenCode".to_string(),
                description:
                    "An open-source coding agent with official terminal, web, desktop, and IDE interfaces."
                        .to_string(),
                official_interfaces: vec![
                    OfficialInterfaceFact {
                        kind: OfficialInterfaceKind::TerminalUi,
                        source_reference: OPENCODE_TUI_DOCS.to_string(),
                    },
                    OfficialInterfaceFact {
                        kind: OfficialInterfaceKind::Web,
                        source_reference: OPENCODE_WEB_DOCS.to_string(),
                    },
                    OfficialInterfaceFact {
                        kind: OfficialInterfaceKind::DesktopApp,
                        source_reference: OPENCODE_DOCS.to_string(),
                    },
                    OfficialInterfaceFact {
                        kind: OfficialInterfaceKind::IdeExtension,
                        source_reference: OPENCODE_DOCS.to_string(),
                    },
                ],
                supported_topologies: vec![
                    SupportedTopologyFact {
                        topology: ExecutionTopology::WslNative,
                        supported: true,
                        track_a_baseline: true,
                        source_reference: OPENCODE_WINDOWS_WSL_DOCS.to_string(),
                    },
                    SupportedTopologyFact {
                        topology: ExecutionTopology::NativeWindows,
                        supported: true,
                        track_a_baseline: false,
                        source_reference: OPENCODE_WINDOWS_WSL_DOCS.to_string(),
                    },
                ],
            },
        }
    }

    fn tested_version() -> TestedVersion {
        TestedVersion {
            version: TESTED_VERSION.to_string(),
            tag: TESTED_TAG.to_string(),
            source_commit: TESTED_SOURCE_COMMIT.to_string(),
        }
    }

    fn reject(
        classification: RejectedDetectionClassification,
        code: &'static str,
        message: &'static str,
    ) -> DetectionReport {
        DetectionReport::Rejected {
            classification,
            code,
            message: message.to_string(),
        }
    }

    fn inspect_wsl_candidate(
        &self,
        candidate: &InstallationCandidate,
    ) -> Result<DetectedInstallation, DetectionReport> {
        let Some(evidence) = candidate.executable_evidence() else {
            return Err(Self::reject(
                RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                "opencode.provenance-missing",
                "OpenCode candidate provenance could not be established.",
            ));
        };

        if evidence.path_class == CandidatePathClass::WindowsMounted
            || evidence.executable_path.starts_with("/mnt/")
            || evidence
                .canonical_path
                .as_deref()
                .is_some_and(|path| path.starts_with("/mnt/"))
        {
            return Err(Self::reject(
                RejectedDetectionClassification::WindowsPathLeakage,
                "opencode.windows-path-in-wsl",
                "A Windows-mounted executable cannot satisfy WSL-native OpenCode provenance.",
            ));
        }

        if evidence.wsl_distribution.as_deref() != Some(SELECTED_WSL_DISTRIBUTION)
            || evidence.linux_user.as_deref() != Some(SELECTED_LINUX_USER)
            || evidence.executable_path != OWNED_EXECUTABLE_PATH
            || evidence.canonical_path.as_deref() != Some(OWNED_EXECUTABLE_PATH)
            || evidence.path_class != CandidatePathClass::LinuxNative
        {
            return Err(Self::reject(
                RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                "opencode.wsl-provenance-mismatch",
                "The OpenCode candidate does not match the selected WSL distribution, user, and owned path.",
            ));
        }

        let Some(architecture) = evidence.architecture.as_deref() else {
            return Err(Self::reject(
                RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                "opencode.architecture-unverified",
                "The OpenCode candidate architecture could not be established.",
            ));
        };
        if !matches!(architecture, "x86_64" | "amd64") {
            return Err(Self::reject(
                RejectedDetectionClassification::WrongArchitecture,
                "opencode.wrong-architecture",
                "The OpenCode candidate does not match the approved Linux x86-64 architecture.",
            ));
        }
        match evidence.format {
            ExecutableFormat::Elf64 => {}
            ExecutableFormat::WindowsPe => {
                return Err(Self::reject(
                    RejectedDetectionClassification::WrongArchitecture,
                    "opencode.wrong-binary-format",
                    "The OpenCode candidate is not the approved Linux ELF executable.",
                ));
            }
            ExecutableFormat::Unknown => {
                return Err(Self::reject(
                    RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                    "opencode.binary-format-unverified",
                    "The OpenCode candidate binary format could not be established.",
                ));
            }
        }

        if evidence.filesystem.as_deref() != Some("ext4")
            || !evidence.regular_file
            || evidence.symlink
            || evidence.owner.as_deref() != Some(SELECTED_LINUX_USER)
            || evidence.group_or_world_writable != Some(false)
            || evidence.provenance != InstallationProvenance::HarnessHostOwned
            || evidence.reported_version.is_none()
        {
            return Err(Self::reject(
                RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                "opencode.provenance-untrusted",
                "The WSL-native OpenCode candidate did not provide complete trusted provenance evidence.",
            ));
        }

        Ok(DetectedInstallation::executable(
            self.descriptor.id.clone(),
            candidate.source,
            DetectedInstallationClassification::ValidWslNative,
            evidence.clone(),
        ))
    }

    fn inspect_windows_candidate(
        &self,
        candidate: &InstallationCandidate,
    ) -> Result<DetectedInstallation, DetectionReport> {
        let Some(evidence) = candidate.executable_evidence() else {
            return Err(Self::reject(
                RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                "opencode.windows-provenance-missing",
                "Native Windows OpenCode provenance could not be established.",
            ));
        };

        if evidence.path_class != CandidatePathClass::WindowsNative
            || evidence.provenance != InstallationProvenance::NativeWindows
            || evidence.format != ExecutableFormat::WindowsPe
            || evidence.canonical_path.is_none()
            || !evidence.regular_file
            || evidence.symlink
        {
            return Err(Self::reject(
                RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                "opencode.windows-provenance-untrusted",
                "The native Windows OpenCode candidate has ambiguous or untrusted provenance.",
            ));
        }

        Ok(DetectedInstallation::executable(
            self.descriptor.id.clone(),
            candidate.source,
            DetectedInstallationClassification::NativeWindowsSupported,
            evidence.clone(),
        ))
    }
}

impl HarnessAdapter for OpenCodeAdapter {
    fn descriptor(&self) -> &HarnessDescriptor {
        &self.descriptor
    }

    fn detect(&self, context: &DetectionContext) -> Result<DetectionReport, AdapterError> {
        let mut first_rejection = None;
        let mut windows_installations = Vec::new();

        for candidate in context.candidates_for(&self.descriptor.id) {
            match candidate.source {
                CandidateSource::WslDistribution => match self.inspect_wsl_candidate(candidate) {
                    Ok(installation) => return Ok(DetectionReport::Detected(installation)),
                    Err(rejection) if first_rejection.is_none() => {
                        first_rejection = Some(rejection)
                    }
                    Err(_) => {}
                },
                CandidateSource::NativeWindows => match self.inspect_windows_candidate(candidate) {
                    Ok(installation) => windows_installations.push(installation),
                    Err(rejection) if first_rejection.is_none() => {
                        first_rejection = Some(rejection)
                    }
                    Err(_) => {}
                },
                CandidateSource::DevelopmentCheckout => {
                    if first_rejection.is_none() {
                        first_rejection = Some(Self::reject(
                            RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                            "opencode.unsupported-candidate-source",
                            "The OpenCode candidate source is not trusted for this adapter.",
                        ));
                    }
                }
            }
        }

        if let Some(rejection) = first_rejection {
            return Ok(rejection);
        }
        match windows_installations.len() {
            0 => Ok(DetectionReport::NotFound {
                code: "opencode.not-installed",
                message: "OpenCode is not installed in the selected Ubuntu distribution or on native Windows."
                    .to_string(),
            }),
            1 => Ok(DetectionReport::Detected(
                windows_installations.pop().expect("length was checked"),
            )),
            _ => Ok(Self::reject(
                RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                "opencode.multiple-windows-candidates",
                "Multiple native Windows OpenCode candidates were found and no candidate was selected.",
            )),
        }
    }

    fn version(&self, installation: &DetectedInstallation) -> Result<VersionReport, AdapterError> {
        if installation.adapter_id() != &self.descriptor.id {
            return Err(AdapterError::new(
                "opencode.installation-mismatch",
                "The detected installation belongs to a different harness adapter.",
            ));
        }
        let detected_version = installation
            .executable_evidence()
            .and_then(|evidence| evidence.reported_version.as_deref())
            .ok_or_else(|| {
                AdapterError::new(
                    "opencode.version-unverified",
                    "The OpenCode version could not be established without trusted evidence.",
                )
            })?;
        let compatibility = if detected_version == TESTED_VERSION {
            CompatibilityState::TestedVersionMatch
        } else {
            CompatibilityState::UnverifiedVersion
        };

        Ok(VersionReport {
            detected_version: detected_version.to_string(),
            tested_versions: vec![Self::tested_version()],
            compatibility,
            start_blocked: compatibility == CompatibilityState::UnverifiedVersion,
        })
    }

    fn capability_manifest(&self, version: Option<&VersionReport>) -> CapabilityManifest {
        CapabilityManifest {
            adapter_id: self.descriptor.id.clone(),
            adapter_name: self.descriptor.display_name.clone(),
            detected_version: version.map(|report| report.detected_version.clone()),
            evidence_version: Self::tested_version(),
            compatibility: version.map(|report| report.compatibility),
            entries: vec![
                supported_interface("terminal-ui", OPENCODE_TUI_DOCS),
                supported_interface("web", OPENCODE_WEB_DOCS),
                supported_interface("desktop-app", OPENCODE_DOCS),
                supported_interface("ide-extension", OPENCODE_DOCS),
            ],
        }
    }
}

fn supported_interface(capability: &str, reference: &str) -> CapabilityEntry {
    CapabilityEntry {
        domain: CapabilityDomain::Interaction,
        capability: capability.to_string(),
        status: CapabilityStatus::Supported,
        limitations: None,
        notes: None,
        evidence: vec![CapabilityEvidence {
            provenance: EvidenceProvenance::Source,
            reference: reference.to_string(),
            summary: "OpenCode documents this as an official interface.".to_string(),
        }],
    }
}

pub(crate) fn machine_candidates() -> Vec<InstallationCandidate> {
    #[cfg(windows)]
    {
        let mut candidates = Vec::new();
        if let Some(candidate) = probe_selected_wsl_candidate() {
            candidates.push(candidate);
        }
        candidates.extend(probe_native_windows_candidates());
        candidates
    }

    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
const WSL_PROBE_SCRIPT: &str = r#"target='/home/heathen/.opencode/bin/opencode'
if [ ! -e "$target" ] && [ ! -L "$target" ]; then
  /usr/bin/printf 'status=not-found\n'
  exit 0
fi
/usr/bin/printf 'status=found\n'
/usr/bin/printf 'canonical=%s\n' "$(/usr/bin/readlink -f -- "$target" 2>/dev/null)"
/usr/bin/printf 'architecture=%s\n' "$(/usr/bin/uname -m 2>/dev/null)"
/usr/bin/printf 'filesystem=%s\n' "$(/usr/bin/findmnt -T "$target" -n -o FSTYPE 2>/dev/null)"
if [ -f "$target" ]; then regular=true; else regular=false; fi
if [ -L "$target" ]; then symlink=true; else symlink=false; fi
/usr/bin/printf 'regular=%s\n' "$regular"
/usr/bin/printf 'symlink=%s\n' "$symlink"
/usr/bin/printf 'owner=%s\n' "$(/usr/bin/stat -c %U -- "$target" 2>/dev/null)"
/usr/bin/printf 'mode=%s\n' "$(/usr/bin/stat -c %a -- "$target" 2>/dev/null)"
if [ -x /usr/bin/file ]; then
  /usr/bin/printf 'format=%s\n' "$(/usr/bin/file -b -- "$target" 2>/dev/null)"
else
  /usr/bin/printf 'format=unknown\n'
fi
"#;

#[cfg(windows)]
fn selected_wsl_probe_command() -> Command {
    let mut command = Command::new("wsl.exe");
    command.args([
        "--distribution",
        SELECTED_WSL_DISTRIBUTION,
        "--user",
        SELECTED_LINUX_USER,
        "--",
        "/bin/sh",
        "-c",
        WSL_PROBE_SCRIPT,
    ]);
    command
}

#[cfg(windows)]
fn probe_selected_wsl_candidate() -> Option<InstallationCandidate> {
    let output = match selected_wsl_probe_command().output() {
        Ok(output) if output.status.success() => output,
        _ => return Some(ambiguous_wsl_candidate()),
    };
    let stdout = match String::from_utf8(output.stdout) {
        Ok(stdout) => stdout,
        Err(_) => return Some(ambiguous_wsl_candidate()),
    };
    let fields = parse_probe_fields(&stdout);
    if fields.get("status").map(String::as_str) == Some("not-found") {
        return None;
    }
    if fields.get("status").map(String::as_str) != Some("found") {
        return Some(ambiguous_wsl_candidate());
    }

    let canonical_path = fields
        .get("canonical")
        .cloned()
        .filter(|value| !value.is_empty());
    let filesystem = fields
        .get("filesystem")
        .cloned()
        .filter(|value| !value.is_empty());
    let path_class = if canonical_path
        .as_deref()
        .is_some_and(|path| path.starts_with("/mnt/"))
    {
        CandidatePathClass::WindowsMounted
    } else if filesystem.as_deref() == Some("ext4") {
        CandidatePathClass::LinuxNative
    } else {
        CandidatePathClass::Unknown
    };
    let format = match fields.get("format").map(String::as_str) {
        Some(value) if value.contains("ELF 64-bit") => ExecutableFormat::Elf64,
        Some(value) if value.contains("PE32") => ExecutableFormat::WindowsPe,
        _ => ExecutableFormat::Unknown,
    };
    let group_or_world_writable = fields
        .get("mode")
        .and_then(|mode| group_or_world_writable(mode));

    Some(InstallationCandidate::executable(
        HarnessId::new(OPENCODE_ADAPTER_ID),
        CandidateSource::WslDistribution,
        ExecutableCandidateEvidence {
            wsl_distribution: Some(SELECTED_WSL_DISTRIBUTION.to_string()),
            linux_user: Some(SELECTED_LINUX_USER.to_string()),
            executable_path: OWNED_EXECUTABLE_PATH.to_string(),
            canonical_path,
            architecture: fields.get("architecture").cloned(),
            filesystem,
            path_class,
            format,
            regular_file: fields.get("regular").map(String::as_str) == Some("true"),
            symlink: fields.get("symlink").map(String::as_str) == Some("true"),
            owner: fields
                .get("owner")
                .cloned()
                .filter(|value| !value.is_empty()),
            group_or_world_writable,
            reported_version: None,
            provenance: InstallationProvenance::Ambiguous,
        },
    ))
}

#[cfg(windows)]
fn ambiguous_wsl_candidate() -> InstallationCandidate {
    InstallationCandidate::executable(
        HarnessId::new(OPENCODE_ADAPTER_ID),
        CandidateSource::WslDistribution,
        ExecutableCandidateEvidence {
            wsl_distribution: Some(SELECTED_WSL_DISTRIBUTION.to_string()),
            linux_user: Some(SELECTED_LINUX_USER.to_string()),
            executable_path: OWNED_EXECUTABLE_PATH.to_string(),
            canonical_path: None,
            architecture: None,
            filesystem: None,
            path_class: CandidatePathClass::Unknown,
            format: ExecutableFormat::Unknown,
            regular_file: false,
            symlink: false,
            owner: None,
            group_or_world_writable: None,
            reported_version: None,
            provenance: InstallationProvenance::Ambiguous,
        },
    )
}

#[cfg(windows)]
fn probe_native_windows_candidates() -> Vec<InstallationCandidate> {
    let output = match Command::new("where.exe").arg("opencode").output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };
    let stdout = match String::from_utf8(output.stdout) {
        Ok(stdout) => stdout,
        Err(_) => return Vec::new(),
    };

    stdout
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| {
            let local_path = Path::new(path);
            let canonical_path = local_path
                .canonicalize()
                .ok()
                .map(|value| value.to_string_lossy().into_owned());
            let symlink = local_path
                .symlink_metadata()
                .map(|metadata| metadata.file_type().is_symlink())
                .unwrap_or(false);
            InstallationCandidate::executable(
                HarnessId::new(OPENCODE_ADAPTER_ID),
                CandidateSource::NativeWindows,
                ExecutableCandidateEvidence {
                    wsl_distribution: None,
                    linux_user: None,
                    executable_path: path.to_string(),
                    canonical_path,
                    architecture: None,
                    filesystem: None,
                    path_class: CandidatePathClass::WindowsNative,
                    format: if path.to_ascii_lowercase().ends_with(".exe") {
                        ExecutableFormat::WindowsPe
                    } else {
                        ExecutableFormat::Unknown
                    },
                    regular_file: local_path.is_file(),
                    symlink,
                    owner: None,
                    group_or_world_writable: None,
                    reported_version: None,
                    provenance: InstallationProvenance::NativeWindows,
                },
            )
        })
        .collect()
}

#[cfg(windows)]
fn parse_probe_fields(output: &str) -> std::collections::BTreeMap<String, String> {
    output
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_string(), value.trim().to_string()))
        .collect()
}

#[cfg(windows)]
fn group_or_world_writable(mode: &str) -> Option<bool> {
    let mode = u32::from_str_radix(mode, 8).ok()?;
    Some(mode & 0o022 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wsl_evidence(
        path: &str,
        architecture: &str,
        version: &str,
        provenance: InstallationProvenance,
    ) -> ExecutableCandidateEvidence {
        ExecutableCandidateEvidence {
            wsl_distribution: Some(SELECTED_WSL_DISTRIBUTION.to_string()),
            linux_user: Some(SELECTED_LINUX_USER.to_string()),
            executable_path: path.to_string(),
            canonical_path: Some(path.to_string()),
            architecture: Some(architecture.to_string()),
            filesystem: Some(
                if path.starts_with("/mnt/") {
                    "9p"
                } else {
                    "ext4"
                }
                .to_string(),
            ),
            path_class: if path.starts_with("/mnt/") {
                CandidatePathClass::WindowsMounted
            } else {
                CandidatePathClass::LinuxNative
            },
            format: if path.starts_with("/mnt/") {
                ExecutableFormat::WindowsPe
            } else {
                ExecutableFormat::Elf64
            },
            regular_file: true,
            symlink: false,
            owner: Some(SELECTED_LINUX_USER.to_string()),
            group_or_world_writable: Some(false),
            reported_version: Some(version.to_string()),
            provenance,
        }
    }

    fn context_for(
        source: CandidateSource,
        evidence: ExecutableCandidateEvidence,
    ) -> DetectionContext {
        DetectionContext::new(vec![InstallationCandidate::executable(
            HarnessId::new(OPENCODE_ADAPTER_ID),
            source,
            evidence,
        )])
    }

    fn detected(context: &DetectionContext) -> DetectedInstallation {
        match OpenCodeAdapter::new().detect(context).unwrap() {
            DetectionReport::Detected(installation) => installation,
            report => panic!("expected detection, got {report:?}"),
        }
    }

    #[test]
    fn descriptor_models_official_interfaces_and_track_a_topology() {
        let adapter = OpenCodeAdapter::new();
        let descriptor = adapter.descriptor();

        assert_eq!(descriptor.id.as_str(), OPENCODE_ADAPTER_ID);
        assert_eq!(descriptor.official_interfaces.len(), 4);
        assert_eq!(descriptor.supported_topologies.len(), 2);
        assert!(descriptor.supported_topologies.iter().any(|topology| {
            topology.topology == ExecutionTopology::WslNative && topology.track_a_baseline
        }));
        assert!(descriptor.supported_topologies.iter().any(|topology| {
            topology.topology == ExecutionTopology::NativeWindows
                && topology.supported
                && !topology.track_a_baseline
        }));
    }

    #[test]
    fn no_candidates_is_not_installed() {
        let report = OpenCodeAdapter::new()
            .detect(&DetectionContext::default())
            .unwrap();
        assert!(matches!(
            report,
            DetectionReport::NotFound {
                code: "opencode.not-installed",
                ..
            }
        ));
    }

    #[test]
    fn recognizes_controlled_wsl_native_owned_candidate() {
        let installation = detected(&context_for(
            CandidateSource::WslDistribution,
            wsl_evidence(
                OWNED_EXECUTABLE_PATH,
                "x86_64",
                TESTED_VERSION,
                InstallationProvenance::HarnessHostOwned,
            ),
        ));

        assert_eq!(
            installation.classification(),
            DetectedInstallationClassification::ValidWslNative
        );
        assert_eq!(
            installation
                .executable_evidence()
                .unwrap()
                .wsl_distribution
                .as_deref(),
            Some(SELECTED_WSL_DISTRIBUTION)
        );
    }

    #[test]
    fn rejects_windows_mount_leakage_as_wsl_native() {
        let context = context_for(
            CandidateSource::WslDistribution,
            wsl_evidence(
                "/mnt/c/Users/example/AppData/Local/opencode.exe",
                "x86_64",
                TESTED_VERSION,
                InstallationProvenance::Foreign,
            ),
        );

        assert!(matches!(
            OpenCodeAdapter::new().detect(&context).unwrap(),
            DetectionReport::Rejected {
                classification: RejectedDetectionClassification::WindowsPathLeakage,
                code: "opencode.windows-path-in-wsl",
                ..
            }
        ));
    }

    #[test]
    fn rejects_wrong_architecture() {
        let context = context_for(
            CandidateSource::WslDistribution,
            wsl_evidence(
                OWNED_EXECUTABLE_PATH,
                "aarch64",
                TESTED_VERSION,
                InstallationProvenance::HarnessHostOwned,
            ),
        );

        assert!(matches!(
            OpenCodeAdapter::new().detect(&context).unwrap(),
            DetectionReport::Rejected {
                classification: RejectedDetectionClassification::WrongArchitecture,
                code: "opencode.wrong-architecture",
                ..
            }
        ));
    }

    #[test]
    fn wrong_version_is_unverified_and_start_blocked() {
        let adapter = OpenCodeAdapter::new();
        let installation = detected(&context_for(
            CandidateSource::WslDistribution,
            wsl_evidence(
                OWNED_EXECUTABLE_PATH,
                "x86_64",
                "1.18.24",
                InstallationProvenance::HarnessHostOwned,
            ),
        ));
        let version = adapter.version(&installation).unwrap();

        assert_eq!(version.detected_version, "1.18.24");
        assert_eq!(version.compatibility, CompatibilityState::UnverifiedVersion);
        assert!(version.start_blocked);
    }

    #[test]
    fn classifies_native_windows_as_supported_non_track_a() {
        let mut evidence = wsl_evidence(
            r"C:\Program Files\OpenCode\opencode.exe",
            "x86_64",
            TESTED_VERSION,
            InstallationProvenance::NativeWindows,
        );
        evidence.wsl_distribution = None;
        evidence.linux_user = None;
        evidence.canonical_path = Some(evidence.executable_path.clone());
        evidence.filesystem = None;
        evidence.path_class = CandidatePathClass::WindowsNative;
        evidence.format = ExecutableFormat::WindowsPe;
        evidence.owner = None;
        evidence.group_or_world_writable = None;
        let installation = detected(&context_for(CandidateSource::NativeWindows, evidence));

        assert_eq!(
            installation.classification(),
            DetectedInstallationClassification::NativeWindowsSupported
        );
        assert_eq!(
            OpenCodeAdapter::new()
                .version(&installation)
                .unwrap()
                .compatibility,
            CompatibilityState::TestedVersionMatch
        );
    }

    #[test]
    fn native_windows_shim_is_not_accepted_as_an_installation() {
        let mut evidence = wsl_evidence(
            r"C:\Users\example\AppData\Roaming\npm\opencode.cmd",
            "x86_64",
            TESTED_VERSION,
            InstallationProvenance::NativeWindows,
        );
        evidence.wsl_distribution = None;
        evidence.linux_user = None;
        evidence.canonical_path = Some(evidence.executable_path.clone());
        evidence.filesystem = None;
        evidence.path_class = CandidatePathClass::WindowsNative;
        evidence.format = ExecutableFormat::Unknown;
        evidence.owner = None;
        evidence.group_or_world_writable = None;
        let context = context_for(CandidateSource::NativeWindows, evidence);

        assert!(matches!(
            OpenCodeAdapter::new().detect(&context).unwrap(),
            DetectionReport::Rejected {
                classification: RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                code: "opencode.windows-provenance-untrusted",
                ..
            }
        ));
    }

    #[test]
    fn ambiguous_provenance_fails_closed() {
        let context = context_for(
            CandidateSource::WslDistribution,
            wsl_evidence(
                OWNED_EXECUTABLE_PATH,
                "x86_64",
                TESTED_VERSION,
                InstallationProvenance::Ambiguous,
            ),
        );

        assert!(matches!(
            OpenCodeAdapter::new().detect(&context).unwrap(),
            DetectionReport::Rejected {
                classification: RejectedDetectionClassification::AmbiguousOrUntrustedProvenance,
                code: "opencode.provenance-untrusted",
                ..
            }
        ));
    }

    #[cfg(windows)]
    #[test]
    fn selected_wsl_probe_never_resolves_opencode_through_inherited_path() {
        let command = selected_wsl_probe_command();
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let script = arguments.last().unwrap();

        assert_eq!(
            arguments[0..4],
            ["--distribution", "Ubuntu", "--user", "heathen"]
        );
        assert!(script.contains(OWNED_EXECUTABLE_PATH));
        assert!(!script.contains("which opencode"));
        assert!(!script.contains("command -v opencode"));
        assert!(!script.contains("$PATH"));
    }

    #[test]
    fn capability_manifest_is_version_scoped() {
        let manifest = OpenCodeAdapter::new().capability_manifest(None);
        assert_eq!(manifest.adapter_id.as_str(), OPENCODE_ADAPTER_ID);
        assert_eq!(manifest.evidence_version.version, TESTED_VERSION);
        assert_eq!(manifest.evidence_version.tag, TESTED_TAG);
        assert_eq!(
            manifest.evidence_version.source_commit,
            TESTED_SOURCE_COMMIT
        );
        assert!(manifest
            .entries
            .iter()
            .all(|entry| !entry.evidence.is_empty()));
    }
}
