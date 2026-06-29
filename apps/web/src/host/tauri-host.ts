/**
 * Tauri 引擎宿主（桌面端版）。
 *
 * 与 `wasm-host.ts` 实现同一份 `EngineHost` 接口，但引擎不在浏览器里跑——
 * 步进循环由 Rust 侧 actor（apps/desktop/src-tauri/src/actor.rs）独占驱动，
 * 每 tick 产 Event[] 经 `app.emit("engine-event", payload)` 推到前端。
 *
 * 与 WasmHost 的差异：
 * - **步进不在 JS 主线程**：`start()` 只创建会话 + 挂监听；循环由后端 actor 自行推进。
 *   因此 `setSpeed` 直接 invoke 后端改 interval，无需 JS 侧 setInterval。
 * - **事件经 Tauri event 总线**：`listen("engine-event", cb)`，payload 形如
 *   `{ session_id: string, events: EngineEvent[] }`。
 * - **快照异步**：后端 `snapshot` 命令经 invoke（异步 Promise）。`EngineHost.snapshot()` 在
 *   接口上是同步的，故这里从最近一次事件批 / `snapshot()` 结果缓存当前 Snapshot，
 *   `snapshot()`/`tick()`/`day()` 均读缓存。初始首帧由 `start()` 内 await 一次 snapshot 写入。
 *
 * 防御式（铁律二）：所有 invoke 失败都显式抛出（Tauri invoke 的 reject 即后端 `Err(String)`），
 * 绝不静默吞；非法速度显式报错。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { EngineEvent, SessionSetup, Snapshot } from "../types/engine";
import type { EngineHost } from "./wasm-host";

/** 后端 `emit("engine-event", payload)` 的 payload（见 lib.rs `EngineEventPayload`）。 */
interface EngineEventPayload {
  session_id: string;
  events: EngineEvent[];
}

/**
 * 深度规整：递归把所有 JS Map 转为普通 Object。
 *
 * 后端走标准 serde_json，理论上 Map 已序列化为普通对象；但保留规整作为防御层——
 * 即便某字段意外产出 Map，也能正确收敛为 Object，保证 RTK 消费形态一致。
 */
function deepNormalize<T>(obj: unknown): T {
  if (obj instanceof Map) {
    const result: Record<string, unknown> = {};
    for (const [key, value] of obj.entries()) {
      result[String(key)] = deepNormalize(value);
    }
    return result as T;
  }
  if (Array.isArray(obj)) {
    return obj.map(deepNormalize) as T;
  }
  if (obj !== null && typeof obj === "object") {
    const result: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(obj)) {
      result[key] = deepNormalize(value);
    }
    return result as T;
  }
  return obj as T;
}

/** 工厂：创建一个绑定到指定 setup/seed 的 TauriHost。与 createWasmHost 签名一致。 */
export function createTauriHost(setup: SessionSetup, seed: bigint): EngineHost {
  let sessionId: string | null = null;
  let unlisten: UnlistenFn | null = null;
  let onEvents: ((events: EngineEvent[]) => void) | null = null;
  // 当前快照缓存：供同步 snapshot()/tick()/day() 读取。后端事件不含完整快照，
  // 故首帧由 start() 内 await invoke('snapshot') 写入；后续仍读这份缓存（增量靠 RTK applyEvents）。
  let cachedSnapshot: Snapshot | null = null;

  return {
    start(cb) {
      onEvents = cb;
      // fire-and-forget：接口约定 start() 同步返回。失败经 Promise reject 上抛（被 App 捕获）。
      // eslint-disable-next-line @typescript-eslint/no-floating-promises
      (async () => {
        // 先挂监听，避免丢失创建后、连监听前的早期事件（actor 开局跳过首个 tick，留有窗口）。
        unlisten = await listen<EngineEventPayload>(
          "engine-event",
          (e) => {
            const payload = e.payload;
            if (payload && Array.isArray(payload.events) && onEvents) {
              onEvents(payload.events);
            }
          },
        );
        // 创建会话（后端 spawn actor 步进循环）。
        sessionId = await invoke<string>("create_session", { setup, seed: Number(seed) });
        // 拉取首帧快照写入缓存。
        const snap = await invoke<Snapshot>("snapshot", { sessionId });
        cachedSnapshot = deepNormalize<Snapshot>(snap);
      })();
    },
    stop() {
      // 优先停后端 actor（若暴露 stop_session）；无论成败都解绑前端监听。
      const id = sessionId;
      if (id !== null) {
        // fire-and-forget；忽略无 stop_session 命令时的 reject。
        // eslint-disable-next-line @typescript-eslint/no-floating-promises
        invoke("stop_session", { sessionId: id }).catch(() => {
          /* 命令可能不存在：仅解绑监听即可（见下）。不静默吞致命错误——此处属可降级路径。 */
        });
      }
      if (unlisten) {
        // eslint-disable-next-line @typescript-eslint/no-floating-promises
        unlisten();
        unlisten = null;
      }
      sessionId = null;
    },
    setSpeed(x) {
      if (x <= 0) {
        // 非法速度：显式报错，绝不静默继续。
        throw new Error(`非法速度倍率：${x}（必须为正数）`);
      }
      if (sessionId === null) {
        throw new Error("会话尚未创建，无法改速（请先 start）");
      }
      // fire-and-forget：后端 SetSpeed 经 mpsc 保证顺序。
      // eslint-disable-next-line @typescript-eslint/no-floating-promises
      invoke("set_speed", { sessionId, speed: x });
    },
    setFrameRate(_fps: number) {
      // Tauri 后端 actor 自控节奏，前端帧率不影响
    },
    submitIntent(intent) {
      if (sessionId === null) {
        throw new Error("会话尚未创建，无法提交意图（请先 start）");
      }
      // fire-and-forget 入队；engine 拒单会以 IntentRejected 事件回到 onEvents。
      // eslint-disable-next-line @typescript-eslint/no-floating-promises
      invoke("enqueue", { sessionId, intent });
    },
    snapshot() {
      if (cachedSnapshot === null) {
        throw new Error("快照尚未就绪（会话创建中或已停止）");
      }
      return cachedSnapshot;
    },
    tick() {
      if (cachedSnapshot === null) {
        throw new Error("快照尚未就绪，无法读取 tick");
      }
      return cachedSnapshot.tick;
    },
    day() {
      if (cachedSnapshot === null) {
        throw new Error("快照尚未就绪，无法读取交易日");
      }
      return cachedSnapshot.day;
    },
  };
}
