import { readFileSync } from "node:fs"

const operations = JSON.parse(readFileSync(new URL("./fixtures/current-company-slice.json", import.meta.url), "utf8")) as Record<string, unknown>

export function representativeCompanyOperationsFixture(): Record<string, unknown> {
  return operations
}
