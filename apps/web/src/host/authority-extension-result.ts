/** 这里只交接实际 owner 状态；只有 controlWorker 的最终 release 能证明 Memory 可复用。 */
export type AuthorityExtensionOwner = { kind: "live" } | { kind: "consumed"; report: unknown; archiveFailure?: unknown } | { kind: "quiescence-unknown"; message: string };
export function acceptAuthorityExtensionResult(response: Readonly<Record<string, unknown>>, requested: number, adopt: (generation: number, owner: AuthorityExtensionOwner) => void): unknown {
  const next = response.nextGeneration;
  if (typeof next !== "number" || !Number.isSafeInteger(next) || next < requested) throw new Error("扩展真实generation无效");
  const owner = response.owner;
  if (owner === null || typeof owner !== "object" || !("kind" in owner)) throw new Error("扩展缺少真实owner状态");
  let parsed: AuthorityExtensionOwner;
  if (owner.kind === "live" && Object.keys(owner).join(",") === "kind") parsed = { kind: "live" };
  else if (owner.kind === "consumed" && "report" in owner && Object.keys(owner).every(key => ["kind", "report", "archiveFailure"].includes(key))) parsed = { kind: "consumed", report: owner.report, ...("archiveFailure" in owner ? { archiveFailure: owner.archiveFailure } : {}) };
  else if (owner.kind === "quiescence-unknown" && "message" in owner && typeof owner.message === "string" && Object.keys(owner).sort().join(",") === "kind,message") parsed = { kind: "quiescence-unknown", message: owner.message };
  else throw new Error("扩展owner状态无效");
  adopt(next, parsed);
  if (typeof response.error === "string") throw new Error(response.error);
  return response.result;
}
