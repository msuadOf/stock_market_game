import type { StrictSaveEnvelope } from "./schema/root.ts"
import type { SessionSetup } from "../types/engine.ts"
import { inspectValidatedArchiveLease, type ValidatedArchiveLease } from "./validated-archive-lease.ts"

/** 配置投影的 tagged 来源；App 的兼容选择值见 InitialSaveValue，租约恢复须显式注入宿主能力。 */
export type InitialSaveSelection = Readonly<{ kind: "archive"; archive: StrictSaveEnvelope }> | Readonly<{ kind: "lease"; lease: ValidatedArchiveLease }>
/** App 配置投影保持 seed 的 wire string，到 host 构造边界再显式转 bigint。 */
export type InitialSaveConfiguration = Readonly<{ setup: SessionSetup; seed: string }>
/** 只投影来源的配置，不把元数据当准入票；archive 分支保持原可变契约。 */
export function getInitialSaveConfiguration(selection: InitialSaveSelection): InitialSaveConfiguration {
  switch (selection.kind) {
    case "archive": return Object.freeze({ setup: selection.archive.setup, seed: selection.archive.seed })
    case "lease": {
      const details = inspectValidatedArchiveLease(selection.lease)
      return Object.freeze({ setup: details.setup, seed: details.seed })
    }
    default: throw new Error("未知的初始存档来源，必须为 archive 或 lease")
  }
}

/** App 保留原 raw archive 选择格式，同时接纳显式租约；不强迫旧 String 路径包装或迁移。 */
export type InitialSaveValue = StrictSaveEnvelope | Extract<InitialSaveSelection, { kind: "lease" }>
export function isLeaseInitialSave(value: InitialSaveValue): value is Extract<InitialSaveSelection, { kind: "lease" }> { return "kind" in value && value.kind === "lease" }
export function getInitialSaveValueConfiguration(value: InitialSaveValue): InitialSaveConfiguration {
  return getInitialSaveConfiguration(isLeaseInitialSave(value) ? value : { kind: "archive", archive: value })
}
