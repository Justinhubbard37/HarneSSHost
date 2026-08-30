import { useCallback, useEffect, useRef, useState, type SyntheticEvent } from "react";
import { getHarnessDetails } from "../host/client";
import { deriveOpenAction } from "../host/openAction";
import type { HarnessCardState, HarnessCardSummary, HarnessDetailsPayload } from "../host/types";
import { HarnessDetails, type HarnessDetailsState } from "./HarnessDetails";

interface HarnessCardProps {
  harness: HarnessCardSummary;
  onOpen: (harnessId: string) => Promise<void>;
}

const stateLabels: Record<HarnessCardState, string> = {
  notInstalled: "Not installed",
  ready: "Ready",
  starting: "Opening",
  open: "Open",
  stopping: "Closing",
  failed: "Could not open",
  problemDetected: "Problem detected",
  unsupportedLocalInstallation: "Unsupported local installation",
};

export function HarnessCard({ harness, onOpen }: HarnessCardProps) {
  const [detailsState, setDetailsState] = useState<HarnessDetailsState>({ status: "idle" });
  const [requestPending, setRequestPending] = useState(false);
  const [openError, setOpenError] = useState(false);
  const detailsRequested = useRef(false);
  const headingId = `harness-${harness.id}-heading`;
  const openAction = deriveOpenAction(harness.state, harness.canOpen, requestPending);

  const requestDetails = useCallback(async () => {
    setDetailsState({ status: "loading" });
    try {
      const details: HarnessDetailsPayload = await getHarnessDetails(harness.id);
      setDetailsState({ status: "ready", details });
    } catch {
      detailsRequested.current = false;
      setDetailsState({ status: "error" });
    }
  }, [harness.id]);

  useEffect(() => {
    if (detailsRequested.current) {
      void requestDetails();
    }
  }, [harness.state, requestDetails]);

  const handleToggle = (event: SyntheticEvent<HTMLDetailsElement>) => {
    if (!event.currentTarget.open || detailsRequested.current) {
      return;
    }

    detailsRequested.current = true;
    void requestDetails();
  };

  const handleOpen = async () => {
    if (!openAction.visible || openAction.disabled) {
      return;
    }
    setRequestPending(true);
    setOpenError(false);
    try {
      await onOpen(harness.id);
    } catch {
      setOpenError(true);
    } finally {
      setRequestPending(false);
    }
  };

  return (
    <article className="harness-card" aria-labelledby={headingId}>
      <div className="harness-card-header">
        <div>
          <h2 id={headingId}>{harness.displayName}</h2>
          <p>{harness.description}</p>
        </div>
        <div className="harness-card-actions">
          <span className={`card-state card-state-${harness.state}`}>
            {stateLabels[harness.state]}
          </span>
          {openAction.visible ? (
            <button
              type="button"
              className="open-harness-button"
              disabled={openAction.disabled}
              onClick={() => void handleOpen()}
            >
              {openAction.label}
            </button>
          ) : null}
        </div>
      </div>

      {harness.failureMessage ? (
        <p className="harness-action-message" role="status">
          {harness.failureMessage}
        </p>
      ) : null}
      {openError ? (
        <p className="harness-action-message harness-action-error" role="alert">
          DeepSeek could not be opened. Technical information is available under Details.
        </p>
      ) : null}

      {harness.hasDetails ? (
        <details className="harness-disclosure" onToggle={handleToggle}>
          <summary>Details</summary>
          <HarnessDetails state={detailsState} />
        </details>
      ) : null}
    </article>
  );
}
