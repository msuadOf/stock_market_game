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

  it("uses the same speed metrics contract as the other engine hosts", () => {
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    assert.match(hostSource, /invoke<unknown>\("speed_metrics"/);
    assert.match(hostSource, /return parseSpeedMetrics/);
  });

  it("uses generated report DTOs and generation-tagged query and restore IPC", () => {
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    assert.match(hostSource, /publicCompanyReports: true/);
    assert.match(hostSource, /invoke<GenerationResponse<PublicReportPage>>\("public_reports"/);
    assert.match(hostSource, /invoke<GenerationResponse<PublicReportSummary>>\(/);
    assert.match(hostSource, /generation: requestGeneration/);
    assert.match(hostSource, /invoke<GenerationResponse<string>>\("civil_date"/);
    assert.match(hostSource, /response\.generation !== requestedGeneration/);
  });

  it("keeps generation as an exact decimal string and preserves actor restore ordering", () => {
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    const actorSource = readFileSync(
      new URL("../../../desktop/src-tauri/src/actor.rs", import.meta.url),
      "utf8",
    );
    const libSource = readFileSync(
      new URL("../../../desktop/src-tauri/src/lib.rs", import.meta.url),
      "utf8",
    );

    assert.match(hostSource, /let generation = "1"/);
    assert.match(hostSource, /BigInt\(requestGeneration\) \+ 1n/);
    assert.doesNotMatch(hostSource, /Number\(requestGeneration\)/);
    assert.match(libSource, /byte\.is_ascii_digit\(\)/);
    assert.match(actorSource, /let restored = GameSession::restore\(&slot\)\?;/);
    assert.match(actorSource, /self\.game = restored;/);
    assert.match(actorSource, /civil_events\.extend\(report\.events\);/);
    assert.match(actorSource, /events\.extend\(civil_events\);/);
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
