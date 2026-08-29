import type {
  CapabilityDomain,
  CapabilityEntry,
  CompatibilityState,
  DetectionStatus,
  HarnessDetailsPayload,
  RuntimePhase,
} from "../host/types";

export type HarnessDetailsState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; details: HarnessDetailsPayload }
  | { status: "error" };

interface HarnessDetailsProps {
  state: HarnessDetailsState;
}

const domains: CapabilityDomain[] = [
  "interaction",
  "tools",
  "agents",
  "governance",
  "session",
];

function titleCase(value: string) {
  return value
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/-/g, " ")
    .replace(/^./, (first) => first.toUpperCase());
}

function detectionLabel(status: DetectionStatus) {
  if (status === "notInstalled") return "Not installed";
  return titleCase(status);
}

function compatibilityLabel(state?: CompatibilityState) {
  if (state === "testedVersionMatch") return "Tested baseline match";
  if (state === "unverifiedVersion") return "Unverified version";
  return "Not available";
}

function runtimeLabel(phase: RuntimePhase) {
  return titleCase(phase);
}

function capabilityStatusLabel(status: CapabilityEntry["status"]) {
  return titleCase(status);
}

export function HarnessDetails({ state }: HarnessDetailsProps) {
  if (state.status === "idle" || state.status === "loading") {
    return (
      <div className="details-state" aria-live="polite">
        Loading technical details…
      </div>
    );
  }

  if (state.status === "error") {
    return (
      <div className="details-state details-error" role="alert">
        Technical details are unavailable.
      </div>
    );
  }

  const { details } = state;
  const baseline = details.capabilityManifest.evidenceVersion;

  return (
    <div className="harness-details">
      <section className="details-section" aria-labelledby={`${details.id}-installation-heading`}>
        <h3 id={`${details.id}-installation-heading`}>Installation</h3>
        <dl className="fact-list">
          <div>
            <dt>Detection</dt>
            <dd>{detectionLabel(details.detection.status)}</dd>
          </div>
          <div>
            <dt>Detected version</dt>
            <dd>{details.detection.detectedVersion ?? "Not detected"}</dd>
          </div>
          <div>
            <dt>Compatibility</dt>
            <dd>{compatibilityLabel(details.detection.compatibility)}</dd>
          </div>
          <div>
            <dt>Runtime</dt>
            <dd>{runtimeLabel(details.runtime.phase)}</dd>
          </div>
        </dl>
        {details.detection.message ? (
          <p className="technical-note">{details.detection.message}</p>
        ) : null}
        {details.detection.code ? (
          <code className="diagnostic-code">{details.detection.code}</code>
        ) : null}
      </section>

      <section className="details-section" aria-labelledby={`${details.id}-baseline-heading`}>
        <h3 id={`${details.id}-baseline-heading`}>Tested baseline</h3>
        <dl className="baseline-list">
          <div>
            <dt>Version</dt>
            <dd>{baseline.version}</dd>
          </div>
          <div>
            <dt>Tag</dt>
            <dd>{baseline.tag}</dd>
          </div>
          <div>
            <dt>Source commit</dt>
            <dd>
              <code>{baseline.sourceCommit}</code>
            </dd>
          </div>
        </dl>
      </section>

      <section className="details-section" aria-labelledby={`${details.id}-capabilities-heading`}>
        <h3 id={`${details.id}-capabilities-heading`}>Capabilities</h3>
        <div className="capability-groups">
          {domains.map((domain) => {
            const entries = details.capabilityManifest.entries.filter(
              (entry) => entry.domain === domain,
            );
            return (
              <details className="capability-group" key={domain}>
                <summary>
                  <span>{titleCase(domain)}</span>
                  <span>{entries.length}</span>
                </summary>
                <ul>
                  {entries.map((entry) => (
                    <li key={`${domain}-${entry.capability}`}>
                      <div className="capability-heading">
                        <strong>{titleCase(entry.capability)}</strong>
                        <span>{capabilityStatusLabel(entry.status)}</span>
                      </div>
                      {entry.limitations ? <p>{entry.limitations}</p> : null}
                    </li>
                  ))}
                </ul>
              </details>
            );
          })}
        </div>
      </section>
    </div>
  );
}
