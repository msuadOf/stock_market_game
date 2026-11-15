// 浏览器和 Native 探针共享同一个测量窗，不依赖目标平台。
export function instrumentWindow(host, { warmupTicks, measuredTicks, now, start, end }) {
  if (!Number.isSafeInteger(warmupTicks) || warmupTicks < 0 ||
    !Number.isSafeInteger(measuredTicks) || measuredTicks < 1) {
    throw new Error("LibOS 性能测量窗参数无效");
  }
  let completed = 0, startedAt, result;
  const tickNs = [];
  const begin = async () => {
    if (startedAt === undefined) {
      await start();
      startedAt = now();
    }
  };
  return {
    host: {
      ...host,
      async state() {
        if (completed === warmupTicks) await begin();
        return host.state();
      },
      async step() {
        if (completed === warmupTicks) await begin();
        const measure = completed >= warmupTicks && completed < warmupTicks + measuredTicks;
        const at = measure ? now() : undefined;
        const update = await host.step();
        if (at !== undefined) tickNs.push(now() - at);
        completed++;
        return update;
      },
      async civil() {
        const close = result === undefined && completed === warmupTicks + measuredTicks;
        const update = await host.civil();
        if (close) {
          const elapsedNs = now() - startedAt;
          result = { elapsedNs, tickNs, ...await end() };
        } else if (result === undefined) {
          throw new Error("LibOS 性能测量窗提前遇到日结");
        }
        return update;
      },
    },
    results() {
      if (result === undefined) throw new Error("LibOS 性能测量窗未结束");
      return result;
    },
  };
}

export function measuredAuthority(witness, { warmupTicks, measuredTicks, activeGroups }) {
  const first = witness.trace.findIndex((entry) => entry.kind === "tick" && entry.tick === warmupTicks + 1);
  const last = first + measuredTicks;
  const facts = witness.trace.slice(first, last + 1);
  if (first < 0 || facts.length !== measuredTicks + 1 ||
    facts.slice(0, -1).some((entry, index) => entry.kind !== "tick" || entry.tick !== warmupTicks + index + 1) ||
    facts.at(-1).kind !== "civil" || facts.at(-1).tick !== warmupTicks + measuredTicks) {
    throw new Error("测量窗未提交完整交易 tick 和日结");
  }
  const outcomes = facts.flatMap(({ facts: entries }) => entries.filter(({ event }) =>
    Object.hasOwn(event, "OrderAccepted") || Object.hasOwn(event, "OrderCanceled") || Object.hasOwn(event, "IntentRejected")));
  const cursor = witness.checkpoint.ingress_receipt_cursors;
  if (typeof cursor !== "object" || cursor === null || !Object.hasOwn(cursor, "next_account_ordinal") ||
    !Object.hasOwn(cursor, "next_stock_ordinal")) throw new Error("日结存档没有实测 ingress receipt 游标");
  const trades = facts.flatMap(({ facts: entries }) => entries.flatMap(({ event }) =>
    Object.hasOwn(event, "Trade") ? [event.Trade] : []));
  const tradedDesks = new Set(trades.map(({ code }) => code));
  const playerTradeCount = trades.filter(({ maker, taker }) => maker === "0" || taker === "0").length;
  const npcTradeCount = trades.filter(({ maker, taker }) => maker !== "0" || taker !== "0").length;
  const accepts = outcomes.filter(({ event }) => Object.hasOwn(event, "OrderAccepted"));
  const playerAccepts = accepts.filter(({ event }) => event.OrderAccepted.account === "0").length;
  const npcOrders = accepts.length - playerAccepts;
  if (playerAccepts === 0 || npcOrders === 0 || playerTradeCount < 2 || npcTradeCount === 0 ||
    tradedDesks.size !== 2 || witness.desks !== 2 || witness.groups !== activeGroups) {
    throw new Error("测量窗缺少实际玩家、NPC 或双 Desk 活动");
  }
  return {
    facts,
    receiptWitness: {
      acknowledgedInputs: witness.inputs.filter(({ at }) => at >= warmupTicks && at < warmupTicks + measuredTicks),
      outcomes, cursor,
    },
    summary: {
      firstCommittedTick: facts[0].tick,
      lastCommittedTick: facts.at(-1).tick,
      lastCommittedSeq: facts.at(-1).to,
      activeGroups,
      deskExchanges: ["Shanghai", "Shenzhen"],
      playerAccepts, npcOrders,
    },
  };
}
