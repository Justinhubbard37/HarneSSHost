export type DetectionStatus = "detected" | "notFound" | "invalid" | "error";

export type CompatibilityState =
  | "testedVersionMatch"
  | "unverifiedVersion";

export type CapabilityDomain =
  | "interaction"
  | "tools"
  | "agents"
  | "governance"
  | "session";

export type CapabilityStatus =
  | "unsupported"
  | "supported"
  | "supported-with-limitations"
  | "unknown";

export interface TestedVersion {
  version: string;
  tag: string;
  sourceCommit: string;
}

export interface CapabilityEvidence {
  provenance: "source" | "director-validated-behavior";
  reference: string;
  summary: string;
}

export interface CapabilityEntry {
  domain: CapabilityDomain;
  capability: string;
  status: CapabilityStatus;
  limitations?: string;
  notes?: string;
  evidence: CapabilityEvidence[];
}

export interface CapabilityManifest {
  adapterId: string;
  adapterName: string;
  detectedVersion?: string;
  evidenceVersion: TestedVersion;
  compatibility?: CompatibilityState;
  entries: CapabilityEntry[];
}

export interface HarnessSnapshot {
  id: string;
  displayName: string;
  detectionStatus: DetectionStatus;
  diagnosticCode?: string;
  sanitizedMessage?: string;
  detectedVersion?: string;
  testedVersions: TestedVersion[];
  compatibility?: CompatibilityState;
  startBlocked: boolean;
  capabilityManifest: CapabilityManifest;
}

export interface HostSurfaceState {
  kind: "noHarnessActive" | "loading" | "unavailable" | "error";
  message?: string;
}

export interface HostSnapshot {
  productName: string;
  interfaceState: HostSurfaceState;
  harnesses: HarnessSnapshot[];
}
