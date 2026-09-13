import { SaveSchemaError, array, civilDate, exact, integer, record, string } from "../../primitives.ts"

export type ScheduledAction = { readonly InterestAccrual: { readonly company: string } } | { readonly ContractMaturity: { readonly company: string; readonly reference: string } }
export type ScheduledDue = { readonly id: number; readonly key: string; readonly due_date: string; readonly action: ScheduledAction }
export type Scheduler = { readonly next_seq: number; readonly settled_through: string | null; readonly pending: readonly ScheduledDue[] }

export function parseScheduledAction(value: unknown, path: string): ScheduledAction {
  const entries = Object.entries(record(value, path))
  if (entries.length !== 1 || entries[0] === undefined) throw new SaveSchemaError(path, "必须是单一调度动作")
  const [tag, body] = entries[0]
  const item = record(body, `${path}.${tag}`)
  switch (tag) {
    case "InterestAccrual":
      exact(item, ["company"], `${path}.InterestAccrual`)
      return { InterestAccrual: { company: string(item.company, `${path}.InterestAccrual.company`) } }
    case "ContractMaturity":
      exact(item, ["company", "reference"], `${path}.ContractMaturity`)
      return { ContractMaturity: { company: string(item.company, `${path}.ContractMaturity.company`), reference: string(item.reference, `${path}.ContractMaturity.reference`) } }
    default: throw new SaveSchemaError(path, "包含无效调度动作")
  }
}

function due(value: unknown, path: string): ScheduledDue {
  const item = record(value, path)
  exact(item, ["id", "key", "due_date", "action"], path)
  return { id: integer(item.id, `${path}.id`, 0), key: string(item.key, `${path}.key`), due_date: civilDate(item.due_date, `${path}.due_date`), action: parseScheduledAction(item.action, `${path}.action`) }
}

export function parseScheduler(value: unknown, path: string): Scheduler {
  const item = record(value, path)
  exact(item, ["next_seq", "settled_through", "pending"], path)
  return { next_seq: integer(item.next_seq, `${path}.next_seq`, 0), settled_through: item.settled_through === null ? null : civilDate(item.settled_through, `${path}.settled_through`), pending: array(item.pending, `${path}.pending`).map((entry, index) => due(entry, `${path}.pending[${index}]`)) }
}
