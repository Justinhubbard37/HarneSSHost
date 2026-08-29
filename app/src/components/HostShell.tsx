import { HarnessStatusRegion } from "./HarnessStatusRegion";
import { InterfaceViewport } from "./InterfaceViewport";
import type { HostSnapshotState } from "../host/useHostSnapshot";

interface HostShellProps {
  hostState: {
    state: HostSnapshotState;
    refresh: () => Promise<void>;
  };
}

export function HostShell({ hostState }: HostShellProps) {
  const { state, refresh } = hostState;

  return (
    <main className="host-shell">
      <header className="host-header">
        <div>
          <p className="eyebrow">Harness evaluation host</p>
          <h1 className="product-name">HarneSSHost</h1>
          <p className="host-status">No harness active</p>
        </div>
        <button type="button" onClick={() => void refresh()}>
          Check again
        </button>
      </header>

      {state.status === "loading" ? (
        <section className="panel" aria-live="polite">
          <p className="section-label">Harness status</p>
          <h2>Checking local harnesses</h2>
          <p className="muted">Reading configured development candidates.</p>
        </section>
      ) : null}

      {state.status === "error" ? (
        <section className="panel" role="alert">
          <p className="section-label">Host error</p>
          <h2 className="error-text">Harness inspection unavailable</h2>
          <p className="muted">HarneSSHost could not retrieve a sanitized harness snapshot.</p>
        </section>
      ) : null}

      {state.status === "ready" ? (
        <div className="host-grid">
          <div className="stack">
            <HarnessStatusRegion harnesses={state.snapshot.harnesses} />
            <InterfaceViewport state={state.snapshot.interfaceState} />
          </div>

          <aside className="panel" aria-labelledby="runtime-heading">
            <p className="section-label">Runtime</p>
            <h2 id="runtime-heading">Inactive</h2>
            <p className="runtime-state">
              Runtime controls are unavailable in this foundation. HarneSSHost has not launched a
              harness process.
            </p>
          </aside>
        </div>
      ) : null}
    </main>
  );
}
