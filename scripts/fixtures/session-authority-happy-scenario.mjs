/** 包装真实产品端口，只记录端点实际公开的事实，不推造内部 durability 或 ACK。 */
export function observeAuthorityPort(port) {
  const rows = [];
  const integer = (value, field) => {
    if (!Number.isSafeInteger(value) || value < 0) throw new Error(`产品权威 ${field} 无效`);
    return String(value);
  };
  const operation = (name, describe) => async (...args) => {
    try {
      const result = await port[name](...args);
      rows.push(describe(result, args));
      return result;
    } catch (error) {
      rows.push({ kind: "operation-failed", operation: name, message: error instanceof Error ? error.message : String(error) });
      throw error;
    }
  };
  const commit = (update) => {
    if (update?.TickBatch) {
      const frames = update.TickBatch.frames;
      if (!Array.isArray(frames) || frames.length === 0) throw new Error("产品 tick 提交缺少 frames");
      const last = frames.at(-1);
      return { kind: "tick", tick: integer(last.tick, "tick"), seq: integer(last.seq_to, "seq") };
    }
    const civil = update?.CivilUpdate;
    if (!civil || typeof civil.civil_date !== "string") throw new Error("产品日界提交缺少 CivilUpdate");
    return { kind: "civil", tick: integer(civil.tick, "tick"), seq: integer(civil.seq_to, "seq"), civilDate: civil.civil_date };
  };
  const host = {
    ...port,
    state: operation("state", (value) => ({ kind: "snapshot", tick: integer(value.tick, "tick"), seq: integer(value.seq, "seq"), civilDate: value.civilDate })),
    step: operation("step", commit),
    civil: operation("civil", commit),
    save: operation("save", (_value, [candidate]) => ({ kind: "sealed-candidate", seq: integer(candidate.seq, "candidate.seq"), settledDate: candidate.settledDate })),
    restore: operation("restore", () => ({ kind: "replacement-completed" })),
  };
  return {
    host,
    evidence: () => ({
      coverage: "observable-happy-path-only",
      unavailable: ["archive.queued", "archive.durable", "archive.blocked", "observer.lease", "observer.sent", "observer.flushed", "observer.accepted", "fault-injection"],
      rows: structuredClone(rows),
    }),
  };
}
