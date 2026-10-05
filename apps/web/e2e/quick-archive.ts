import type { Page } from "@playwright/test";
import { SAVE_DATABASE, SAVE_OBJECT_STORE, QUICK_SAVE_KEY } from "../src/save/save-storage-keys.ts";

export async function readQuickArchive(page: Page): Promise<string | null> {
  // 此函数在浏览器执行；Node 侧 E2E tsconfig 不引入整套 DOM 全局类型。
  return page.evaluate<string | null>(`(([databaseName, storeName, key]) => new Promise((resolve, reject) => {
    const opening = indexedDB.open(databaseName, 1);
    opening.onerror = () => reject(opening.error);
    opening.onsuccess = () => {
      const database = opening.result;
      const transaction = database.transaction(storeName, "readonly");
      const request = transaction.objectStore(storeName).get(key);
      let value = null;
      request.onsuccess = () => {
        if (request.result === undefined) value = null;
        else if (typeof request.result === "string") value = request.result;
        else reject(new Error("快速槽内容不是压缩存档字符串"));
      };
      transaction.oncomplete = () => { database.close(); resolve(value); };
      transaction.onabort = () => { database.close(); reject(transaction.error); };
      transaction.onerror = () => { database.close(); reject(transaction.error); };
    };
  }))(${JSON.stringify([SAVE_DATABASE, SAVE_OBJECT_STORE, QUICK_SAVE_KEY])})`);
}
