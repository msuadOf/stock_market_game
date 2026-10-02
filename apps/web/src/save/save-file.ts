/**
 * 文件存档：选择可复用的日终输出目标，以及读取存档文件。
 *
 * 三种环境自适应：
 * 1. **Tauri 桌面端**（`window.__TAURI_INTERNALS__` 存在）：
 *    原生对话框选择路径；日终经同目录临时文件与 rename 原子替换。
 * 2. **现代浏览器**（File System Access API 可用）：`showSaveFilePicker` / `showOpenFilePicker`。
 * 3. **不支持 FS Access 的浏览器**：不支持文件持续覆盖；仅保留上传读档。
 *
 * 防御式（铁律二）：任何环节失败都抛出可读错误（带上下文），绝不静默吞；
 * 调用方负责把错误展示给用户。
 */

import type { StrictSaveEnvelope } from "./schema/root.ts";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { open as openFile, readTextFile, writeTextFile, rename, remove } from "@tauri-apps/plugin-fs";
import { parseSaveJson, parseSaveSlot } from "./save-schema";

/** 存档文件扩展名与 MIME。 */
const SAVE_EXT = "json";
const SAVE_MIME = "application/json";
/** 文件名里的日期戳格式（例：2026-06-30）。 */
function dateStamp(d = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}
function defaultFileName(): string {
  return `stock-game-save-${dateStamp()}.${SAVE_EXT}`;
}

// ---------------------------------------------------------------------------
// Tauri 分支
// ---------------------------------------------------------------------------
/** 是否运行在 Tauri 桌面壳内。 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export interface DayEndFileTarget {
  write(slot: unknown, isCurrent?: () => boolean): Promise<void>;
}

function assertCurrent(isCurrent?: () => boolean): void {
  if (isCurrent !== undefined && !isCurrent()) {
    throw new Error("旧局 generation 的日终文件写入已拒绝");
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function selectViaTauri(): Promise<DayEndFileTarget | null> {
  const path = await saveDialog({
    defaultPath: defaultFileName(),
    filters: [{ name: "股票存档", extensions: [SAVE_EXT] }],
  });
  if (path === null) return null;
  if (typeof path !== "string" || path.trim().length === 0) throw new Error("未选择有效的日终存档文件路径");
  return {
    async write(slot, isCurrent) {
      assertCurrent(isCurrent);
      const json = JSON.stringify(parseSaveSlot(slot));
      if (typeof globalThis.crypto?.randomUUID !== "function") {
        throw new Error("Tauri 日终文件写入失败：无法生成独占临时文件名，旧档未修改");
      }
      const directoryEnd = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1;
      const temporaryPath = `${path.slice(0, directoryEnd)}stock-game-day-end-${globalThis.crypto.randomUUID()}.tmp`;
      let owned = false;
      let resource: Awaited<ReturnType<typeof openFile>> | null = null;
      try {
        resource = await openFile(temporaryPath, { write: true, createNew: true });
        owned = true;
        assertCurrent(isCurrent);
        await resource.close();
        resource = null;
        assertCurrent(isCurrent);
        await writeTextFile(temporaryPath, json, { create: false });
        assertCurrent(isCurrent);
        await rename(temporaryPath, path);
      } catch (error) {
        const errors: unknown[] = [error];
        if (resource !== null) {
          try {
            await resource.close();
          } catch (closeError) {
            errors.push(closeError);
          }
        }
        if (owned) {
          try {
            await remove(temporaryPath);
          } catch (cleanupError) {
            errors.push(cleanupError);
          }
        }
        if (errors.length > 1) {
          throw new AggregateError(errors, `Tauri 日终文件写入及临时文件清理失败：${errors.map(errorMessage).join("；")}`, { cause: error });
        }
        throw new Error(`Tauri 日终文件写入失败：${errorMessage(error)}。请核对 fs open/write-text-file/rename/remove 权限及同目录临时文件授权范围。`, { cause: error });
      }
    },
  };
}

/** 用 Tauri 原生对话框选择文件并读回。返回解析后的对象；用户取消返回 null。 */
async function loadViaTauri(): Promise<unknown | null> {
  const path = await openDialog({
    multiple: false,
    directory: false,
    filters: [{ name: "股票存档", extensions: [SAVE_EXT] }],
  });
  // open() 取消时返回 null。
  if (path === null) return null;
  // open() 在某些配置下可能返回 string[]，这里强制单选，按 string 处理。
  const p = Array.isArray(path) ? path[0] : path;
  if (typeof p !== "string" || p.length === 0) {
    throw new Error("未选择有效文件路径");
  }
  const text = await readTextFile(p);
  return parseSaveJson(text);
}

// ---------------------------------------------------------------------------
// 浏览器 FS Access API 分支
// ---------------------------------------------------------------------------

/** 浏览器是否支持 File System Access API（showSaveFilePicker 等）。 */
function hasFsAccessApi(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof (window as unknown as { showSaveFilePicker?: unknown }).showSaveFilePicker === "function" &&
    typeof (window as unknown as { showOpenFilePicker?: unknown }).showOpenFilePicker === "function"
  );
}

async function selectViaFsAccess(): Promise<DayEndFileTarget> {
  const w = window as unknown as {
    showSaveFilePicker: (opts: unknown) => Promise<FileSystemFileHandle>;
  };
  const handle = await w.showSaveFilePicker({
    suggestedName: defaultFileName(),
    types: [
      {
        description: "股票存档",
        accept: { [SAVE_MIME]: [`.${SAVE_EXT}`] },
      },
    ],
  });
  return {
    async write(slot, isCurrent) {
      assertCurrent(isCurrent);
      const json = JSON.stringify(parseSaveSlot(slot));
      let writable: FileSystemWritableFileStream | null = null;
      try {
        writable = await handle.createWritable();
        assertCurrent(isCurrent);
        await writable.write(json);
        assertCurrent(isCurrent);
        await writable.close();
      } catch (error) {
        if (writable !== null) {
          try {
            await writable.abort(error);
          } catch (abortError) {
            throw new AggregateError([error, abortError], `日终文件写入失败：${errorMessage(error)}；abort 撤销临时写入失败：${errorMessage(abortError)}`, { cause: error });
          }
        }
        throw new Error(`日终文件写入失败：${errorMessage(error)}`, { cause: error });
      }
    },
  };
}

async function loadViaFsAccess(): Promise<unknown | null> {
  const w = window as unknown as {
    showOpenFilePicker: (opts: unknown) => Promise<FileSystemFileHandle[]>;
  };
  const [handle] = await w.showOpenFilePicker({
    multiple: false,
    types: [
      {
        description: "股票存档",
        accept: { [SAVE_MIME]: [`.${SAVE_EXT}`] },
      },
    ],
  });
  const file = await handle.getFile();
  const text = await file.text();
  return parseSaveJson(text);
}

// ---------------------------------------------------------------------------
// 浏览器降级分支（仅上传）
// ---------------------------------------------------------------------------

/**
 * 弹出隐藏的 `<input type=file>` 让用户选一个文件并读回。
 * 用户取消 → resolve(null)；读取失败 → reject。
 */
function loadViaUpload(): Promise<unknown | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = `.${SAVE_EXT}`;
    input.style.position = "fixed";
    input.style.left = "-9999px";
    let settled = false;
    input.addEventListener("change", () => {
      const file = input.files && input.files[0];
      if (!file) {
        if (!settled) { settled = true; resolve(null); }
        return;
      }
      file
        .text()
        .then((text) => {
          try {
            resolve(parseSaveJson(text));
          } catch (e) {
            reject(new Error(`存档文件不是合法 JSON：${e instanceof Error ? e.message : String(e)}`));
          }
        })
        .catch((e) => {
          reject(new Error(`读取文件失败：${e instanceof Error ? e.message : String(e)}`));
        });
    });
    // 用户取消文件选择框：change 不触发，靠窗口 focus 兜底（粗略）。
    window.addEventListener(
      "focus",
      () => {
        setTimeout(() => {
          if (!settled && (!input.files || input.files.length === 0)) {
            settled = true;
            resolve(null);
          }
        }, 1000);
      },
      { once: true },
    );
    document.body.appendChild(input);
    input.click();
    input.remove();
  });
}

// ---------------------------------------------------------------------------
// 公共入口
// ---------------------------------------------------------------------------

export async function saveToFile(_slot: unknown): Promise<boolean> {
  throw new Error("日内禁止通过 saveToFile 保存当前状态；请用 selectDayEndFileTarget 选择目标，仅在自然日日结成功后由日终协调器写入候选。");
}

export async function selectDayEndFileTarget(): Promise<DayEndFileTarget | null> {
  if (isTauri()) {
    try {
      return await selectViaTauri();
    } catch (error) {
      throw new Error(`日终文件目标选择失败：${errorMessage(error)}`, { cause: error });
    }
  }
  if (typeof window === "undefined" || typeof (window as unknown as { showSaveFilePicker?: unknown }).showSaveFilePicker !== "function") {
    throw new Error("当前浏览器不支持已授权存档文件的持续覆盖；请选择支持 File System Access API 的浏览器或桌面端。不会用下载冒充覆盖。");
  }
  try {
    return await selectViaFsAccess();
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") return null;
    throw new Error(`日终文件目标选择失败：${errorMessage(error)}`, { cause: error });
  }
}

/**
 * 从文件读档。环境自适应。
 * @returns 存档对象；用户取消返回 null。失败抛出 Error。
 */
export async function loadFromFile(): Promise<StrictSaveEnvelope | null> {
  let loaded: unknown | null;
  if (isTauri()) {
    loaded = await loadViaTauri();
  } else if (hasFsAccessApi()) {
    try {
      loaded = await loadViaFsAccess();
    } catch (e) {
      if (e instanceof DOMException && e.name === "AbortError") return null;
      throw new Error(`文件读取失败：${e instanceof Error ? e.message : String(e)}`);
    }
  } else {
    loaded = await loadViaUpload();
  }
  return loaded === null ? null : parseSaveSlot(loaded);
}
