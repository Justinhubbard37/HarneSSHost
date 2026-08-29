import { useCallback, useEffect, useState } from "react";
import { getHostSnapshot } from "./client";
import type { HostSnapshot } from "./types";

export type HostSnapshotState =
  | { status: "loading" }
  | { status: "ready"; snapshot: HostSnapshot }
  | { status: "error" };

export function useHostSnapshot() {
  const [state, setState] = useState<HostSnapshotState>({ status: "loading" });

  const refresh = useCallback(async () => {
    setState({ status: "loading" });
    try {
      const snapshot = await getHostSnapshot();
      setState({ status: "ready", snapshot });
    } catch {
      setState({ status: "error" });
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { state, refresh };
}
