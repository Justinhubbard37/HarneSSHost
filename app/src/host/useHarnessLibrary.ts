import { useCallback, useEffect, useState } from "react";
import { getHarnessLibrary } from "./client";
import type { HarnessLibraryPayload } from "./types";

export type HarnessLibraryState =
  | { status: "loading" }
  | { status: "ready"; library: HarnessLibraryPayload }
  | { status: "error" };

export function useHarnessLibrary() {
  const [state, setState] = useState<HarnessLibraryState>({ status: "loading" });

  const refresh = useCallback(async () => {
    setState({ status: "loading" });
    try {
      const library = await getHarnessLibrary();
      setState({ status: "ready", library });
    } catch {
      setState({ status: "error" });
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { state, refresh };
}
