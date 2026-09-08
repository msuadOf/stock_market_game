import type { SaveSlot } from "../types/engine";
import { parseSaveJson } from "./save-schema.ts";

interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface SaveRepository {
  save(slot: SaveSlot): void;
  load(): SaveSlot | null;
}

export class LocalStorageSaveRepository implements SaveRepository {
  private readonly storage: KeyValueStorage;
  private readonly key: string;

  constructor(storage: KeyValueStorage, key = "stock-game-save") {
    this.storage = storage;
    this.key = key;
  }

  save(slot: SaveSlot): void {
    try {
      this.storage.setItem(this.key, JSON.stringify(slot));
    } catch (error) {
      throw new Error(`写入浏览器存档失败：${error instanceof Error ? error.message : String(error)}`);
    }
  }

  load(): SaveSlot | null {
    let raw: string | null;
    try {
      raw = this.storage.getItem(this.key);
    } catch (error) {
      throw new Error(`读取浏览器存档失败：${error instanceof Error ? error.message : String(error)}`);
    }
    return raw === null ? null : parseSaveJson(raw);
  }
}
