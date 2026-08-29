import { HarnessCard } from "./HarnessCard";
import type { HarnessCardSummary } from "../host/types";

interface HarnessLibraryProps {
  harnesses: HarnessCardSummary[];
}

export function HarnessLibrary({ harnesses }: HarnessLibraryProps) {
  if (harnesses.length === 0) {
    return (
      <section className="library-state">
        <h2>No supported harnesses</h2>
        <p>This HarneSSHost version does not expose a supported harness catalog.</p>
      </section>
    );
  }

  return (
    <section className="harness-library" aria-label="Supported harnesses">
      {harnesses.map((harness) => (
        <HarnessCard harness={harness} key={harness.id} />
      ))}
    </section>
  );
}
