import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";

describe("Tauri host startup contract", () => {
  it("awaits a ready Tauri host before App configures speed and reads snapshot", () => {
    const appSource = readFileSync(new URL("../app/useSessionHostLifecycle.ts", import.meta.url), "utf8");
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    const timelineSource = readFileSync(new URL("./tauri-timeline-state.ts", import.meta.url), "utf8");

    // 启动槽恢复参数已进入同一异步创建链；仍须 await 后才配置宿主，不删顺序门禁。
    assert.match(appSource, /return createTauriHost\(setup, seed, \{ resumeArchive \}\)/);
    assert.match(appSource, /const host = await createHost\(setup, seed, startupTarget, resumeArchive\)/);
    assert.match(hostSource, /export async function createTauriHost/);
    assert.match(hostSource, /await invoke<unknown>\("engine_baseline", \{ sessionId, generation: timeline\.currentGeneration\(\) \}\)/);
    assert.match(hostSource, /timeline\.installInitialBaseline\(initialBaseline\)/);
    assert.match(timelineSource, /response\.generation !== this\.generation/);
    assert.match(timelineSource, /this\.installSnapshot\(response\.snapshot, response\.civil_date, "Tauri engine_baseline"\)/);
    assert.match(timelineSource, /parseProtocolSnapshot\(snapshot, `\$\{where\}\.snapshot`\)/);
    assert.match(timelineSource, /parseIsoDate\(civilDate, `\$\{where\}\.civil_date`\)/);
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

  it("bridges the company contract queries with the shared web parsers", () => {
    // 任务 178：F 批四查询面经 Tauri 命令暴露，且解析复用 Web 同源 parser。
    const hostSource = readFileSync(new URL("./tauri-host.ts", import.meta.url), "utf8");
    const libSource = readFileSync(
      new URL("../../../desktop/src-tauri/src/lib.rs", import.meta.url),
      "utf8",
    );
    assert.match(hostSource, /invoke<unknown>\("company_capabilities"/);
    assert.match(hostSource, /invoke<unknown>\("company_period_explanation"/);
    assert.match(hostSource, /invoke<unknown>\("owner_rights_offerings"/);
    assert.match(hostSource, /invoke<unknown>\("owner_flat_withholding_receipts"/);
    assert.match(hostSource, /parseCompanyCapabilities\(response\.value\)/);
    assert.match(hostSource, /parsePeriodChangeExplanation\(response\.value\)/);
    assert.match(hostSource, /parseOwnerRightsOfferings\(response\.value\)/);
    assert.match(hostSource, /parseFlatWithholdingReceipts\(response\.value\)/);
    // 两份 generate_handler（默认与 host-parity）都必须注册四个命令。
    const registrations = libSource.match(/^\s{12}(company_capabilities|company_period_explanation|owner_rights_offerings|owner_flat_withholding_receipts),$/gm) ?? [];
    assert.equal(registrations.length, 8, "每个命令必须在两份 handler 列表各注册一次");
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
    // 后台暂停仅适用于本地宿主；远程页面隐藏不能暂停共享市场，Tauri 仍必须暂停。
    const hiddenStopIndex = appSource.indexOf("if (host.capabilities.persistence !== \"remote\" && isDocumentHidden()) await host.stop();", startIndex);

    assert.notEqual(startIndex, -1);
    assert.notEqual(hiddenStopIndex, -1);
    assert.ok(hiddenStopIndex > startIndex);
    assert.match(appSource, /isDocumentHidden: \(\) => document\.hidden/);
  });
});
