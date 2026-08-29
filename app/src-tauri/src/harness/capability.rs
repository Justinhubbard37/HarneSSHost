use crate::harness::adapter::{CompatibilityState, HarnessId, TestedVersion};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CapabilityDomain {
    Interaction,
    Tools,
    Agents,
    Governance,
    Session,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)] // The shared contract must represent states not used by the initial DeepSeek manifest.
pub(crate) enum CapabilityStatus {
    Unsupported,
    Supported,
    SupportedWithLimitations,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EvidenceProvenance {
    Source,
    DirectorValidatedBehavior,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityEvidence {
    pub(crate) provenance: EvidenceProvenance,
    pub(crate) reference: String,
    pub(crate) summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityEntry {
    pub(crate) domain: CapabilityDomain,
    pub(crate) capability: String,
    pub(crate) status: CapabilityStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) limitations: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) notes: Option<String>,
    pub(crate) evidence: Vec<CapabilityEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityManifest {
    pub(crate) adapter_id: HarnessId,
    pub(crate) adapter_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detected_version: Option<String>,
    pub(crate) evidence_version: TestedVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) compatibility: Option<CompatibilityState>,
    pub(crate) entries: Vec<CapabilityEntry>,
}
