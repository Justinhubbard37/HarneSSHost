import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getHarnessLibrary, openHarness } from "./client";
import type { HarnessLibraryPayload, RuntimeChangedEvent } from "./types";

export type HarnessLibraryState =
  | { status: "loading" }
  | { status: "ready"; library: HarnessLibraryPayload }
  | { status: "error" };

export function useHarnessLibrary() {
  const [state, setState] = useState<HarnessLibraryState>({ status: "loading" });

  const refresh = useCallback(async (showLoading = true) => {
    if (showLoading) {
      setState({ status: "loading" });
    }
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

  useEffect(() => {
    let disposed = false;
    let removeListener: (() => void) | undefined;

    void listen<RuntimeChangedEvent>("harness-runtime-changed", () => {
      void refresh(false);
    })
      .then((unlisten) => {
        if (disposed) {
          unlisten();
        } else {
          removeListener = unlisten;
        }
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      removeListener?.();
    };
  }, [refresh]);

  const hasTransientRuntime =
    state.status === "ready" &&
    state.library.harnesses.some(
      (harness) => harness.state === "starting" || harness.state === "stopping",
    );

  useEffect(() => {
    if (!hasTransientRuntime) {
      return;
    }
    const interval = window.setInterval(() => {
      void refresh(false);
    }, 750);
    return () => window.clearInterval(interval);
  }, [hasTransientRuntime, refresh]);

  const open = useCallback(
    async (harnessId: string) => {
      await openHarness(harnessId);
      await refresh(false);
    },
    [refresh],
  );

  return { state, refresh, open };
}
