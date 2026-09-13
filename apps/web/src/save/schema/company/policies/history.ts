import { civilDate, exact, record } from "../../primitives.ts"

export type History = { readonly generated_through: string }

export function parseHistory(value: unknown, path: string): History {
  const item = record(value, path)
  exact(item, ["generated_through"], path)
  return { generated_through: civilDate(item.generated_through, `${path}.generated_through`) }
}
