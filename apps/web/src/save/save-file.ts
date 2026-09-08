/**
 * 文件存档：把引擎存档（SaveSlot JSON）读写到磁盘文件。
 *
 * 三种环境自适应：
 * 1. **Tauri 桌面端**（`window.__TAURI_INTERNALS__` 存在）：
 *    用 `@tauri-apps/plugin-dialog` 弹原生文件对话框 + `@tauri-apps/plugin-fs` 读写文件。
 * 2. **现代浏览器**（File System Access API 可用）：`showSaveFilePicker` / `showOpenFilePicker`。
 * 3. **不支持 FS Access 的浏览器**（Safari/Firefox/旧版）：降级为
 *    `<a download>` 下载 + `<input type=file>` 上传。
 *
 * 防御式（铁律二）：任何环节失败都抛出可读错误（带上下文），绝不静默吞；
 * 调用方负责把错误展示给用户。
 */

import type { SaveSlot } from "../types/engine";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
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

/** 用 Tauri 原生对话框另存为文件。返回 true 表示成功。 */
async function saveViaTauri(json: string): Promise<boolean> {
  const path = await saveDialog({
    defaultPath: defaultFileName(),
    filters: [{ name: "股票存档", extensions: [SAVE_EXT] }],
  });
  // 用户取消：save() 返回 null（非错误，不抛）。
  if (path === null) return false;
  await writeTextFile(path, json);
  return true;
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

async function saveViaFsAccess(json: string): Promise<boolean> {
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
  const writable = await handle.createWritable();
  try {
    await writable.write(json);
  } finally {
    await writable.close();
  }
  return true;
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
// 浏览器降级分支（下载 + 上传）
// ---------------------------------------------------------------------------

/** 触发一次隐藏的 `<a download>` 下载。 */
async function saveViaDownload(json: string): Promise<boolean> {
  const blob = new Blob([json], { type: SAVE_MIME });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = defaultFileName();
  a.rel = "noopener";
  document.body.appendChild(a);
  a.click();
  a.remove();
  // 留出下载启动后再回收 URL。
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return true;
}

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

/**
 * 把存档对象另存为文件。环境自适应。
 * @returns 成功 true；用户取消 false。失败抛出 Error。
 */
export async function saveToFile(slot: SaveSlot): Promise<boolean> {
  const json = JSON.stringify(slot);
  if (isTauri()) {
    return saveViaTauri(json);
  }
  if (hasFsAccessApi()) {
    try {
      return await saveViaFsAccess(json);
    } catch (e) {
      // AbortError：用户在 FS Access 对话框点了取消 → 视作取消而非错误。
      if (e instanceof DOMException && e.name === "AbortError") return false;
      throw new Error(`文件保存失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }
  return saveViaDownload(json);
}

/**
 * 从文件读档。环境自适应。
 * @returns 存档对象；用户取消返回 null。失败抛出 Error。
 */
export async function loadFromFile(): Promise<SaveSlot | null> {
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
