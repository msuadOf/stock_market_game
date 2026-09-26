import { readFileSync } from "node:fs"

const save = JSON.parse(readFileSync(new URL("./fixtures/current-schema-save.json", import.meta.url), "utf8")) as Record<string, unknown>

/** A populated schema projection; cross-domain references may be omitted, so Rust restore validity is not asserted. */
export function representativeCurrentSaveFixture(): Record<string, unknown> {
  return save
}
