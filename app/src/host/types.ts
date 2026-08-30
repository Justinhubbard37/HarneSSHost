export type HarnessCardState =
  | "notInstalled"
  | "ready"
  | "starting"
  | "open"
  | "stopping"
  | "failed"
  | "problemDetected"
  | "unsupportedLocalInstallation";

export interface HarnessCardSummary {
  id: string;
  displayName: string;
  description: string;
  state: HarnessCardState;
  canOpen: boolean;
  failureMessage?: string;
  hasDetails: boolean;
}

export interface HarnessLibraryPayload {
  harnesses: HarnessCardSummary[];
  surface: HostSurfaceState;
}

export type DetectionStatus = "detected" | "notInstalled" | "invalid" | "error";

export type CompatibilityState = "testedVersionMatch" | "unverifiedVersion";

export type RuntimePhase = "inactive" | "starting" | "ready" | "stopping" | "failed";

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

export interface HarnessDetectionDetails {
  status: DetectionStatus;
  code?: string;
  message?: string;
  detectedVersion?: string;
  compatibility?: CompatibilityState;
}

export interface HarnessRuntimeDetails {
  phase: RuntimePhase;
  failureCode?: string;
}

export interface HarnessDetailsPayload {
  id: string;
  displayName: string;
  description: string;
  detection: HarnessDetectionDetails;
  runtime: HarnessRuntimeDetails;
  capabilityManifest: CapabilityManifest;
}

export interface HostSurfaceState {
  kind:
    | "noHarnessActive"
    | "loading"
    | "officialInterfaceAvailable"
    | "unavailable"
    | "error";
  message?: string;
}

export interface OpenHarnessResult {
  phase: RuntimePhase;
  canOpen: boolean;
  surface: HostSurfaceState;
}

export interface RuntimeChangedEvent {
  harnessId: string;
  phase: RuntimePhase;
  canOpen: boolean;
  surface: HostSurfaceState;
}
