import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";

describe("Tauri host startup contract", () => {
  it("awaits a ready Tauri host before App configures speed and reads snapshot", () => {
    const appSource = readFileSync(new URL("../app/useSessionHostLifecycle.ts", import.meta.url), "utf8");
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    const timelineSource = readFileSync(new URL("./tauri-timeline-state.ts", import.meta.url), "utf8");

    assert.match(appSource, /return createTauriHost\(setup, seed\)/);
    assert.match(appSource, /const host = await createHost\(setup, seed, startupTarget\)/);
    assert.match(hostSource, /export async function createTauriHost/);
    assert.match(hostSource, /await invoke<unknown>\("engine_baseline", \{ sessionId, generation: timeline\.currentGeneration\(\) \}\)/);
    assert.match(hostSource, /timeline\.installInitialBaseline\(initialBaseline\)/);
    assert.match(timelineSource, /response\.generation !== this\.generation/);
    assert.match(timelineSource, /this\.installSnapshot\(response\.snapshot, "Tauri engine_baseline.snapshot"\)/);
    assert.match(timelineSource, /parseProtocolSnapshot\(snapshot, where\)/);
  });

  it("uses an explicit JSON-safe protocol for the fastest desktop speed", () => {
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    assert.match(hostSource, /multiplier === Infinity \? "Fastest"/);
  });

  it("uses the same speed metrics contract as the other engine hosts", () => {
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    assert.match(hostSource, /invoke<unknown>\("speed_metrics"/);
    assert.match(hostSource, /return parseSpeedMetrics/);
  });

  it("uses generated report DTOs and generation-tagged query and restore IPC", () => {
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    assert.match(hostSource, /publicCompanyReports: true/);
    assert.match(hostSource, /invoke<unknown>\("public_reports"/);
    assert.match(hostSource, /invoke<unknown>\("public_report_by_id"/);
    assert.match(hostSource, /generation: timeline\.currentGeneration\(\)/);
  });

  it("keeps generation as an exact decimal string and preserves actor restore ordering", () => {
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    const timelineSource = readFileSync(new URL("./tauri-timeline-state.ts", import.meta.url), "utf8");
    const actorSource = readFileSync(
      new URL("../../../desktop/src-tauri/src/actor.rs", import.meta.url),
      "utf8",
    );
    const libSource = readFileSync(
      new URL("../../../desktop/src-tauri/src/lib.rs", import.meta.url),
      "utf8",
    );

    assert.match(timelineSource, /private generation = "1"/);
    assert.match(timelineSource, /BigInt\(value\) \+ 1n/);
    assert.doesNotMatch(hostSource + timelineSource, /Number\((?:currentGeneration|this\.generation)\)/);
    assert.match(libSource, /byte\.is_ascii_digit\(\)/);
    assert.match(actorSource, /let restored = ProtocolSession::restore\(&slot\)\?;/);
    assert.match(actorSource, /self\.game = restored;/);
    assert.match(actorSource, /EngineUpdate::CivilUpdate/);
  });

  it("pauses a host that finishes initialization after the page became hidden", () => {
    const appSource = readFileSync(new URL("../app/useSessionHostLifecycle.ts", import.meta.url), "utf8");
    const startIndex = appSource.indexOf("host.start(");
    const hiddenStopIndex = appSource.indexOf("if (isDocumentHidden()) host.stop();", startIndex);

    assert.notEqual(startIndex, -1);
    assert.notEqual(hiddenStopIndex, -1);
    assert.ok(hiddenStopIndex > startIndex);
    assert.match(appSource, /isDocumentHidden: \(\) => document\.hidden/);
  });
});
