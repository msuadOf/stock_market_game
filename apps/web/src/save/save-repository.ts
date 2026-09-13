import { parseSaveSlot } from "./save-schema.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";
import { parseSaveJson } from "./save-schema.ts";

interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface SaveRepository {
  save(slot: unknown): void;
  load(): StrictSaveEnvelope | null;
}

export class LocalStorageSaveRepository implements SaveRepository {
  private readonly storage: KeyValueStorage;
  private readonly key: string;

  constructor(storage: KeyValueStorage, key = "stock-game-save") {
    this.storage = storage;
    this.key = key;
  }

  save(slot: unknown): void {
    try {
      this.storage.setItem(this.key, JSON.stringify(parseSaveSlot(slot)));
    } catch (error) {
      throw new Error(`写入浏览器存档失败：${error instanceof Error ? error.message : String(error)}`);
    }
  }

  load(): StrictSaveEnvelope | null {
    let raw: string | null;
    try {
      raw = this.storage.getItem(this.key);
    } catch (error) {
      throw new Error(`读取浏览器存档失败：${error instanceof Error ? error.message : String(error)}`);
    }
    return raw === null ? null : parseSaveJson(raw);
  }
}
