import { invoke } from "@tauri-apps/api/core";
import type { HarnessDetailsPayload, HarnessLibraryPayload } from "./types";

export function getHarnessLibrary(): Promise<HarnessLibraryPayload> {
  return invoke<HarnessLibraryPayload>("get_harness_library");
}

export function getHarnessDetails(harnessId: string): Promise<HarnessDetailsPayload> {
  return invoke<HarnessDetailsPayload>("get_harness_details", { harnessId });
}
