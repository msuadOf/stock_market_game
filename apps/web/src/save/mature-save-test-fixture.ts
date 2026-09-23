import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { readFileSync } from "node:fs"
import { gunzipSync } from "node:zlib"

import { upgradeLegacySaveFixture } from "./save-v2-test-fixture.ts"

const ARCHIVE_SHA256 = "b6adb18db572faabd1ab24f76cd818152e65f6fb1cab46800c5ae083f9462738"
const JSON_SHA256 = "13ede8a8b27b79b5a7b2707ae0c1ee2197d25fff65e81532e53ac0f140f175fe"
const archive = readFileSync(new URL("./fixtures/task29-save.json.gz", import.meta.url))

assert.equal(createHash("sha256").update(archive).digest("hex"), ARCHIVE_SHA256, "mature save fixture archive digest")
const json = gunzipSync(archive)
assert.equal(createHash("sha256").update(json).digest("hex"), JSON_SHA256, "mature save fixture JSON digest")

const legacy = JSON.parse(json.toString("utf8")) as Record<string, unknown>
const current = upgradeLegacySaveFixture(legacy)

export function matureLegacySaveFixture(): Record<string, unknown> {
  return legacy
}

export function matureCurrentSaveFixture(): Record<string, unknown> {
  return current
}
