import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";

describe("Tauri host startup contract", () => {
  it("awaits a ready Tauri host before App configures speed and reads snapshot", () => {
    const appSource = readFileSync(new URL("../app/useSessionHostLifecycle.ts", import.meta.url), "utf8");
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    const timelineSource = readFileSync(new URL("./tauri-timeline-state.ts", import.meta.url), "utf8");
    const updateSource = readFileSync(new URL("./host-update.ts", import.meta.url), "utf8");

    assert.match(appSource, /return createTauriHost\(setup, seed\)/);
    assert.match(appSource, /const onSpawnFailure = \(failure: HostFailure\) => \{ if \(!cancelled\) ports\.onSpawnFailure\(failure\); \};/);
    assert.match(appSource, /if \(memoryTicket === undefined\) host = await createHost\(setup, seed, startupTarget, onSpawnFailure, initialSlot, undefined, onInitializationProgress\);/);
    assert.match(appSource, /else host = await createHost\(setup, seed, startupTarget, onSpawnFailure, initialSlot, memoryTicket, onInitializationProgress\);/);
    assert.match(appSource, /if \(startupTarget\.kind === "tauri"\) return createTauriHost\(setup, seed\);/);
    assert.match(appSource, /if \(startupTarget\.kind === "remote"\) return createRemoteHost\(setup, seed, \{ baseUrl: startupTarget\.baseUrl \}\);/);
    assert.match(appSource, /const options = \{ enableE2EStepping: tradingE2EMode, onSpawnFailure, initialArchive, memoryAdmissionTicket, onInitializationProgress, \.\.\.wasmRuntimeOptions \};/);
    const hostReadyIndex = appSource.indexOf("host = await createHost(setup, seed, startupTarget, onSpawnFailure, initialSlot, undefined, onInitializationProgress);");
    const ticketHostReadyIndex = appSource.indexOf("host = await createHost(setup, seed, startupTarget, onSpawnFailure, initialSlot, memoryTicket, onInitializationProgress);");
    const conditionalLoadIndex = appSource.indexOf("if (initialSlot !== null && startupTarget.kind !== \"wasm\") await host.load(initialSlot);");
    const appWiringIndex = appSource.indexOf("hostRef.current = host;", hostReadyIndex);
    assert.notEqual(hostReadyIndex, -1);
    assert.notEqual(ticketHostReadyIndex, -1);
    assert.notEqual(conditionalLoadIndex, -1);
    assert.notEqual(appWiringIndex, -1);
    assert.ok(conditionalLoadIndex > hostReadyIndex);
    assert.ok(conditionalLoadIndex > ticketHostReadyIndex);
    assert.ok(conditionalLoadIndex < appWiringIndex);
    assert.match(hostSource, /export async function createTauriHost/);
    assert.match(hostSource, /await invokeBaseline\("engine_baseline", \{ sessionId, generation: timeline\.currentGeneration\(\) \}\)/);
    assert.match(hostSource, /timeline\.installInitialBaseline\(initialBaseline\)/);
    assert.match(timelineSource, /response\.generation !== this\.generation/);
    assert.match(timelineSource, /return this\.installSnapshot\(response, "Tauri engine_baseline"\)/);
    assert.match(timelineSource, /prepareBaselineUpdate\(response\.generation, response\.snapshot, response\.stock_catalog, response\.company_catalog, \{\s*civilDate: parseIsoDate\(response\.civil_date, `\$\{where\}\.civil_date`\), revision: null,\s*\}\)/);
    assert.match(updateSource, /parseProtocolSnapshot\(snapshot, "baseline\.snapshot", resolver\)/);
    assert.match(timelineSource, /parseIsoDate\(response\.civil_date, `\$\{where\}\.civil_date`\)/);
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
    const startIndex = appSource.indexOf("await host.start(");
    const hiddenStopIndex = appSource.indexOf("if (isDocumentHidden()) await host.stop();", startIndex);

    assert.notEqual(startIndex, -1);
    assert.notEqual(hiddenStopIndex, -1);
    assert.ok(hiddenStopIndex > startIndex);
    assert.match(appSource, /isDocumentHidden: \(\) => document\.hidden/);
  });
});
