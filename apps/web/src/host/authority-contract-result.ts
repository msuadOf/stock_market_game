/** diagnostics 合同失败仍须交接实际代际，才能按真实 owner 关闭。 */
export async function acceptAuthorityContractResult(result: Readonly<Record<string, unknown>>, requested: number,
  adopt: (generation: number) => void, ready: () => Promise<unknown>): Promise<readonly unknown[]> {
  const next = result.nextGeneration;
  if (typeof next !== "number" || !Number.isSafeInteger(next) || next < requested) throw new Error("合同验收实际结束 generation 无效");
  adopt(next);
  const primary = typeof result.error === "string" ? new Error(result.error) : null;
  try { await ready(); }
  catch (error) { if (primary !== null) throw new AggregateError([primary, error], primary.message); throw error; }
  if (primary !== null) throw primary;
  if (!Array.isArray(result.rows)) throw new Error("合同验收 rows 无效");
  return result.rows;
}

/** 成功末条在关闭归档前实际采样；失败则必须读取仍存活的当前观察流。 */
export function authorityContractCleanupLease(rows: readonly unknown[], error: string | undefined, current: () => unknown): bigint {
  let value: unknown;
  if (error === undefined) {
    const final = rows.at(-1);
    const observer = final !== null && typeof final === "object" && "observer" in final ? final.observer : undefined;
    value = observer !== null && typeof observer === "object" && "lease" in observer ? observer.lease : undefined;
  } else value = current();
  if (typeof value !== "string" || !/^[1-9][0-9]*$/.test(value)) throw new Error("合同清理缺少真实观察租约");
  return BigInt(value);
}
