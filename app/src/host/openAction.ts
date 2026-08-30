import type { HarnessCardState } from "./types";

export interface OpenActionState {
  visible: boolean;
  disabled: boolean;
  label: "Open" | "Opening..." | "Closing...";
}

export function deriveOpenAction(
  state: HarnessCardState,
  canOpen: boolean,
  requestPending: boolean,
): OpenActionState {
  if (state === "starting" || requestPending) {
    return { visible: true, disabled: true, label: "Opening..." };
  }
  if (state === "stopping") {
    return { visible: true, disabled: true, label: "Closing..." };
  }
  if (state === "ready" || state === "open" || state === "failed") {
    return { visible: true, disabled: !canOpen, label: "Open" };
  }
  return { visible: false, disabled: true, label: "Open" };
}
