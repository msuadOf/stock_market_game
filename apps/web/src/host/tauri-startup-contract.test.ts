import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";

describe("Tauri host startup contract", () => {
  it("awaits a ready Tauri host before App configures speed and reads snapshot", () => {
    const appSource = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");

    assert.match(appSource, /host = await createTauriHost\(/);
    assert.match(hostSource, /export async function createTauriHost/);
    assert.match(hostSource, /await invoke<Snapshot>\("snapshot"/);
  });

  it("uses an explicit JSON-safe protocol for the fastest desktop speed", () => {
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    assert.match(hostSource, /x === Infinity \? "Fastest"/);
  });

  it("pauses a host that finishes initialization after the page became hidden", () => {
    const appSource = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
    const startIndex = appSource.indexOf("host.start(");
    const hiddenStopIndex = appSource.indexOf("if (document.hidden) host.stop();", startIndex);

    assert.notEqual(startIndex, -1);
    assert.notEqual(hiddenStopIndex, -1);
    assert.ok(hiddenStopIndex > startIndex);
  });
});
