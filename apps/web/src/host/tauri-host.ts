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
import type { EngineEvent, SaveSlot, SessionSetup, Snapshot } from "../types/engine";
import type { EngineHost } from "./engine-host";
import { createTauriEventCoordinator } from "./tauri-event-coordinator";

/** 后端 `emit("engine-event", payload)` 的 payload（见 lib.rs `EngineEventPayload`）。 */
interface EngineEventPayload {
  session_id: string;
  events: EngineEvent[];
  runtime_snapshot?: Snapshot;
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

/** 创建并等待监听器、Rust 会话和首帧快照全部就绪。 */
export async function createTauriHost(setup: SessionSetup, seed: bigint): Promise<EngineHost> {
  let sessionId: string | null = null;
  let unlisten: UnlistenFn | null = null;
  let onEvents: ((events: EngineEvent[]) => void) | null = null;
  let onSnapshot: ((snapshot: Snapshot) => void) | null = null;
  let onFatalError: ((message: string) => void) | null = null;
  // 当前快照缓存：供同步 snapshot()/tick()/day() 读取。后端事件不含完整快照，
  // 故首帧由 start() 内 await invoke('snapshot') 写入；后续仍读这份缓存（增量靠 RTK applyEvents）。
  let cachedSnapshot: Snapshot | null = null;
  let disposed = false;
  const reportHostError = (reason: string) => {
    onEvents?.([{ SettlementError: {
      seq: 0,
      account: 0,
      code: "SYSTEM",
      reason,
    } }]);
  };
  const coordinator = createTauriEventCoordinator({
    deliverEvents(events) {
      onEvents?.(events);
    },
    deliverSnapshot(snapshot) {
      cachedSnapshot = deepNormalize<Snapshot>(snapshot);
      onSnapshot?.(cachedSnapshot);
    },
  });

  try {
    unlisten = await listen<EngineEventPayload>("engine-event", (e) => {
      const payload = e.payload;
      if (
        payload &&
        Array.isArray(payload.events) &&
        sessionId !== null &&
        payload.session_id === sessionId
      ) {
        try {
          coordinator.accept(payload.events, payload.runtime_snapshot);
        } catch (error) {
          const message = `Tauri 事件协议错误，游戏已中止：${String(error)}`;
          void invoke("pause_session", { sessionId }).catch((pauseError) => {
            console.error(`[TauriHost] 协议错误后的暂停也失败：${String(pauseError)}`);
          });
          if (onFatalError) onFatalError(message);
          else console.error(`[TauriHost] ${message}`);
        }
      }
    });
    sessionId = await invoke<string>("create_session", { setup, seed: seed.toString() });
    const snap = await invoke<Snapshot>("snapshot", { sessionId });
    cachedSnapshot = deepNormalize<Snapshot>(snap);
  } catch (error) {
    if (unlisten) await unlisten();
    if (sessionId !== null) {
      await invoke("stop_session", { sessionId }).catch((stopError) => {
        console.error(`[TauriHost] 初始化失败后的会话清理也失败：${String(stopError)}`);
      });
    }
    throw new Error(`Tauri 会话初始化失败：${String(error)}`);
  }

  return {
    start(cb, snapshotCb, fatalCb) {
      if (disposed) throw new Error("Tauri 会话已经销毁，不能重新启动");
      onEvents = cb;
      if (snapshotCb) onSnapshot = snapshotCb;
      if (fatalCb) onFatalError = fatalCb;
      if (sessionId === null) throw new Error("Tauri 会话尚未就绪");
      void invoke("resume_session", { sessionId }).catch((error) => {
        onFatalError?.(`Tauri 会话启动失败：${String(error)}`);
      });
    },
    stop() {
      const id = sessionId;
      if (id !== null) {
        void invoke("pause_session", { sessionId: id }).catch((error) => {
          reportHostError(`暂停 Tauri 会话失败：${String(error)}`);
        });
      }
    },
    dispose() {
      const id = sessionId;
      if (id !== null) {
        void invoke("stop_session", { sessionId: id }).catch((error) => {
          console.error(`[TauriHost] 释放会话失败：${String(error)}`);
        });
      }
      if (unlisten) {
        void unlisten();
        unlisten = null;
      }
      sessionId = null;
      disposed = true;
      onEvents = null;
      onSnapshot = null;
      onFatalError = null;
      cachedSnapshot = null;
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
      const speed = x === Infinity ? "Fastest" : { Fixed: x };
      invoke("set_speed", { sessionId, speed }).catch((error) => {
        reportHostError(`设置 Tauri 倍速失败：${String(error)}`);
      });
    },
    setFrameRate(_fps: number) {},
    async save() {
      if (sessionId === null) throw new Error("会话尚未创建，无法保存");
      return deepNormalize<SaveSlot>(await invoke<SaveSlot>("save_session", { sessionId }));
    },
    async load(slot) {
      if (sessionId === null) throw new Error("会话尚未创建，无法加载存档");
      const restored = await invoke<Snapshot>("restore_session", { sessionId, slot });
      cachedSnapshot = deepNormalize<Snapshot>(restored);
      onSnapshot?.(cachedSnapshot);
    },
    submitIntent(intent) {
      if (sessionId === null) {
        throw new Error("会话尚未创建，无法提交意图（请先 start）");
      }
      // fire-and-forget 入队；engine 拒单会以 IntentRejected 事件回到 onEvents。
      // eslint-disable-next-line @typescript-eslint/no-floating-promises
      invoke("enqueue", { sessionId, intent }).catch((error) => {
        reportHostError(`提交 Tauri 意图失败：${String(error)}`);
      });
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
