import { parseStrictSaveEnvelope, type StrictSaveEnvelope } from "./schema/root.ts"
import { materializeSaveValue, validateSaveValue } from "./schema/primitives.ts"
import { parseRawSaveJson } from "./schema/raw-json.ts"

export function parseSaveSlot(value: unknown): StrictSaveEnvelope {
  validateSaveValue(value)
  return parseStrictSaveEnvelope(value)
}

export function parseSaveJson(text: string): StrictSaveEnvelope {
  try {
    return materializeSaveValue(parseStrictSaveEnvelope(parseRawSaveJson(text)))
  } catch (error) {
    if (error instanceof SyntaxError) throw new Error(`存档不是合法 JSON：${error.message}`)
    throw error
  }
}
