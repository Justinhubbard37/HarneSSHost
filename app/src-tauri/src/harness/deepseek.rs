use crate::harness::adapter::{
    AdapterError, CompatibilityState, DetectedInstallation, DetectionContext, DetectionReport,
    HarnessAdapter, HarnessDescriptor, HarnessId, TestedVersion, VersionReport,
};
use crate::harness::capability::{
    CapabilityDomain, CapabilityEntry, CapabilityEvidence, CapabilityManifest, CapabilityStatus,
    EvidenceProvenance,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

pub(crate) const DEEPSEEK_ADAPTER_ID: &str = "deepseek";
pub(crate) const TESTED_VERSION: &str = "0.1.2-alpha.1";
pub(crate) const TESTED_TAG: &str = "dsh-v0.1.2-alpha.1";
pub(crate) const TESTED_SOURCE_COMMIT: &str = "cd5ef8148158c3a752a658978873241fdf8e2bbc";

pub(crate) struct DeepSeekAdapter {
    descriptor: HarnessDescriptor,
}

impl DeepSeekAdapter {
    pub(crate) fn new() -> Self {
        Self {
            descriptor: HarnessDescriptor {
                id: HarnessId::new(DEEPSEEK_ADAPTER_ID),
                display_name: "DeepSeek Harness".to_string(),
                description:
                    "A local agent harness with chat, tools, agents, governance, and durable sessions."
                        .to_string(),
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
}

impl HarnessAdapter for DeepSeekAdapter {
    fn descriptor(&self) -> &HarnessDescriptor {
        &self.descriptor
    }

    fn detect(&self, context: &DetectionContext) -> Result<DetectionReport, AdapterError> {
        for candidate in context.candidates_for(&self.descriptor.id) {
            let canonical_path = match fs::canonicalize(&candidate.path) {
                Ok(path) if path.is_dir() => path,
                Ok(_) => {
                    return Ok(DetectionReport::Invalid {
                        code: "deepseek.candidate-not-directory",
                        message: "The configured DeepSeek candidate is not a directory."
                            .to_string(),
                    });
                }
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(_) => {
                    return Ok(DetectionReport::Error {
                        code: "deepseek.candidate-unreadable",
                        message: "The configured DeepSeek candidate could not be inspected."
                            .to_string(),
                    });
                }
            };

            let root_package = match read_package(&canonical_path.join("package.json")) {
                Ok(package) => package,
                Err(error) => {
                    return Ok(DetectionReport::Invalid {
                        code: error.code,
                        message: error.message,
                    });
                }
            };
            if root_package.name.as_deref() != Some("@deepseek-ai/dsh-root") {
                return Ok(DetectionReport::Invalid {
                    code: "deepseek.root-package-mismatch",
                    message: "The candidate is not a DeepSeek Harness source checkout.".to_string(),
                });
            }
            if !root_package
                .scripts
                .as_ref()
                .and_then(|scripts| scripts.get("dsh"))
                .is_some_and(|script| !script.trim().is_empty())
            {
                return Ok(DetectionReport::Invalid {
                    code: "deepseek.root-cli-script-missing",
                    message: "The DeepSeek Harness source launcher metadata is missing."
                        .to_string(),
                });
            }

            let cli_package_path = canonical_path.join("apps").join("cli").join("package.json");
            let cli_package = match read_package(&cli_package_path) {
                Ok(package) => package,
                Err(error) => {
                    return Ok(DetectionReport::Invalid {
                        code: error.code,
                        message: error.message,
                    });
                }
            };
            if cli_package.name.as_deref() != Some("@deepseek-ai/dsh") {
                return Ok(DetectionReport::Invalid {
                    code: "deepseek.cli-package-mismatch",
                    message: "The candidate does not contain the expected DeepSeek CLI package."
                        .to_string(),
                });
            }
            if !cli_package
                .bin
                .as_ref()
                .is_some_and(|bins| bins.contains_key("dsh"))
            {
                return Ok(DetectionReport::Invalid {
                    code: "deepseek.cli-bin-missing",
                    message: "The DeepSeek CLI package does not declare the dsh executable."
                        .to_string(),
                });
            }
            if !canonical_path
                .join("apps")
                .join("cli")
                .join("src")
                .join("bin.ts")
                .is_file()
            {
                return Ok(DetectionReport::Invalid {
                    code: "deepseek.cli-source-missing",
                    message: "The DeepSeek CLI source entry point is missing.".to_string(),
                });
            }

            return Ok(DetectionReport::Detected(DetectedInstallation::new(
                self.descriptor.id.clone(),
                candidate.source,
                canonical_path,
            )));
        }

        Ok(DetectionReport::NotFound {
            code: "deepseek.not-found",
            message: "DeepSeek Harness was not found in the configured development candidates."
                .to_string(),
        })
    }

    fn version(&self, installation: &DetectedInstallation) -> Result<VersionReport, AdapterError> {
        if installation.adapter_id() != &self.descriptor.id {
            return Err(AdapterError::new(
                "deepseek.installation-mismatch",
                "The detected installation belongs to a different harness adapter.",
            ));
        }

        let root_package = read_package(&installation.path().join("package.json"))?;
        let cli_package = read_package(
            &installation
                .path()
                .join("apps")
                .join("cli")
                .join("package.json"),
        )?;

        let root_version = package_version(&root_package, "root")?;
        let cli_version = package_version(&cli_package, "CLI")?;
        if root_version != cli_version {
            return Err(AdapterError::new(
                "deepseek.version-mismatch",
                "The DeepSeek root and CLI package versions do not match.",
            ));
        }

        let compatibility = if root_version == TESTED_VERSION {
            CompatibilityState::TestedVersionMatch
        } else {
            CompatibilityState::UnverifiedVersion
        };

        Ok(VersionReport {
            detected_version: root_version.to_string(),
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
            entries: capability_entries(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PackageMetadata {
    name: Option<String>,
    version: Option<String>,
    scripts: Option<BTreeMap<String, String>>,
    bin: Option<BTreeMap<String, String>>,
}

fn read_package(path: &Path) -> Result<PackageMetadata, AdapterError> {
    let source = fs::read_to_string(path).map_err(|_| {
        AdapterError::new(
            "deepseek.package-unreadable",
            "Required DeepSeek package metadata could not be read.",
        )
    })?;
    serde_json::from_str(&source).map_err(|_| {
        AdapterError::new(
            "deepseek.package-invalid",
            "Required DeepSeek package metadata is not valid JSON.",
        )
    })
}

fn package_version<'a>(
    package: &'a PackageMetadata,
    package_label: &str,
) -> Result<&'a str, AdapterError> {
    package
        .version
        .as_deref()
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .ok_or_else(|| {
            AdapterError::new(
                "deepseek.version-missing",
                format!("The DeepSeek {package_label} package version is missing."),
            )
        })
}

fn capability_entries() -> Vec<CapabilityEntry> {
    vec![
        entry(
            CapabilityDomain::Interaction,
            "chat",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/bundle/web-app/cordis.patch.yml",
                "The official Web composition mounts the chat interface.",
            )],
        ),
        entry(
            CapabilityDomain::Interaction,
            "streaming",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/llm/llm/README.md",
                "The provider-neutral runtime defines token-level stream chunks.",
            )],
        ),
        entry(
            CapabilityDomain::Interaction,
            "attachments",
            CapabilityStatus::SupportedWithLimitations,
            Some("Durable image attachments support PNG, JPEG, WebP, and GIF."),
            None,
            vec![source(
                "packages/attachment/attachment/README.md",
                "The attachment package documents durable image formats.",
            )],
        ),
        entry(
            CapabilityDomain::Interaction,
            "multimodal",
            CapabilityStatus::SupportedWithLimitations,
            Some("Image input requires a selected route and model that declare image support."),
            None,
            vec![source(
                "docs/user/guide/providers.md",
                "Provider configuration documents model-dependent image input.",
            )],
        ),
        entry(
            CapabilityDomain::Tools,
            "functionCalling",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/core/agent-loop/README.md",
                "The core loop exposes tool schemas and guarded tool dispatch.",
            )],
        ),
        entry(
            CapabilityDomain::Tools,
            "filesystem",
            CapabilityStatus::Supported,
            None,
            None,
            vec![
                source(
                    "packages/preset/agent-presets/presets/standard/agent.cordis.yml",
                    "The standard preset mounts filesystem and search tools.",
                ),
                behavior("filesystem-read-write-validation", "Filesystem reads and controlled writes were independently validated."),
            ],
        ),
        entry(
            CapabilityDomain::Tools,
            "shell",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/preset/agent-presets/presets/standard/agent.cordis.yml",
                "The standard preset selects Bash or PowerShell by platform.",
            )],
        ),
        entry(
            CapabilityDomain::Tools,
            "web",
            CapabilityStatus::SupportedWithLimitations,
            Some("Search requires provider credentials and fetch rejects non-public destinations."),
            None,
            vec![source(
                "apps/cli/reference/README.md",
                "The CLI reference documents search credentials and fetch restrictions.",
            )],
        ),
        entry(
            CapabilityDomain::Tools,
            "MCP",
            CapabilityStatus::SupportedWithLimitations,
            Some("The MCP client ships, but no server is enabled by default because server commands are trusted executable code."),
            None,
            vec![source(
                "apps/cli/reference/README.md",
                "The CLI reference documents the opt-in MCP client boundary.",
            )],
        ),
        entry(
            CapabilityDomain::Tools,
            "customTools",
            CapabilityStatus::SupportedWithLimitations,
            Some("Trusted bundles or presets can supply tools; installation and restart are required."),
            None,
            vec![source(
                "apps/cli/reference/README.md",
                "The CLI reference documents trusted bundle and preset installation.",
            )],
        ),
        entry(
            CapabilityDomain::Agents,
            "subagents",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/bundle/base/cordis.patch.yml",
                "The base composition mounts in-process spawn and fork providers.",
            )],
        ),
        entry(
            CapabilityDomain::Agents,
            "delegation",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/preset/agent-presets/presets/standard/agent.cordis.yml",
                "The standard preset exposes spawn, fork, follow-up, and listing tools.",
            )],
        ),
        entry(
            CapabilityDomain::Agents,
            "planning",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/preset/agent-presets/presets/standard/agent.cordis.yml",
                "The standard preset mounts isolated plan mode.",
            )],
        ),
        entry(
            CapabilityDomain::Agents,
            "taskManagement",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/preset/agent-presets/presets/standard/agent.cordis.yml",
                "The standard preset includes jobs, goals, workflows, and todo tools.",
            )],
        ),
        entry(
            CapabilityDomain::Governance,
            "approvals",
            CapabilityStatus::Supported,
            None,
            None,
            vec![
                source(
                    "packages/bundle/base/cordis.patch.yml",
                    "The base composition mounts the user-approval service.",
                ),
                behavior("approval-flow-validation", "Allow once and Reject behavior were independently validated."),
            ],
        ),
        entry(
            CapabilityDomain::Governance,
            "permissions",
            CapabilityStatus::Supported,
            None,
            None,
            vec![
                source(
                    "packages/bundle/base/cordis.patch.yml",
                    "The base composition defines read-only, workspace-write, and danger-full-access presets.",
                ),
                behavior("permission-reversion-validation", "Read-only enforcement and one-time write escalation reversion were independently validated."),
            ],
        ),
        entry(
            CapabilityDomain::Governance,
            "sandboxing",
            CapabilityStatus::SupportedWithLimitations,
            Some("Enforcement and process visibility depend on the platform and selected backend."),
            None,
            vec![
                source(
                    "apps/cli/reference/README.md",
                    "The CLI reference documents platform-dependent sandbox enforcement.",
                ),
                behavior("windows-sandbox-validation", "The Windows read-only and approval boundaries were independently validated."),
            ],
        ),
        entry(
            CapabilityDomain::Governance,
            "humanInLoop",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/preset/agent-presets/presets/standard/agent.cordis.yml",
                "The standard preset mounts approval prompts and the ask-user tool.",
            )],
        ),
        entry(
            CapabilityDomain::Session,
            "persistence",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/bundle/base/cordis.patch.yml",
                "The base profile persists sessions under the DeepSeek home directory.",
            )],
        ),
        entry(
            CapabilityDomain::Session,
            "history",
            CapabilityStatus::Supported,
            None,
            None,
            vec![source(
                "packages/session/session-persistence-jsonl/README.md",
                "Durable session logs support inspection and resumed history.",
            )],
        ),
        entry(
            CapabilityDomain::Session,
            "checkpoints",
            CapabilityStatus::SupportedWithLimitations,
            Some("DeepSeek provides durable session and checkpoint behavior, but this is not an unrestricted generic HarneSSHost checkpoint capability."),
            Some("The declaration is scoped to the tested DeepSeek baseline."),
            vec![source(
                "packages/session/session-checkpoint-policy/README.md",
                "The checkpoint policy flushes requests, top-level tool calls, and step boundaries around side effects.",
            )],
        ),
        entry(
            CapabilityDomain::Session,
            "artifacts",
            CapabilityStatus::SupportedWithLimitations,
            Some("The official UI exposes files produced by recognized first-party mutation tools; it is not a generic artifact system."),
            None,
            vec![source(
                "packages/client/ui-deliverables/README.md",
                "The deliverables UI documents recognized first-party mutation outputs.",
            )],
        ),
    ]
}

fn entry(
    domain: CapabilityDomain,
    capability: &str,
    status: CapabilityStatus,
    limitations: Option<&str>,
    notes: Option<&str>,
    evidence: Vec<CapabilityEvidence>,
) -> CapabilityEntry {
    CapabilityEntry {
        domain,
        capability: capability.to_string(),
        status,
        limitations: limitations.map(str::to_string),
        notes: notes.map(str::to_string),
        evidence,
    }
}

fn source(reference: &str, summary: &str) -> CapabilityEvidence {
    CapabilityEvidence {
        provenance: EvidenceProvenance::Source,
        reference: reference.to_string(),
        summary: summary.to_string(),
    }
}

fn behavior(reference: &str, summary: &str) -> CapabilityEvidence {
    CapabilityEvidence {
        provenance: EvidenceProvenance::DirectorValidatedBehavior,
        reference: reference.to_string(),
        summary: summary.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::adapter::{CandidateSource, InstallationCandidate};
    use std::collections::HashSet;
    use tempfile::TempDir;

    fn checkout(root_name: &str, root_version: &str, cli_name: &str, cli_version: &str) -> TempDir {
        let directory = tempfile::tempdir().unwrap();
        let cli_source = directory.path().join("apps").join("cli").join("src");
        fs::create_dir_all(&cli_source).unwrap();
        fs::write(cli_source.join("bin.ts"), "export {};\n").unwrap();
        fs::write(
            directory.path().join("package.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": root_name,
                "version": root_version,
                "scripts": { "dsh": "node apps/cli/src/bin.ts" }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            directory
                .path()
                .join("apps")
                .join("cli")
                .join("package.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": cli_name,
                "version": cli_version,
                "bin": { "dsh": "lib/bin.js" }
            }))
            .unwrap(),
        )
        .unwrap();
        directory
    }

    fn context_for(path: &Path) -> DetectionContext {
        DetectionContext::new(vec![InstallationCandidate::new(
            HarnessId::new(DEEPSEEK_ADAPTER_ID),
            CandidateSource::DevelopmentCheckout,
            path.to_path_buf(),
        )])
    }

    fn detected(adapter: &DeepSeekAdapter, context: &DetectionContext) -> DetectedInstallation {
        match adapter.detect(context).unwrap() {
            DetectionReport::Detected(installation) => installation,
            report => panic!("expected detection, got {report:?}"),
        }
    }

    #[test]
    fn reports_missing_candidate_without_exposing_a_path() {
        let adapter = DeepSeekAdapter::new();
        let context = context_for(Path::new("definitely-not-a-real-deepseek-checkout"));

        let report = adapter.detect(&context).unwrap();
        assert!(matches!(report, DetectionReport::NotFound { .. }));
        assert!(!format!("{report:?}").contains("C:\\"));
    }

    #[test]
    fn ignores_candidates_for_other_adapters() {
        let directory = checkout(
            "@deepseek-ai/dsh-root",
            TESTED_VERSION,
            "@deepseek-ai/dsh",
            TESTED_VERSION,
        );
        let context = DetectionContext::new(vec![InstallationCandidate::new(
            HarnessId::new("other"),
            CandidateSource::DevelopmentCheckout,
            directory.path().to_path_buf(),
        )]);

        assert!(matches!(
            DeepSeekAdapter::new().detect(&context).unwrap(),
            DetectionReport::NotFound { .. }
        ));
    }

    #[test]
    fn rejects_wrong_root_package_identity() {
        let directory = checkout(
            "not-deepseek",
            TESTED_VERSION,
            "@deepseek-ai/dsh",
            TESTED_VERSION,
        );
        let report = DeepSeekAdapter::new()
            .detect(&context_for(directory.path()))
            .unwrap();

        assert!(matches!(
            report,
            DetectionReport::Invalid {
                code: "deepseek.root-package-mismatch",
                ..
            }
        ));
    }

    #[test]
    fn rejects_missing_cli_source_entry() {
        let directory = checkout(
            "@deepseek-ai/dsh-root",
            TESTED_VERSION,
            "@deepseek-ai/dsh",
            TESTED_VERSION,
        );
        fs::remove_file(
            directory
                .path()
                .join("apps")
                .join("cli")
                .join("src")
                .join("bin.ts"),
        )
        .unwrap();

        let report = DeepSeekAdapter::new()
            .detect(&context_for(directory.path()))
            .unwrap();
        assert!(matches!(
            report,
            DetectionReport::Invalid {
                code: "deepseek.cli-source-missing",
                ..
            }
        ));
    }

    #[test]
    fn classifies_matching_metadata_as_tested_version_match() {
        let directory = checkout(
            "@deepseek-ai/dsh-root",
            TESTED_VERSION,
            "@deepseek-ai/dsh",
            TESTED_VERSION,
        );
        let adapter = DeepSeekAdapter::new();
        let installation = detected(&adapter, &context_for(directory.path()));
        let report = adapter.version(&installation).unwrap();

        assert_eq!(installation.source(), CandidateSource::DevelopmentCheckout);
        assert_eq!(report.detected_version, TESTED_VERSION);
        assert_eq!(report.compatibility, CompatibilityState::TestedVersionMatch);
        assert!(!report.start_blocked);
        assert_eq!(report.tested_versions[0].tag, TESTED_TAG);
        assert_eq!(
            report.tested_versions[0].source_commit,
            TESTED_SOURCE_COMMIT
        );
    }

    #[test]
    fn classifies_other_metadata_as_unverified_and_start_blocked() {
        let directory = checkout(
            "@deepseek-ai/dsh-root",
            "0.1.3",
            "@deepseek-ai/dsh",
            "0.1.3",
        );
        let adapter = DeepSeekAdapter::new();
        let installation = detected(&adapter, &context_for(directory.path()));
        let report = adapter.version(&installation).unwrap();

        assert_eq!(report.compatibility, CompatibilityState::UnverifiedVersion);
        assert!(report.start_blocked);
    }

    #[test]
    fn rejects_mismatched_package_versions() {
        let directory = checkout(
            "@deepseek-ai/dsh-root",
            TESTED_VERSION,
            "@deepseek-ai/dsh",
            "0.1.3",
        );
        let adapter = DeepSeekAdapter::new();
        let installation = detected(&adapter, &context_for(directory.path()));
        let error = adapter.version(&installation).unwrap_err();

        assert_eq!(error.code, "deepseek.version-mismatch");
    }

    #[test]
    fn capability_manifest_is_complete_evidenced_and_version_scoped() {
        let adapter = DeepSeekAdapter::new();
        let manifest = adapter.capability_manifest(None);
        let keys = manifest
            .entries
            .iter()
            .map(|entry| format!("{:?}:{}", entry.domain, entry.capability))
            .collect::<HashSet<_>>();

        assert_eq!(manifest.entries.len(), 22);
        assert_eq!(keys.len(), 22);
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
        assert!(manifest.entries.iter().all(|entry| {
            entry.status != CapabilityStatus::SupportedWithLimitations
                || entry
                    .limitations
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
        }));

        let checkpoints = manifest
            .entries
            .iter()
            .find(|entry| entry.capability == "checkpoints")
            .unwrap();
        assert_eq!(
            checkpoints.status,
            CapabilityStatus::SupportedWithLimitations
        );
        assert!(checkpoints
            .limitations
            .as_deref()
            .unwrap()
            .contains("not an unrestricted generic HarneSSHost checkpoint capability"));
    }
}
