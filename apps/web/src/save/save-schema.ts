import { parseStrictSaveEnvelope, type StrictSaveEnvelope } from "./schema/root.ts"

export function parseSaveSlot(value: unknown): StrictSaveEnvelope {
  return parseStrictSaveEnvelope(value)
}

export function parseSaveJson(text: string): StrictSaveEnvelope {
  try {
    return parseSaveSlot(JSON.parse(text))
  } catch (error) {
    if (error instanceof SyntaxError) throw new Error(`存档不是合法 JSON：${error.message}`)
    throw error
  }
}
