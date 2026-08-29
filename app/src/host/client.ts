import { invoke } from "@tauri-apps/api/core";
import type { HostSnapshot } from "./types";

export function getHostSnapshot(): Promise<HostSnapshot> {
  return invoke<HostSnapshot>("get_host_snapshot");
}
