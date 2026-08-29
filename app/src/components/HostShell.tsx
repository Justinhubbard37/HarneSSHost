import { HarnessLibrary } from "./HarnessLibrary";
import type { HarnessLibraryState } from "../host/useHarnessLibrary";

interface HostShellProps {
  libraryState: {
    state: HarnessLibraryState;
    refresh: () => Promise<void>;
  };
}

export function HostShell({ libraryState }: HostShellProps) {
  const { state, refresh } = libraryState;

  return (
    <main className="host-shell">
      <header className="library-header">
        <h1 className="product-name">HarneSSHost</h1>
        <h2 className="library-title">Harness Library</h2>
        <p className="library-introduction">
          Harnesses supported by this version, with technical information available when needed.
        </p>
      </header>

      {state.status === "loading" ? (
        <section className="library-state" aria-live="polite">
          <h2>Loading the library</h2>
          <p>Checking the supported catalog and local installation state.</p>
        </section>
      ) : null}

      {state.status === "error" ? (
        <section className="library-state" role="alert">
          <h2>Harness Library unavailable</h2>
          <p>HarneSSHost could not retrieve the supported harness catalog.</p>
          <button type="button" onClick={() => void refresh()}>
            Try again
          </button>
        </section>
      ) : null}

      {state.status === "ready" ? <HarnessLibrary harnesses={state.library.harnesses} /> : null}
    </main>
  );
}
