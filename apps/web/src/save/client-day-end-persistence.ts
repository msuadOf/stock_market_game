import type { DayEndExportSubscription, EngineHost } from "../host/engine-host.ts";
import type { StartupTarget } from "../host/startup-policy.ts";
import type { CivilUpdate } from "../types/generated/CivilUpdate.ts";
import { captureCivilDayEndCandidate } from "./civil-day-end-capture.ts";
import { validateDayEndCandidate } from "./day-end-candidate.ts";
import type { DayEndPersistence } from "./day-end-persistence.ts";
import { clientDayEndTargets, writeDayEndTargets, type DayEndTarget } from "./day-end-targets.ts";

export interface ClientCivilDayEndPorts {
  hostKind: StartupTarget["kind"];
  generation: string;
  update: Pick<CivilUpdate, "seq_to" | "boundary">;
  persistence: DayEndPersistence;
  save: EngineHost["save"];
  browserQuickSlot(): DayEndTarget;
  authorizedFile: DayEndTarget | null;
  exportSubscription?: DayEndExportSubscription;
  isCurrent(): boolean;
  isFileTargetCurrent?(): boolean;
  refreshPlayerOrders(): void;
}

/** CivilUpdate 只在存在客户端目标时提取候选；返回实际成功目标，过期或无目标返回 null。 */
export async function persistClientCivilDayEnd(ports: ClientCivilDayEndPorts): Promise<readonly string[] | null> {
  if (!ports.isCurrent()) return null;
  const fileIsCurrent = () => ports.isFileTargetCurrent?.() !== false;
  const file = ports.authorizedFile;
  const targets = clientDayEndTargets(ports.hostKind, ports.browserQuickSlot, file === null ? null : {
    label: file.label,
    write: (slot, isCurrent) => fileIsCurrent() ? file.write(slot, () => isCurrent() && fileIsCurrent()) : Promise.resolve(false),
  });
  if (targets.length === 0) { ports.refreshPlayerOrders(); return null; }
  const reference = { seq: ports.update.seq_to, settledDate: ports.update.boundary.settled_date };
  const subscription = ports.hostKind === "remote" ? ports.exportSubscription : undefined;
  if (ports.hostKind === "remote" && subscription === undefined) throw new Error("远程日终文件未订阅候选导出，请重新授权文件");
  if (subscription !== undefined && reference.seq <= subscription.afterSeq) {
    ports.refreshPlayerOrders();
    return null;
  }
  const committed = new Set<string>();
  const trackedTargets = targets.map(target => ({
    label: target.label,
    write: async (slot: unknown, isCurrent: () => boolean) => {
      const result = await target.write(slot, isCurrent);
      if (result !== false && isCurrent()) committed.add(target.label);
      return result;
    },
  }));
  const write = async (slot: unknown, queueIsCurrent: () => boolean) => {
    const isCurrent = () => queueIsCurrent() && ports.isCurrent();
    if (!isCurrent()) return false;
    return await writeDayEndTargets(slot, isCurrent, trackedTargets);
  };
  const capture = () => captureCivilDayEndCandidate(
    (subscription === undefined ? ports.save(reference) : subscription.read(reference)).then(slot => validateDayEndCandidate(slot, reference)),
    ports.isCurrent,
    ports.refreshPlayerOrders,
  );
  const saved = subscription === undefined
    ? await ports.persistence.completed(ports.generation, capture(), write)
    : await ports.persistence.completedDeferred(ports.generation, capture, write,
      () => subscription.acknowledge(reference), () => ports.isCurrent() && fileIsCurrent());
  return saved && ports.isCurrent() ? targets.filter(target => committed.has(target.label)).map(target => target.label) : null;
}
