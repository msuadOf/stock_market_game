import { capturedGzipPayload, captureGzipArchiveSource } from "./archive-source.ts";
import { createValidatedArchiveLease, inspectValidatedArchiveLease, type ValidatedArchiveLease } from "./validated-archive-lease.ts";
import { SHA256_CHAIN_ALGORITHM, SHA256_CHAIN_BLOCK_BYTES } from "./sha256-chain.ts";
import { parsePlanU64 } from "../utils/plan-identity.ts";

export type ChunkedSourceDescriptor = Readonly<{
  kind: "load-chunked-source"; sourceId: string; utf8Bytes: number; seed: string;
  contentDigest: Readonly<{ algorithm: typeof SHA256_CHAIN_ALGORITHM; blockBytes: number; byteLength: string; blocks: string; digest: string }>;
}>;
export type PinnedArchiveSource = Readonly<{ kind: "pinned-archive-source" }>;
export type SourcePublicationLock = <T>(sourceId: string, signal: AbortSignal, action: () => Promise<T>) => Promise<T>;
const pins = new WeakMap<PinnedArchiveSource, { lease: ValidatedArchiveLease; descriptor: ChunkedSourceDescriptor; authority: object }>();
const prefix = "wasm-handover-source:";

/** 持久化数据仅描述目标；不能作为租约或 admission ticket。 */
export function parseChunkedSourceDescriptor(input: unknown): ChunkedSourceDescriptor {
  const row = input as Record<string, unknown>;
  if (row === null || typeof row !== "object" || Array.isArray(row) || Object.keys(row).sort().join(",") !== "contentDigest,kind,seed,sourceId,utf8Bytes") throw new Error("固定存档描述字段无效");
  const digest = row.contentDigest as Record<string, unknown>;
  if (digest === null || typeof digest !== "object" || Array.isArray(digest) || Object.keys(digest).sort().join(",") !== "algorithm,blockBytes,blocks,byteLength,digest") throw new Error("固定存档摘要字段无效");
  if (row.kind !== "load-chunked-source" || typeof row.sourceId !== "string" || !/^[a-f0-9]{32}$/.test(row.sourceId) || typeof row.utf8Bytes !== "number" || !Number.isSafeInteger(row.utf8Bytes) || row.utf8Bytes < 0) throw new Error("固定存档描述无效");
  const seed = parsePlanU64(row.seed, "固定存档 seed");
  const bytes = BigInt(row.utf8Bytes);
  if (digest.algorithm !== SHA256_CHAIN_ALGORITHM || digest.blockBytes !== SHA256_CHAIN_BLOCK_BYTES || digest.byteLength !== bytes.toString() || digest.blocks !== ((bytes + BigInt(SHA256_CHAIN_BLOCK_BYTES) - 1n) / BigInt(SHA256_CHAIN_BLOCK_BYTES)).toString() || typeof digest.digest !== "string" || !/^[a-f0-9]{64}$/.test(digest.digest)) throw new Error("固定存档摘要无效");
  return { kind: "load-chunked-source", sourceId: row.sourceId, utf8Bytes: row.utf8Bytes, seed, contentDigest: { algorithm: SHA256_CHAIN_ALGORITHM, blockBytes: SHA256_CHAIN_BLOCK_BYTES, byteLength: digest.byteLength, blocks: digest.blocks, digest: digest.digest } as ChunkedSourceDescriptor["contentDigest"] };
}

export function inspectPinnedArchiveSource(pin: PinnedArchiveSource, lease: ValidatedArchiveLease): ChunkedSourceDescriptor {
  const record = pins.get(pin);
  if (record === undefined || record.lease !== lease) throw new Error("固定存档凭证与租约不一致");
  return record.descriptor;
}

function nonce(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, byte => byte.toString(16).padStart(2, "0")).join("");
}

/** 面板临时描述只用于确认选择，不证明源已持久化。 */
export function describeValidatedArchiveLease(lease: ValidatedArchiveLease): ChunkedSourceDescriptor {
  const details = inspectValidatedArchiveLease(lease), digest = details.contentDigest;
  return { kind: "load-chunked-source", sourceId: nonce(), utf8Bytes: details.utf8Bytes, seed: details.seed,
    contentDigest: { algorithm: digest.algorithm, blockBytes: digest.blockBytes, byteLength: digest.byteLength.toString(), blocks: digest.blocks.toString(), digest: digest.digest } };
}

export function createChunkedHandoverSources(storage: {
  getItem(key: string): Promise<string | null>;
  hasItem?(key: string): Promise<boolean>;
  addItem(key: string, value: string, isCurrent: () => boolean, seed?: string): Promise<boolean>;
  removeItem(key: string): Promise<void>;
}, randomId = nonce, withLock: SourcePublicationLock = async (_id, _signal, action) => action()) {
  const authority = {};
  const cache = new WeakMap<ValidatedArchiveLease, WeakMap<AbortSignal, Promise<PinnedArchiveSource>>>();
  return {
    async withDescriptorPublication<T>(descriptor: ChunkedSourceDescriptor, signal: AbortSignal, action: () => T): Promise<T> {
      descriptor = parseChunkedSourceDescriptor(descriptor);
      return withLock(descriptor.sourceId, signal, async () => {
        signal.throwIfAborted();
        const exists = storage.hasItem === undefined ? await storage.getItem(prefix + descriptor.sourceId) !== null : await storage.hasItem(prefix + descriptor.sourceId);
        signal.throwIfAborted();
        if (!exists) throw new Error("固定的重载存档不存在；原交接保持未消费");
        return action();
      });
    },
    async withPinnedPublication<T>(pin: PinnedArchiveSource, lease: ValidatedArchiveLease, signal: AbortSignal, action: () => T): Promise<T> {
      const record = pins.get(pin);
      if (record === undefined || record.authority !== authority || record.lease !== lease) throw new Error("固定存档凭证不属于本来源或租约");
      return this.withDescriptorPublication(record.descriptor, signal, action);
    },
    inspect(pin: PinnedArchiveSource, lease: ValidatedArchiveLease): ChunkedSourceDescriptor {
      const record = pins.get(pin);
      if (record === undefined || record.authority !== authority || record.lease !== lease) throw new Error("固定存档凭证不属于本来源或租约");
      return record.descriptor;
    },
    pin(lease: ValidatedArchiveLease, signal: AbortSignal): Promise<PinnedArchiveSource> {
      signal.throwIfAborted();
      const details = inspectValidatedArchiveLease(lease);
      let bySignal = cache.get(lease);
      if (bySignal === undefined) { bySignal = new WeakMap(); cache.set(lease, bySignal); }
      const existing = bySignal.get(signal);
      const currentCache = bySignal;
      if (existing !== undefined) return existing.then(async pin => {
        signal.throwIfAborted();
        const descriptor = inspectPinnedArchiveSource(pin, lease);
        const raw = await storage.getItem(prefix + descriptor.sourceId);
        signal.throwIfAborted();
        if (raw !== capturedGzipPayload(details.source)) {
          if (currentCache.get(signal) === existing) currentCache.delete(signal);
          throw new Error("固定存档源已删除或已改变，请重试选择");
        }
        return pin;
      });
      const pending = (async () => {
        const sourceId = randomId();
        if (!/^[0-9a-f]{32}$/.test(sourceId)) throw new Error("固定存档 sourceId 必须为 128 位小写十六进制");
        const digest = details.contentDigest;
        const descriptor = Object.freeze({ kind: "load-chunked-source" as const, sourceId, utf8Bytes: details.utf8Bytes, seed: details.seed,
          contentDigest: Object.freeze({ algorithm: digest.algorithm, blockBytes: digest.blockBytes, byteLength: digest.byteLength.toString(), blocks: digest.blocks.toString(), digest: digest.digest }) });
        const stored = await storage.addItem(prefix + sourceId, capturedGzipPayload(details.source), () => !signal.aborted, details.seed);
        if (signal.aborted && stored) {
          try { await storage.removeItem(prefix + sourceId); }
          catch (error) { throw new AggregateError([signal.reason, error], "取消后清理未发布固定源失败"); }
        }
        signal.throwIfAborted();
        if (!stored) throw new Error("固定存档事务未提交");
        const pin = Object.freeze({ kind: "pinned-archive-source" as const });
        pins.set(pin, { lease, descriptor, authority });
        return pin;
      })();
      currentCache.set(signal, pending);
      void pending.catch(() => { if (currentCache.get(signal) === pending) currentCache.delete(signal); });
      return pending;
    },
    async resolve(descriptor: ChunkedSourceDescriptor, signal: AbortSignal): Promise<ValidatedArchiveLease> {
      signal.throwIfAborted();
      descriptor = parseChunkedSourceDescriptor(descriptor);
      const raw = await storage.getItem(prefix + descriptor.sourceId);
      signal.throwIfAborted();
      if (raw === null) throw new Error("固定的重载存档不存在；原交接保持未消费");
      const lease = await createValidatedArchiveLease(captureGzipArchiveSource(raw), signal);
      const value = inspectValidatedArchiveLease(lease);
      const digest = value.contentDigest;
      if (value.utf8Bytes !== descriptor.utf8Bytes || value.seed !== descriptor.seed || digest.algorithm !== descriptor.contentDigest.algorithm || digest.blockBytes !== descriptor.contentDigest.blockBytes || digest.byteLength.toString() !== descriptor.contentDigest.byteLength || digest.blocks.toString() !== descriptor.contentDigest.blocks || digest.digest !== descriptor.contentDigest.digest) throw new Error("固定存档内容与冻结摘要不一致");
      return lease;
    },
  };
}
