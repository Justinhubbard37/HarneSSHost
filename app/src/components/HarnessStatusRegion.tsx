import type {
  CapabilityDomain,
  CapabilityEntry,
  CompatibilityState,
  HarnessSnapshot,
} from "../host/types";

interface HarnessStatusRegionProps {
  harnesses: HarnessSnapshot[];
}

const domains: CapabilityDomain[] = [
  "interaction",
  "tools",
  "agents",
  "governance",
  "session",
];

function compatibilityLabel(state?: CompatibilityState) {
  if (state === "testedVersionMatch") return "TestedVersionMatch";
  if (state === "unverifiedVersion") return "UnverifiedVersion";
  return "Unavailable";
}

function statusLabel(status: CapabilityEntry["status"]) {
  if (status === "supported-with-limitations") return "Supported with limitations";
  return status.charAt(0).toUpperCase() + status.slice(1);
}

export function HarnessStatusRegion({ harnesses }: HarnessStatusRegionProps) {
  return (
    <section className="panel" aria-labelledby="harnesses-heading">
      <p className="section-label">Harnesses</p>
      <h2 id="harnesses-heading">Detection and capability status</h2>

      {harnesses.map((harness) => {
        const evidenceVersion = harness.capabilityManifest.evidenceVersion;
        return (
          <article key={harness.id}>
            <h3>{harness.displayName}</h3>
            <div className="status-row">
              <div className="status-item">
                <p className="status-label">Detection</p>
                <p
                  className={`status-value ${
                    harness.detectionStatus === "detected" ? "success" : "error"
                  }`}
                >
                  {harness.detectionStatus === "detected" ? "Detected" : "Unavailable"}
                </p>
              </div>
              <div className="status-item">
                <p className="status-label">Source metadata version</p>
                <p className="status-value">{harness.detectedVersion ?? "Not detected"}</p>
              </div>
              <div className="status-item">
                <p className="status-label">Compatibility</p>
                <p className="status-value">{compatibilityLabel(harness.compatibility)}</p>
              </div>
              <div className="status-item">
                <p className="status-label">Runtime controls</p>
                <p className="status-value">Unavailable</p>
              </div>
            </div>

            <p className="compatibility-note">
              A TestedVersionMatch means source package metadata matches the tested version. It is
              not proof of executable, runtime, or source-revision identity.
            </p>

            {harness.sanitizedMessage ? (
              <p className="muted">{harness.sanitizedMessage}</p>
            ) : null}

            <div className="metadata-grid">
              <div className="status-item">
                <p className="status-label">Tested baseline</p>
                <p className="status-value">{evidenceVersion.version}</p>
                <p className="muted">{evidenceVersion.tag}</p>
              </div>
              <div className="status-item">
                <p className="status-label">Tested source commit</p>
                <code>{evidenceVersion.sourceCommit}</code>
              </div>
            </div>

            <div className="capability-groups">
              {domains.map((domain) => {
                const entries = harness.capabilityManifest.entries.filter(
                  (entry) => entry.domain === domain,
                );
                return (
                  <details className="capability-group" key={domain}>
                    <summary>
                      {domain.charAt(0).toUpperCase() + domain.slice(1)} | {entries.length}
                    </summary>
                    <ul className="capability-list">
                      {entries.map((entry) => (
                        <li className="capability-item" key={`${domain}-${entry.capability}`}>
                          <div className="capability-heading">
                            <strong>{entry.capability}</strong>
                            <span className="capability-status">{statusLabel(entry.status)}</span>
                          </div>
                          {entry.limitations ? (
                            <p className="capability-detail">{entry.limitations}</p>
                          ) : null}
                        </li>
                      ))}
                    </ul>
                  </details>
                );
              })}
            </div>
          </article>
        );
      })}
    </section>
  );
}
