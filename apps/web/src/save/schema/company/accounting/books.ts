import { exact, record } from "../../primitives.ts"
import { parseChartOfAccounts, type ChartOfAccounts } from "./chart.ts"
import { parseJournal, type Journal } from "./journal.ts"

export type Books = { readonly chart: ChartOfAccounts; readonly journal: Journal }

export function parseBooks(value: unknown, path: string): Books {
  const parsed = record(value, path)
  exact(parsed, ["chart", "journal"], path)
  return {
    chart: parseChartOfAccounts(parsed.chart, `${path}.chart`),
    journal: parseJournal(parsed.journal, `${path}.journal`),
  }
}
