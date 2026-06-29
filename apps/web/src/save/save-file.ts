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
//
// 注意：`@tauri-apps/plugin-dialog` / `@tauri-apps/plugin-fs` 在纯浏览器构建里
// 并未安装（桌面端才引入）。这里用【变量形式的动态 import】——
// 当 import 路径是非常量表达式时，TS 不做模块解析检查，类型退化为 Promise<unknown>，
// 从而不依赖这两个包也能通过 `tsc -b`。运行时若包不存在会进 catch，安全降级。
const PLUGIN_DIALOG = "@tauri-apps/plugin-dialog";
const PLUGIN_FS = "@tauri-apps/plugin-fs";

/** 可调用（函数）的极简类型，用于从动态导入的插件模块里挑出方法。 */
type Callable = (...args: unknown[]) => unknown;

/** 是否运行在 Tauri 桌面壳内。 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

interface TauriFileApi {
  save: (opts: unknown) => Promise<string | null>;
  open: (opts: unknown) => Promise<string | null>;
  writeTextFile: (path: string, contents: string) => Promise<void>;
  readTextFile: (path: string) => Promise<string>;
}

/** 懒加载 Tauri 文件插件（缓存）。失败则抛出可读错误。 */
let tauriFileApiPromise: Promise<TauriFileApi> | null = null;
async function loadTauriFileApi(): Promise<TauriFileApi> {
  if (!tauriFileApiPromise) {
    tauriFileApiPromise = (async () => {
      try {
        // 非常量动态 import：TS 不做模块解析（插件在纯浏览器构建未安装），
        // 类型退化为 Promise<unknown>；运行时在 Tauri 桌面端才命中真实模块。
        const dialog = (await import(/* @vite-ignore */ PLUGIN_DIALOG)) as Record<string, Callable>;
        const fs = (await import(/* @vite-ignore */ PLUGIN_FS)) as Record<string, Callable>;
        if (typeof dialog.save !== "function" || typeof dialog.open !== "function"
          || typeof fs.writeTextFile !== "function" || typeof fs.readTextFile !== "function") {
          throw new Error("Tauri 文件插件缺少所需方法（save/open/writeTextFile/readTextFile）");
        }
        // 插件方法是泛型可调用对象；此处按已知签名断言为 TauriFileApi 的方法形态。
        return {
          save: dialog.save.bind(dialog) as TauriFileApi["save"],
          open: dialog.open.bind(dialog) as TauriFileApi["open"],
          writeTextFile: fs.writeTextFile.bind(fs) as TauriFileApi["writeTextFile"],
          readTextFile: fs.readTextFile.bind(fs) as TauriFileApi["readTextFile"],
        };
      } catch (e) {
        throw new Error(
          `加载 Tauri 文件插件失败（@tauri-apps/plugin-dialog / plugin-fs 未安装？）：${e instanceof Error ? e.message : String(e)}`,
        );
      }
    })();
  }
  return tauriFileApiPromise;
}

/** 用 Tauri 原生对话框另存为文件。返回 true 表示成功。 */
async function saveViaTauri(json: string): Promise<boolean> {
  const api = await loadTauriFileApi();
  const path = await api.save({
    defaultPath: defaultFileName(),
    filters: [{ name: "股票存档", extensions: [SAVE_EXT] }],
  });
  // 用户取消：save() 返回 null（非错误，不抛）。
  if (path === null) return false;
  await api.writeTextFile(path, json);
  return true;
}

/** 用 Tauri 原生对话框选择文件并读回。返回解析后的对象；用户取消返回 null。 */
async function loadViaTauri(): Promise<unknown | null> {
  const api = await loadTauriFileApi();
  const path = await api.open({
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
  const text = await api.readTextFile(p);
  return JSON.parse(text);
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
  return JSON.parse(text);
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
            resolve(JSON.parse(text));
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
export async function saveToFile(slot: unknown): Promise<boolean> {
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
export async function loadFromFile(): Promise<unknown | null> {
  if (isTauri()) {
    return loadViaTauri();
  }
  if (hasFsAccessApi()) {
    try {
      return await loadViaFsAccess();
    } catch (e) {
      if (e instanceof DOMException && e.name === "AbortError") return null;
      throw new Error(`文件读取失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }
  return loadViaUpload();
}
