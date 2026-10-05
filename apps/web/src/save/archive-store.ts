import type { StrictSaveEnvelope } from "./schema/root.ts";
import { parseSaveSlot } from "./save-schema.ts";
import { validateDayEndArchive } from "./day-end-candidate.ts";
import { civilDate } from "./schema/primitives.ts";

export interface ArchiveMetadata {
  slot_id: string;
  name: string;
  civil_date: string;
  tick: number;
}

export interface ArchiveStore {
  list(): Promise<ArchiveMetadata[]>;
  load(slotId?: string): Promise<StrictSaveEnvelope | null>;
  select(slotId: string, isCurrent?: () => boolean): Promise<boolean>;
  rename(slotId: string, name: string): Promise<void>;
  delete(slotId: string): Promise<void>;
  copy(slotId: string, name: string): Promise<ArchiveMetadata>;
}

export interface BrowserArchiveStore extends ArchiveStore {
  save(value: unknown, isCurrent?: () => boolean): Promise<boolean>;
  cancelPending(): void;
  newSlot(): void;
}

export function parseArchiveMetadata(value: unknown): ArchiveMetadata {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("存档元数据结构无效");
  const record = value as Record<string, unknown>;
  if (Object.keys(record).sort().join() !== "civil_date,name,slot_id,tick"
    || typeof record.slot_id !== "string" || record.slot_id.length === 0
    || typeof record.name !== "string" || record.name.trim().length === 0
    || typeof record.civil_date !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(record.civil_date)
    || typeof record.tick !== "number" || !Number.isSafeInteger(record.tick) || record.tick < 0) {
    throw new Error("存档元数据结构无效；不支持旧数据库结构");
  }
  civilDate(record.civil_date, "archive.civil_date");
  return record as unknown as ArchiveMetadata;
}

export function createTransportArchiveStore(request: (operation: "list" | "load" | "select" | "rename" | "delete" | "copy", slotId?: string, name?: string) => Promise<unknown>): ArchiveStore {
  return {
    async list() {
      const result = await request("list");
      if (!Array.isArray(result)) throw new Error("宿主存档列表结构无效");
      const entries = result.map(parseArchiveMetadata);
      if (new Set(entries.map((entry) => entry.slot_id)).size !== entries.length) throw new Error("宿主存档列表含重复槽位");
      return entries;
    },
    async load(slotId) { const result = await request("load", slotId); return result === null ? null : validateDayEndArchive(parseSaveSlot(result)); },
    async select(slotId, isCurrent = () => true) {
      if (!isCurrent()) return false;
      const result = await request("select", slotId);
      if (!isCurrent()) return false;
      if (result !== true) throw new Error("启动槽选择响应结构无效");
      return true;
    },
    async rename(slotId, name) { const result = await request("rename", slotId, name); if (result !== null) throw new Error("存档重命名响应结构无效"); },
    async delete(slotId) { const result = await request("delete", slotId); if (result !== null) throw new Error("存档删除响应结构无效"); },
    async copy(slotId, name) { return parseArchiveMetadata(await request("copy", slotId, name)); },
  };
}
