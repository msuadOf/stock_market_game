// 同一驱动由 Native HTTP/WS 与浏览器 Worker 执行。单步只控制到达顺序，不替换市场处理。
export const SCENARIO = Object.freeze({
  seed: "42",
  codes: ["600101", "002156"],
  startDate: "2030-01-04",
  ticksPerDay: 60,
  auctionTicks: 3,
  closingAuctionTicks: 3,
  retailCount: 64,
  institutionCount: 4,
  hotCount: 2,
});

export function demand(value, message) {
  if (!value) throw new Error(`LibOS 产品对照：${message}`);
}

export function makeSetup(defaults) {
  const stocks = SCENARIO.codes.map((code) => {
    const stock = defaults.stocks.find((candidate) => candidate.code === code);
    demand(stock, `默认股票目录缺少 ${code}`);
    return stock;
  });
  demand(stocks[0].exchange === "Shanghai" && stocks[1].exchange === "Shenzhen", "测试必须覆盖沪深两个 Desk");
  return {
    ...defaults,
    stocks,
    npcs: {
      ...defaults.npcs, retail_count: SCENARIO.retailCount,
      inst_count: SCENARIO.institutionCount, hot_count: SCENARIO.hotCount,
    },
    start_date: SCENARIO.startDate,
    ticks_per_day: SCENARIO.ticksPerDay,
    auction_ticks: SCENARIO.auctionTicks,
    closing_auction_ticks: SCENARIO.closingAuctionTicks,
    runtime_sync_policy: "every_tick",
    // 安静复现盘口的机会依然来自真实 NPC 策略，不注入成交或篡改输入账户。
    strategy_params: { ...defaults.strategy_params, retail: { ...defaults.strategy_params.retail, arrival_rate: 0.8 } },
  };
}

export function fixtureFacts(update) {
  demand(update && typeof update === "object", "缺少权威更新");
  if (Object.hasOwn(update, "TickBatch")) {
    demand(Array.isArray(update.TickBatch.frames) && update.TickBatch.frames.length === 1, "每次单步须提交一帧");
    return { kind: "tick", ...commit(update.TickBatch.frames[0]) };
  }
  demand(Object.hasOwn(update, "CivilUpdate"), "未知权威更新");
  const day = update.CivilUpdate;
  return { kind: "civil", ...commit(day), boundary: day.boundary, civilDate: day.civil_date };
}

function commit(value) {
  demand(value && Number.isSafeInteger(value.tick) && Number.isSafeInteger(value.seq_from)
    && Number.isSafeInteger(value.seq_to) && Array.isArray(value.facts), "权威游标或事实缺失");
  // 保持实际发布顺序；内容包括 EventStableKey，而非把事件按标的或账户重排。
  return { tick: value.tick, from: value.seq_from, to: value.seq_to, facts: value.facts };
}

export function verifyAuthoritySpan(entry, previousSeq) {
  const { from, to, facts } = entry;
  demand(from === previousSeq && from >= 0 && to >= from && to - from === facts.length,
    "已提交事实的 seq 覆盖不连续");
  const seqs = new Set(), keys = new Set();
  for (const { event, key } of facts) {
    demand(event && Object.keys(event).length === 1, "非法权威事件");
    const seq = Object.values(event)[0]?.seq;
    demand(Number.isSafeInteger(seq) && seq > from && seq <= to && !seqs.has(seq), "权威 seq 缺失或重复");
    seqs.add(seq);
    demand(key && Number.isSafeInteger(key.phase_rank) && key.phase_rank >= 0
      && Number.isSafeInteger(key.local_event_index) && key.local_event_index >= 0
      && typeof key.source === "string", "缺失 typed EventStableKey");
    const identity = JSON.stringify([key.phase_rank, key.entity, key.source, key.local_event_index]);
    demand(!keys.has(identity), "同一提交存在重复 EventStableKey");
    keys.add(identity);
  }
}

function closedDayWitness(entry, date, next, nextStatus) {
  const { boundary, civilDate } = entry;
  const closes = entry.facts.flatMap(({ event }) => Object.hasOwn(event, "CivilDateAdvanced") ? [event.CivilDateAdvanced] : []);
  demand(boundary.settled_phase === "ClosedDay" && boundary.settled_date === date
    && boundary.next_date === next && civilDate === next
    && JSON.stringify(boundary.next_status) === JSON.stringify(nextStatus)
    && closes.length === 1 && closes[0].settled_date === date && closes[0].next_date === next
    && JSON.stringify(closes[0].next_status) === JSON.stringify(nextStatus),
  `周末日界事实非法：${date} → ${next}`);
  demand(entry.facts.every(({ event }) => event.CivilDateAdvanced || event.CompanyDisclosurePublished),
    `闭市日 ${date} 发布了市场成交或委托事实`);
  return { tick: entry.tick, settledDate: date, nextDate: next, nextStatus };
}

function mondayWitness(entry) {
  const auctions = [];
  for (const { event } of entry.facts) {
    demand(!event.Trade && !event.AuctionCompleted, "周一集合竞价首拍不得提前成交或清算");
    if (!event.AuctionTick) continue;
    const { tick, phase, code, indicative_price, matched_volume, imbalance } = event.AuctionTick;
    demand(tick === entry.tick && phase === "CallAuction" && Number.isSafeInteger(code)
      && Number.isSafeInteger(matched_volume) && matched_volume >= 0
      && Number.isSafeInteger(imbalance) && imbalance >= 0
      && (indicative_price === null ? matched_volume === 0
        : typeof indicative_price === "string" && /^[1-9][0-9]*$/u.test(indicative_price)),
    "周一集合竞价权威预演事实非法");
    auctions.push(code);
  }
  auctions.sort((left, right) => left - right);
  demand(auctions.length === SCENARIO.codes.length && auctions.every((code, i) => code === i),
    "周一首拍必须具有每个 Desk 的独立集合竞价见证");
  return { tick: entry.tick, date: "2030-01-07", phase: "CallAuction", desks: auctions };
}

function saveWitness(slot) {
  const keys = ["snapshot", "civil_clock", "ingress_receipt_cursors", "book_next_sequences", "resting_orders",
    "filled_orders", "market_minute_bars", "personal_trade_confirmations", "retained_market_days", "npc_attention",
    "runtime_state", "npc_order_lifecycles", "pending_player", "receipt_watermark", "next_order_id", "rng_state"];
  for (const key of keys) demand(Object.hasOwn(slot, key), `日终档缺少 ${key}`);
  // 主体认证绑定有意不同（Server 游客 vs Browser local-owner）；不比较认证字段。
  return Object.fromEntries(keys.map((key) => [key, slot[key]]));
}

export function observedNpcGroups(slot, tradedAccounts, groupCount) {
  demand(groupCount === 1 || groupCount === 2, "组数必须与已验证的宿主拓扑一致");
  const roster = Object.keys(slot.snapshot.accounts).sort((left, right) => {
    const a = BigInt(left), b = BigInt(right);
    return a < b ? -1 : a > b ? 1 : 0;
  });
  const indexByAccount = new Map(roster.map((id, index) => [id, index]));
  const membersPerGroup = Math.ceil(roster.length / groupCount);
  demand(roster[0] === "0" && membersPerGroup > 0, "完整日终档未保留分组名册");
  const groupParticipants = Array.from({ length: groupCount }, () => new Set());
  for (const account of tradedAccounts) {
    if (!Object.hasOwn(slot.npc_attention, account)) continue;
    const index = indexByAccount.get(account);
    demand(index !== undefined, `成交 NPC ${account} 缺少日终账户事实`);
    // 与 RuntimeDriver 相同，按完整 AccountId 顺序切成等宽片；只用真实成交和 NPC 记录。
    groupParticipants[Math.floor(index / membersPerGroup)].add(account);
  }
  return groupParticipants.map((participants) => participants.size);
}

export async function executeScenario(host, groupCount = 1) {
  const first = await host.state();
  demand(first.tick === 0 && first.civilDate === SCENARIO.startDate && first.phase === "CallAuction", "没有从周五开盘集合竞价之前开始");
  const submit = async (intent) => {
    await host.submit(intent);
    inputs.push({ at: lastTick, intent });
  };
  const trace = [];
  const inputs = [];
  const tradedDesks = new Set();
  const playerDesks = new Set();
  const phases = new Set([first.phase]);
  const tradedAccounts = new Set();
  const unwind = { id: null, placed: false, sent: false, done: false };
  const player = "0", shanghai = SCENARIO.codes[0];
  const unwindPrice = "1008";
  const stockIds = new Map([...SCENARIO.codes].sort().map((code, id) => [code, id]));
  const shanghaiId = stockIds.get(shanghai);
  let lastTick = 0, lastSeq = first.seq, playerTradeCount = 0, npcTradeCount = 0;

  const observe = (entry) => {
    verifyAuthoritySpan(entry, lastSeq);
    for (const { event, key } of entry.facts) {
      if (event.Trade) {
        const { code, qty, maker, taker } = event.Trade;
        demand(Number.isSafeInteger(qty) && qty > 0, "成交数量必须为正股数");
        tradedDesks.add(code);
        tradedAccounts.add(maker);
        tradedAccounts.add(taker);
        if (maker === player || taker === player) {
          playerTradeCount++;
          playerDesks.add(code);
        }
        if (maker !== player || taker !== player) npcTradeCount++;
      }
      const accepted = event.OrderAccepted;
      if (unwind.placed && unwind.id === null && accepted?.account === player
        && accepted.code === shanghaiId && accepted.side === "Buy"
        && accepted.price === unwindPrice && accepted.remaining_qty > 0) {
        demand(Number.isSafeInteger(accepted.id), "撤单必须引用真实受理的 OrderId");
        unwind.id = accepted.id;
      }
      if (unwind.sent && key?.source === "Sealed" && entry.tick < SCENARIO.ticksPerDay - SCENARIO.closingAuctionTicks
        && event.OrderCanceled?.account === player
        && event.OrderCanceled.code === shanghaiId && event.OrderCanceled.id === unwind.id) unwind.done = true;
    }
    trace.push(entry);
    lastTick = entry.tick;
    lastSeq = entry.to;
  };

  const tick = async () => {
    const entry = fixtureFacts(await host.step());
    demand(entry.kind === "tick" && entry.tick === lastTick + 1, "单步漏提交或推进超过一拍");
    observe(entry);
  };
  const civil = async () => {
    const entry = fixtureFacts(await host.civil());
    demand(entry.kind === "civil" && entry.tick === lastTick, "自然日结算不得额外推进交易 tick");
    observe(entry);
    return entry;
  };

  while (lastTick < SCENARIO.ticksPerDay) {
    const state = await host.state();
    demand(state.tick === lastTick && state.seq === lastSeq, "权威快照与已交付的 tick/seq 不一致");
    phases.add(state.phase);
    if (state.phase === "Continuous") {
      if (!unwind.placed) {
        // 连续竞价内留在簿上的限价余量才有 OrderAccepted；不假定开盘竞价队列会发它。
        // 买入申报按价格档位向下取整，直接用 1120 分昨收的有效跌停价。
        await submit({ PlaceLimit: { code: shanghai, side: "Buy", price: { Fixed: unwindPrice }, qty: 1000 } });
        unwind.placed = true;
      }
      if (unwind.id !== null && !unwind.sent) {
        await submit({ Cancel: { code: shanghai, id: unwind.id } });
        unwind.sent = true;
      }
      if (unwind.done) {
        // 先完成独立的真实撤单，再用当拍允许的最高买价尝试撮合上一拍看到的卖量。
        for (const code of SCENARIO.codes) {
          const market = state.markets[code];
          demand(market, `快照缺少 ${code} 盘口`);
          if (!playerDesks.has(stockIds.get(code)) && typeof market.best_ask === "string"
            && market.asks[0]?.[1] > 0) {
            await submit({ PlaceLimit: { code, side: "Buy", price: "Highest", qty: 100 } });
          }
        }
      }
    }
    await tick();
  }
  for (const phase of ["CallAuction", "PreOpen", "Continuous", "ClosingAuction"]) {
    demand(phases.has(phase), `未观察到交易阶段 ${phase}`);
  }
  if (!unwind.sent || !unwind.done) {
    const playerOrderFacts = trace.flatMap(({ tick, facts }) => facts.flatMap(({ event }) =>
      ((event.OrderAccepted ?? event.OrderCanceled ?? event.IntentRejected)?.account === player
        || event.Trade?.maker === player || event.Trade?.taker === player)
        ? [{ tick, event }] : [])).slice(0, 12);
    demand(false, `没有真实玩家撤单事实：${JSON.stringify({ unwind, playerOrderFacts })}`);
  }
  demand(playerDesks.size === 2 && tradedDesks.size === 2 && playerTradeCount > 1 && npcTradeCount > 0,
    `没有在两个 Desk 均观察到真实玩家成交及 NPC 参与：${JSON.stringify({
      playerDesks: [...playerDesks], tradedDesks: [...tradedDesks], playerTradeCount, npcTradeCount, inputs,
    })}`);

  const friday = await civil();
  demand(friday.boundary.settled_date === SCENARIO.startDate
    && friday.boundary.next_date === "2030-01-05" && Object.hasOwn(friday.boundary.next_status, "Closed"),
  "周五日终后没有进入闭市周六");
  const slot = await host.save({ seq: friday.to, settledDate: SCENARIO.startDate });
  const checkpoint = saveWitness(slot);
  demand(checkpoint.snapshot.tick === lastTick && checkpoint.snapshot.seq === lastSeq, "存档未绑定到已提交的周五日终游标");
  demand(checkpoint.civil_clock.settled_through === SCENARIO.startDate
    && Object.values(checkpoint.resting_orders).every((orders) => Array.isArray(orders) && orders.length === 0)
    && checkpoint.runtime_state.live_envelopes.length === 0 && checkpoint.npc_order_lifecycles.length === 0
    && checkpoint.pending_player.length === 0, "周五日终存档未释放全部在途委托、信封和待受理输入");
  const npcGroupActivity = observedNpcGroups(checkpoint, tradedAccounts, groupCount);
  demand(npcGroupActivity.every((count) => count > 0), `存在仅启动而没有真实 NPC 成交见证的组：${npcGroupActivity}`);
  const advanceThroughMonday = async () => {
    const begin = trace.length;
    const closes = [];
    for (const [date, next, nextStatus] of [
      ["2030-01-05", "2030-01-06", { Closed: "Weekend" }],
      ["2030-01-06", "2030-01-07", "Trading"],
    ]) {
      closes.push(closedDayWitness(await civil(), date, next, nextStatus));
    }
    const monday = await host.state();
    demand(monday.civilDate === "2030-01-07" && monday.phase === "CallAuction"
      && monday.tick === lastTick && monday.seq === lastSeq, "周一复市基线必须落在首拍前的权威游标");
    await tick();
    return { closedDays: closes, monday: mondayWitness(trace.at(-1)), frames: trace.slice(begin) };
  };
  const uninterrupted = await advanceThroughMonday();
  const continued = trace.at(-1);
  await host.restore(slot);
  const restored = await host.state();
  demand(restored.tick === checkpoint.snapshot.tick && restored.seq === checkpoint.snapshot.seq
    && restored.civilDate === "2030-01-05", "恢复没有回到该周五日终存档");
  const restoredCheckpoint = saveWitness(await host.save({ seq: friday.to, settledDate: SCENARIO.startDate }));
  lastTick = restored.tick;
  lastSeq = restored.seq;
  const restarted = await advanceThroughMonday();
  return {
    trace, inputs, checkpoint, restoredCheckpoint, continued, replayed: trace.at(-1),
    freeRestart: { uninterrupted, restarted }, lastTick, lastSeq, playerTradeCount, npcTradeCount, npcGroupActivity,
  };
}
