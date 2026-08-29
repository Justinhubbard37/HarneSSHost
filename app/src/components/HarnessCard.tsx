import { useRef, useState, type SyntheticEvent } from "react";
import { getHarnessDetails } from "../host/client";
import type { HarnessCardState, HarnessCardSummary, HarnessDetailsPayload } from "../host/types";
import { HarnessDetails, type HarnessDetailsState } from "./HarnessDetails";

interface HarnessCardProps {
  harness: HarnessCardSummary;
}

const stateLabels: Record<HarnessCardState, string> = {
  notInstalled: "Not installed",
  ready: "Ready",
  problemDetected: "Problem detected",
  unsupportedLocalInstallation: "Unsupported local installation",
};

export function HarnessCard({ harness }: HarnessCardProps) {
  const [detailsState, setDetailsState] = useState<HarnessDetailsState>({ status: "idle" });
  const detailsRequested = useRef(false);
  const headingId = `harness-${harness.id}-heading`;

  const requestDetails = async () => {
    setDetailsState({ status: "loading" });
    try {
      const details: HarnessDetailsPayload = await getHarnessDetails(harness.id);
      setDetailsState({ status: "ready", details });
    } catch {
      detailsRequested.current = false;
      setDetailsState({ status: "error" });
    }
  };

  const handleToggle = (event: SyntheticEvent<HTMLDetailsElement>) => {
    if (!event.currentTarget.open || detailsRequested.current) {
      return;
    }

    detailsRequested.current = true;
    void requestDetails();
  };

  return (
    <article className="harness-card" aria-labelledby={headingId}>
      <div className="harness-card-header">
        <div>
          <h2 id={headingId}>{harness.displayName}</h2>
          <p>{harness.description}</p>
        </div>
        <span className={`card-state card-state-${harness.state}`}>
          {stateLabels[harness.state]}
        </span>
      </div>

      {harness.hasDetails ? (
        <details className="harness-disclosure" onToggle={handleToggle}>
          <summary>Details</summary>
          <HarnessDetails state={detailsState} />
        </details>
      ) : null}
    </article>
  );
}
