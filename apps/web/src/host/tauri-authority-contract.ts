//! 真实Tauri窗口的验证桥；普通恢复、查询、接纳和重同步始终经过实际tauri-host。
import { invoke } from "@tauri-apps/api/core";
import { createTauriHost, tauriIdentityForContract } from "./tauri-host.ts";
import { requireTauriContractBackend, requireTauriContractEnvironment } from "./tauri-authority-contract-gate.ts";
import type { SessionSetup } from "../types/engine.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";

type Row = { id: string; generation: string; lifecycle?: { commandClosed: boolean; archiveLaneReleased: boolean }; evidence?: unknown; [key: string]: unknown };
function row(value: unknown, id: string): Row {
  if (value === null || typeof value !== "object" || (value as Row).id !== id || typeof (value as Row).generation !== "string") throw new Error("实际Desktop合同响应缺少匹配动作/代际");
  return value as Row;
}
async function capabilities() {
  const value = await invoke<unknown>("authority_contract_setup");
  requireTauriContractBackend(value);
  return value;
}
async function create(setup: SessionSetup, seed: string) {
  requireTauriContractEnvironment(import.meta.env);
  await capabilities();
  if (!/^(0|[1-9]\d*)$/.test(seed) || BigInt(seed) > 18_446_744_073_709_551_615n) throw new Error("合同seed必须为规范u64整数串");
  const host = await createTauriHost(setup, BigInt(seed));
  const identity = tauriIdentityForContract(host);
  const events: { event: string; value?: unknown }[] = [];
  let closedRow: Row | null = null;
  const snapshot = async (id: string) => row(await invoke("authority_contract_snapshot", { sessionId: identity.sessionId, generation: identity.generation(), id }), id);
  return {
    host,
    sessionId: identity.sessionId,
    generation: () => identity.generation(),
    observerLease: () => identity.observerLease(),
    staleDeliveries: () => identity.staleDeliveries(),
    async holdObserver() { await identity.acknowledgementsSettled(); await identity.holdObserver(); },
    async subscribe(onUpdate: (update: HostUpdate) => void | boolean, onFailure?: (failure: HostFailure) => void, paused = false) {
      const subscribe = paused ? identity.subscribePaused : host.start;
      await subscribe(update => {
        const accepted = onUpdate(update);
        events.push({ event: accepted === false ? "host-rejected" : "host-accepted", value: update });
        return accepted;
      }, failure => { events.push({ event: "host-failure", value: failure }); onFailure?.(failure); });
    },
    async load(slot: unknown, onRestored?: () => void) {
      await host.load(slot, () => { events.push({ event: "host-restore-committed", value: identity.generation() }); onRestored?.(); });
    },
    async refreshBaseline() { await host.refreshBaseline(); await identity.acknowledgementsSettled(); },
    async disconnect() {
      await identity.acknowledgementsSettled();
      identity.disconnectObserver();
      events.push({ event: "actual-host-listeners-removed" });
    },
    async reconnect() {
      await identity.reconnectObserver();
      await host.refreshBaseline();
      await identity.acknowledgementsSettled();
      events.push({ event: "actual-host-listeners-rebound-and-resynced" });
    },
    async inject(id: string, checkpoint?: unknown) {
      if (id === "replaced" || id === "prepare-rejected" || id === "restore-postcommit-io") throw new Error("普通恢复及其拒绝必须走实际host.load，不能用raw IPC绕过frontend策略");
      const value = row(await invoke("authority_contract_step", { sessionId: identity.sessionId, generation: identity.generation(), id, ...(checkpoint === undefined ? {} : { checkpoint }) }), id);
      if (value.lifecycle?.commandClosed) closedRow = value;
      return value;
    },
    async restoreFromCurrent(id: "replaced" | "prepare-rejected") {
      const slot = await host.save();
      if (id === "prepare-rejected") {
        const invalid = structuredClone(slot) as { setup: { ticks_per_day: number } };
        invalid.setup.ticks_per_day = 0;
        let rejected = false;
        try { await host.load(invalid); } catch (error) { rejected = true; events.push({ event: "host-prepare-rejected", value: String(error) }); }
        if (!rejected) throw new Error("实际host.load没有拒绝无效候选");
      } else await host.load(slot, () => events.push({ event: "host-restore-committed", value: identity.generation() }));
      return snapshot(id);
    },
    async postcommitBaselineFailure() {
      const slot = await host.save();
      await invoke("authority_contract_step", { sessionId: identity.sessionId, generation: identity.generation(), id: "arm-restore-postcommit-baseline-failure" });
      const previous = identity.generation();
      let rejected = false;
      try { await host.load(slot); } catch (error) { rejected = true; events.push({ event: "host-postcommit-baseline-rejected", value: String(error) }); }
      if (!rejected || identity.generation() === previous) throw new Error("实际host没有保留已提交恢复的故障代际");
      return snapshot("restore-postcommit-io");
    },
    snapshot,
    acknowledgementsSettled: () => identity.acknowledgementsSettled(),
    evidence: () => structuredClone(events),
    async close() {
      // 真实host.dispose执行取消监听和stop_session；后端仅明确完成的关闭可幂等。
      // 已知归档失败仍原样拒绝，调用者必须记为预期非耐久清理证据，不能吞成成功。
      try { await host.dispose(); }
      catch (error) {
        events.push({ event: "host-dispose-error", value: { message: String(error), confirmedShutdown: closedRow } });
        const shutdown = closedRow?.evidence as { shutdown?: { kind?: string; message?: string; ipcMessage?: string } } | undefined;
        if (closedRow?.lifecycle?.commandClosed && closedRow.lifecycle.archiveLaneReleased && shutdown?.shutdown?.kind === "known-archive-failure" && (typeof error === "string" ? error : error instanceof Error ? error.message : null) === shutdown.shutdown.ipcMessage) {
          throw Object.assign(new Error("已确认关闭保留非耐久归档失败", { cause: error }), { kind: "confirmed-nondurable-shutdown", receipt: closedRow });
        }
        throw error;
      }
    },
  };
}
export type TauriAuthorityContractOwner = Awaited<ReturnType<typeof create>>;
export async function installTauriAuthorityContractBridge(): Promise<void> {
  requireTauriContractEnvironment(import.meta.env);
  if (!("__TAURI_INTERNALS__" in globalThis)) throw new Error("合同桥只能安装到实际Tauri窗口");
  await capabilities();
  if (Object.hasOwn(globalThis, "__authorityTauriContract")) throw new Error("实际Tauri合同桥已经安装");
  Object.defineProperty(globalThis, "__authorityTauriContract", { value: Object.freeze({ create, capabilities }), writable: false, configurable: false });
}
