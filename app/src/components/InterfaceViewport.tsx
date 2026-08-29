import type { HostSurfaceState } from "../host/types";

interface InterfaceViewportProps {
  state: HostSurfaceState;
}

export function InterfaceViewport({ state }: InterfaceViewportProps) {
  const title =
    state.kind === "loading"
      ? "Interface loading"
      : state.kind === "unavailable"
        ? "Interface unavailable"
        : state.kind === "error"
          ? "Interface error"
          : "No harness interface active";

  return (
    <section className="panel interface-viewport" aria-labelledby="interface-heading">
      <p className="section-label">Interface viewport</p>
      <h2 id="interface-heading">{title}</h2>
      <p className="muted">
        {state.message ??
          "HarneSSHost remains in its branded host state until a harness runtime and interface are explicitly available."}
      </p>
    </section>
  );
}
