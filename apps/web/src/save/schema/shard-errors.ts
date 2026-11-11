export type SchemaMapPosition = Readonly<{ key: string; insertion: number }>
export type SchemaFailurePosition = Readonly<{ stage: number; steps: readonly (number | SchemaMapPosition)[] }>
export type RetainedSchemaFailure = Readonly<{ position: SchemaFailurePosition; error: unknown }>

function ordinal(value: number, path: string): number {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${path} 必须为非负 safe integer`)
  return value
}
function snapshot(position: SchemaFailurePosition): SchemaFailurePosition {
  const stage = ordinal(position.stage, "schema.stage")
  const steps = position.steps.map((step, index) => {
    if (typeof step === "number") return ordinal(step, `schema.steps[${index}]`)
    if (step === null || typeof step !== "object" || typeof step.key !== "string") throw new Error(`schema.steps[${index}].key 必须为字符串`)
    return Object.freeze({ key: step.key, insertion: ordinal(step.insertion, `schema.steps[${index}].insertion`) })
  })
  return Object.freeze({ stage, steps: Object.freeze(steps) })
}
function arrayIndex(key: string): number | undefined {
  const index = Number(key)
  return Number.isInteger(index) && index >= 0 && index < 4294967295 && String(index) === key ? index : undefined
}
function compareMap(left: SchemaMapPosition, right: SchemaMapPosition): number {
  const a = arrayIndex(left.key), b = arrayIndex(right.key)
  if (a !== undefined && b !== undefined) return a - b
  if (a !== undefined) return -1
  if (b !== undefined) return 1
  return left.insertion - right.insertion
}
function compare(left: SchemaFailurePosition, right: SchemaFailurePosition): number {
  if (left.stage !== right.stage) return left.stage - right.stage
  for (let index = 0; index < Math.min(left.steps.length, right.steps.length); index++) {
    const a = left.steps[index]!, b = right.steps[index]!
    if (typeof a !== typeof b) throw new Error(`schema.steps[${index}] 的位置类型不一致`)
    const difference = typeof a === "number" ? a - (b as number) : compareMap(a, b as SchemaMapPosition)
    if (difference !== 0) return difference
  }
  return left.steps.length - right.steps.length
}

/** 仅仲裁已发现的 schema 首错；空台账不代表字段覆盖或完整校验已通过。 */
export class SchemaFailureLedger {
  private current?: RetainedSchemaFailure
  retain(position: SchemaFailurePosition, error: unknown): void {
    const frozen = snapshot(position)
    if (this.current === undefined || compare(frozen, this.current.position) < 0) this.current = Object.freeze({ position: frozen, error })
  }
  failure(): RetainedSchemaFailure | undefined { return this.current }
  throwIfAny(): void { if (this.current !== undefined) throw this.current.error }
}
