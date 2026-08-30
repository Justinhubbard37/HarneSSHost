import assert from "node:assert/strict";
import test from "node:test";
import { deriveOpenAction } from "../src/host/openAction.ts";

test("Open is visible only for verified launchable DeepSeek states", () => {
  assert.deepEqual(deriveOpenAction("ready", true, false), {
    visible: true,
    disabled: false,
    label: "Open",
  });

  for (const state of ["notInstalled", "problemDetected", "unsupportedLocalInstallation"]) {
    assert.equal(deriveOpenAction(state, false, false).visible, false);
  }
});

test("Starting and a pending click prevent duplicate action", () => {
  assert.deepEqual(deriveOpenAction("starting", false, false), {
    visible: true,
    disabled: true,
    label: "Opening...",
  });
  assert.equal(deriveOpenAction("ready", true, true).disabled, true);
});

test("Ready runtime keeps Open available for focus", () => {
  assert.deepEqual(deriveOpenAction("open", true, false), {
    visible: true,
    disabled: false,
    label: "Open",
  });
});

test("Failed runtime retries only when cleanup makes Open available", () => {
  assert.equal(deriveOpenAction("failed", false, false).disabled, true);
  assert.equal(deriveOpenAction("failed", true, false).disabled, false);
});

test("Stopping remains non-duplicating", () => {
  assert.deepEqual(deriveOpenAction("stopping", false, false), {
    visible: true,
    disabled: true,
    label: "Closing...",
  });
});
