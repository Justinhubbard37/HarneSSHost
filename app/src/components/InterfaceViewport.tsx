import type { HostSurfaceState } from "../host/types";

interface InterfaceViewportProps {
  state: HostSurfaceState;
}

export function InterfaceViewport({ state }: InterfaceViewportProps) {
  if (state.kind === "noHarnessActive") {
    return null;
  }

  const title =
    state.kind === "loading"
      ? "Interface opening"
      : state.kind === "officialInterfaceAvailable"
        ? "Official interface open"
      : state.kind === "unavailable"
        ? "Interface unavailable"
        : state.kind === "error"
          ? "Interface error"
          : "No harness interface active";

  return (
    <section
      className={`interface-status interface-status-${state.kind}`}
      aria-labelledby="interface-heading"
      aria-live="polite"
    >
      <h2 id="interface-heading">{title}</h2>
      <p>
        {state.message ??
          "HarneSSHost is coordinating the official harness interface."}
      </p>
    </section>
  );
}
