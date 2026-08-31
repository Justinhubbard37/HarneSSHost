export type HarnessCardState =
  | "notInstalled"
  | "detected"
  | "supportedNonBaseline"
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

export type DetectionClassification =
  | "notInstalled"
  | "validInstallation"
  | "validWslNative"
  | "nativeWindowsSupported"
  | "wrongVersion"
  | "wrongArchitecture"
  | "windowsPathLeakage"
  | "ambiguousOrUntrustedProvenance"
  | "invalid"
  | "error";

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
  classification: DetectionClassification;
  code?: string;
  message?: string;
  detectedVersion?: string;
  compatibility?: CompatibilityState;
  provenance?: HarnessProvenanceDetails;
}

export interface HarnessRuntimeDetails {
  available: boolean;
  phase?: RuntimePhase;
  failureCode?: string;
}

export type CandidatePathClass =
  | "linuxNative"
  | "windowsMounted"
  | "windowsNative"
  | "unknown";

export type InstallationProvenance =
  | "harnessHostOwned"
  | "nativeWindows"
  | "foreign"
  | "ambiguous";

export interface HarnessProvenanceDetails {
  wslDistribution?: string;
  linuxUser?: string;
  architecture?: string;
  filesystem?: string;
  pathClass: CandidatePathClass;
  installationProvenance: InstallationProvenance;
  trackABaseline: boolean;
}

export type OfficialInterfaceKind =
  | "terminalUi"
  | "web"
  | "desktopApp"
  | "ideExtension";

export interface OfficialInterfaceFact {
  kind: OfficialInterfaceKind;
  sourceReference: string;
}

export type ExecutionTopology = "wslNative" | "nativeWindows";

export interface SupportedTopologyFact {
  topology: ExecutionTopology;
  supported: boolean;
  trackABaseline: boolean;
  sourceReference: string;
}

export interface HarnessDetailsPayload {
  id: string;
  displayName: string;
  description: string;
  officialInterfaces: OfficialInterfaceFact[];
  supportedTopologies: SupportedTopologyFact[];
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
  harnessId: string;
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
