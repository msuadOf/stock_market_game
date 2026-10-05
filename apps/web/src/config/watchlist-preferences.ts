export const WATCHLIST_STORAGE_KEY = "stock-game-watchlist";

interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function validateCodes(value: unknown): string[] {
  if (!Array.isArray(value) || value.some(code => typeof code !== "string" || !/^[a-zA-Z0-9]+$/.test(code))) throw new Error("自选必须是由非空证券代码组成的数组");
  if (new Set(value).size !== value.length) throw new Error("自选代码不能重复");
  return [...value];
}

/** 自选为浏览器展示偏好，独立于游戏存档；存储失败必须保留上次有效名单。 */
export class WatchlistPreferences {
  private storage: () => KeyValueStorage;
  constructor(storage: () => KeyValueStorage) { this.storage = storage; }

  load(): string[] {
    try {
      const raw = this.storage().getItem(WATCHLIST_STORAGE_KEY);
      return raw === null ? [] : validateCodes(JSON.parse(raw));
    } catch (error) {
      throw new Error(`读取自选失败：${error instanceof Error ? error.message : String(error)}`);
    }
  }

  save(codes: readonly string[]): void {
    try {
      this.storage().setItem(WATCHLIST_STORAGE_KEY, JSON.stringify(validateCodes(codes)));
    } catch (error) {
      throw new Error(`保存自选失败：${error instanceof Error ? error.message : String(error)}`);
    }
  }
}
