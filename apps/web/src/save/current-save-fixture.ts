import { readFileSync } from "node:fs"

type JsonObject = Record<string, unknown>

const engineSave = JSON.parse(readFileSync(new URL("./fixtures/minimal-current-save.json", import.meta.url), "utf8")) as JsonObject

export function currentSaveFixture(): JsonObject {
  return structuredClone(engineSave)
}
