import { checksumWasmMemoryRecord, wasmMemoryLedgerKey, wasmMemoryGenesisKey } from "./wasm-memory-schema.ts";
import { WasmMemoryStorage } from "./wasm-memory-storage.ts";
import type { WasmMemoryStorageOptions, WasmMemoryStoragePort } from "./wasm-memory-storage.ts";

export const MiB = 1024 * 1024;
export const CHAIN = "0123456789abcdef0123456789abcdef";
export const FORK_A = "11111111111111111111111111111111";
export const FORK_B = "22222222222222222222222222222222";
export const NONCE = "33333333333333333333333333333333";
export const CHAIN_KEY = "wasm-chain";
export const SNAPSHOT_KEY = "wasm-residual-state";
export type StorageCall = { operation: "get" | "set" | "keys"; key?: string; value?: string };

export function storageFixture() {
  const data = new Map<string, string>();
  const calls: StorageCall[] = [];
  let hook: ((call: StorageCall) => void) | null = null;
  const dispatch = (call: StorageCall) => { calls.push(call); hook?.(call); };
  const target = Object.create(null) as WasmMemoryStoragePort;
  Object.defineProperties(target, {
    getItem: { configurable: true, value: (key: string) => { dispatch({ operation: "get", key }); return data.has(key) ? data.get(key)! : null; } },
    setItem: { configurable: true, value: (key: string, value: string) => { dispatch({ operation: "set", key, value }); data.set(key, value); } },
  });
  const port = new Proxy(target, {
    ownKeys() { dispatch({ operation: "keys" }); return [...data.keys()]; },
    getOwnPropertyDescriptor(_target, key) {
      if (typeof key === "string" && data.has(key)) return { configurable: true, enumerable: true, value: data.get(key) };
      return Reflect.getOwnPropertyDescriptor(target, key);
    },
  });
  return { data, calls, port, onCall(next: typeof hook) { hook = next; } };
}
export function seal<T extends Record<string, unknown>>(body: T) {
  return { ...body, checksum: checksumWasmMemoryRecord(body) };
}
export function ledgerEntry(seq = 0, carry = 512 * MiB, reloadCount = 1) {
  return seal({ schemaVersion: 1, seq, chainId: CHAIN, carriedResidualBytes: carry, reloadCount,
    handoverNonce: NONCE, outcome: "reload", ts: 1000 + seq, highWater: { carry, reloadCount } });
}
export function snapshot(carry = 0, resident = 0, live = 0, reloadCount = 0) {
  return seal({ schemaVersion: 1, chainId: CHAIN, carryBase: carry, sessionResident: resident,
    liveUpperBound: live, reloadCount, genesis: false });
}
export function genesis() {
  return seal({ schemaVersion: 1, seq: 0, chainId: CHAIN, carriedResidualBytes: 0, reloadCount: 0,
    handoverNonce: NONCE, outcome: "genesis", ts: 1000, highWater: { carry: 0, reloadCount: 0 } });
}
export function fixture(changes: Partial<WasmMemoryStorageOptions> = {}) {
  const session = storageFixture(), ledger = storageFixture();
  // 与预置 FORK_A / FORK_B 分离，碰撞行为由专门用例显式注入。
  let random = 64;
  const options: WasmMemoryStorageOptions = { session: session.port, ledger: ledger.port,
    maximumBytes: 2048 * MiB, reclaimBytes: 512 * MiB, measuredMaxLiveBytes: 256 * MiB,
    randomValues(bytes) { bytes.fill(random++); }, now: () => 1000, ...changes };
  const owner = () => new WasmMemoryStorage(options);
  const existing = (value: unknown = snapshot()) => {
    session.data.set(CHAIN_KEY, CHAIN);
    if (value !== null) session.data.set(SNAPSHOT_KEY, JSON.stringify(value));
    ledger.data.set(wasmMemoryGenesisKey(CHAIN), JSON.stringify(genesis()));
  };
  const put = (fork: string, rows: unknown[]) => ledger.data.set(wasmMemoryLedgerKey(CHAIN, fork), JSON.stringify(rows));
  return { session, ledger, options, owner, existing, put };
}
