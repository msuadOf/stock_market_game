import type { CapturedArchiveSource } from "./archive-source.ts"
import { validateCapturedArchiveSchema } from "./archive-schema-validation.ts"
import type { SessionSetup } from "../types/engine.ts"
import type { Sha256ChainResult } from "./sha256-chain.ts"
import { SHA256_CHAIN_ALGORITHM, SHA256_CHAIN_BLOCK_BYTES } from "./sha256-chain.ts"

/** 独立验证阶段的租约，尚不是生产 Memory admission 凭证。 */
export type ValidatedArchiveLease = Readonly<{ kind: "validated-archive-lease"; productionAdmission: false }>
export type ValidatedArchiveLeaseDetails = Readonly<{ source: CapturedArchiveSource; utf8Bytes: number; contentDigest: Sha256ChainResult; setup: SessionSetup; seed: string; productionAdmission: false }>
const leases = new WeakMap<ValidatedArchiveLease, ValidatedArchiveLeaseDetails>()

function deepFreeze<Value>(value: Value): Value {
  if (value !== null && typeof value === "object") {
    for (const child of Object.values(value)) deepFreeze(child)
    Object.freeze(value)
  }
  return value
}

/** 只接受真实捕获源，内部执行完整 schema、跨域与日终验证，不接受外部报告或 codec。 */
export async function createValidatedArchiveLease(source: CapturedArchiveSource, signal?: AbortSignal): Promise<ValidatedArchiveLease> {
  signal?.throwIfAborted()
  const result = await validateCapturedArchiveSchema(source, signal)
  signal?.throwIfAborted()
  if (result.source !== source || result.schemaValidated !== true || result.dayEndValidated !== true || result.productionAdmission !== false || !Number.isSafeInteger(result.utf8Bytes) || result.utf8Bytes < 0) {
    throw new Error("完整存档验证结果与捕获源不一致")
  }
  if (result.contentDigest.algorithm !== SHA256_CHAIN_ALGORITHM || result.contentDigest.blockBytes !== SHA256_CHAIN_BLOCK_BYTES || result.contentDigest.byteLength !== BigInt(result.utf8Bytes) || !/^[a-f0-9]{64}$/.test(result.contentDigest.digest)) throw new Error("完整存档验证摘要与字节数不一致")
  const lease = Object.freeze({ kind: "validated-archive-lease" as const, productionAdmission: false as const })
  leases.set(lease, Object.freeze({ source, utf8Bytes: result.utf8Bytes, contentDigest: Object.freeze({ ...result.contentDigest }), setup: deepFreeze(structuredClone(result.setup)), seed: result.seed, productionAdmission: false as const }))
  return lease
}

/** 只读取本模块签发的租约，不信任结构相同的对象或验证报告。 */
export function inspectValidatedArchiveLease(lease: ValidatedArchiveLease): ValidatedArchiveLeaseDetails {
  const details = leases.get(lease)
  if (details === undefined) throw new Error("无效的已验证存档租约身份")
  return details
}
