// 注入实际Linux Tauri窗口；普通会话行为必须走嵌入e2e前端中的actual tauri-host桥。
export function createTauriAuthorityProductPort() {
  const internal = globalThis.__TAURI_INTERNALS__, bridge = globalThis.__authorityTauriContract;
  if (typeof internal?.invoke !== "function" || typeof bridge?.create !== "function") throw new Error("需要实际Tauri窗口及独立e2e前端的actual-host合同桥");
  const invoke = (name, args = {}) => internal.invoke(name, args);
  const require = (condition, message) => { if (!condition) throw new Error(message); };
  let owner, sessionId, generation = "1", accept = false, latestRow, observer, oldLease;
  let received = "0";
  const listeners = [], events = [], keys = {};
  const waitFor = async (predicate, label) => {
    const deadline = Date.now() + 5000;
    while (!predicate()) { if (Date.now() >= deadline) throw new Error(`实际Tauri事件等待超时：${label}`); await new Promise(resolve => setTimeout(resolve, 1)); }
  };
  const listen = async () => {
    for (const event of ["engine-event", "engine-failure"]) {
      const callback = internal.transformCallback(({ payload }) => {
        if (payload.session_id !== sessionId) return;
        events.push({ event, payload });
        if (payload.publication_ordinal !== undefined) {
          require(typeof payload.publication_ordinal === "string" && typeof payload.observer_lease === "string", "实际IPC包缺少lease/ordinal");
          received = payload.publication_ordinal;
        }
      });
      const eventId = await invoke("plugin:event|listen", { event, target: { kind: "Any" }, handler: callback });
      listeners.push({ event, eventId });
    }
  };
  const unlisten = async () => {
    for (const { event, eventId } of listeners.splice(0)) {
      await invoke("plugin:event|unlisten", { event, eventId });
      globalThis.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener(event, eventId);
    }
  };
  const subscribePaused = async () => {
    await owner.subscribe(update => update.type === "baseline" || accept, failure => events.push({ event: "actual-host-failure", failure }), true);
    await owner.acknowledgementsSettled();
  };
  const read = async id => {
    const value = await owner.snapshot(id);
    generation = value.generation; observer = value.observer;
    return value;
  };
  const staleRelease = async lease => {
    let rejected = false;
    try { await invoke("engine_hold_observer", { sessionId, generation, observerLease: lease, held: false }); } catch { rejected = true; }
    require(rejected, "实际IPC接纳旧lease release");
    events.push({ event: "stale-release-rejected", generation, lease });
  };
  return {
    host: "linux-tauri",
    capabilities: () => bridge.capabilities(),
    async create(setup, seed) {
      owner = await bridge.create(setup, seed); sessionId = owner.sessionId; generation = owner.generation();
      await listen(); await subscribePaused();
      latestRow = await read("initial");
      require(latestRow.tick === "0" && latestRow.paused, "实际start/stop准备夹具期间已推进，不能冒充统一初态");
    },
    async observeBefore(id) {
      if (id === "without-observer") { await owner.disconnect(); await unlisten(); }
      if (id === "slow-observer") { accept = false; await listen(); await owner.reconnect(); }
      if (id === "prefix-failure") { accept = true; await subscribePaused(); }
      if (id === "new-lease") {
        await subscribePaused();
        oldLease = (await read(id)).observer.lease;
        await owner.refreshBaseline();
      }
    },
    async action(id) {
      if (id === "replaced" || id === "prepare-rejected") latestRow = await owner.restoreFromCurrent(id);
      else {
        const checkpoint = id === "duplicate-receipt" ? keys.durable : ["old-receipt", "bad-receipt", "old-generation"].includes(id) ? keys.failed : undefined;
        latestRow = await owner.inject(id, checkpoint);
      }
      require(typeof latestRow.generation === "string", "实际动作缺少代际");
      generation = owner.generation(); observer = latestRow.observer;
      if (id === "slow-observer") keys.durable = latestRow.archive.durable;
      if (id === "archive-full") keys.failed = latestRow.archive.queued[0];
    },
    async observeAfter(id) {
      await owner.acknowledgementsSettled();
      if (id === "old-lease") {
        const before = await read(id);
        await staleRelease(oldLease);
        for (const [command, args] of [["engine_ack_observer", { ordinal: "0" }], ["engine_resume_observer", { ordinal: "0" }]]) {
          let rejected = false;
          try { await invoke(command, { sessionId, generation, observerLease: oldLease, ...args }); } catch { rejected = true; }
          require(rejected, `${command}接受旧lease`);
        }
        const after = await read(id);
        require(JSON.stringify(before.observer) === JSON.stringify(after.observer), "旧lease操作改变新观察流");
      }
      if (id === "prefix-failure") {
        await waitFor(() => events.some(value => value.event === "engine-failure" && value.payload?.code === "STEP_FATAL"), "terminal");
        require(events.filter(value => value.event === "engine-failure" && value.payload?.code === "STEP_FATAL").length === 1, "实际terminal必须恰好一次");
      }
    },
    async snapshot(id) {
      const row = latestRow?.lifecycle?.commandClosed ? latestRow : await read(id);
      return { ...row, client: { received, accepted: row.observer.accepted } };
    },
    async extension(id) {
      let before, after;
      if (id === "prepare-rejected-running-preserved") {
        await owner.inject("without-observer");
        accept = true; await owner.subscribe(() => true);
        before = await read(id);
        require(!before.paused, "健康旧owner必须实际运行");
        const slot = structuredClone(await owner.host.save()); slot.setup.ticks_per_day = 0;
        let rejected = false;
        try { await owner.load(slot); } catch { rejected = true; }
        after = await read(id);
        require(rejected && before.generation === after.generation && !after.paused, "actualhost prepare拒绝改变旧owner运行意图");
        latestRow = after;
      } else if (id === "restore-postcommit-io") {
        await owner.inject("without-observer"); before = await read(id);
        after = await owner.postcommitBaselineFailure(); latestRow = after;
        require(after.generation !== before.generation && after.terminal === "CATALOG_BASELINE_FAILED", "actualhost未保留已提交baseline故障");
      } else if (id === "terminal-queue-full-prefix-or-gap") {
        await owner.holdObserver();
        latestRow = await owner.inject(id);
        await waitFor(() => events.some(value => value.event === "engine-failure" && value.payload?.recoverable === false), "队列满terminal");
        await waitFor(() => owner.evidence().some(value => value.event === "host-failure" && value.value?.recoverable === false), "actualhost接纳held终态");
        const terminal = events.find(value => value.event === "engine-failure" && value.payload?.recoverable === false);
        require(terminal.payload.missing !== undefined || /未送达|未收到|缺口/.test(terminal.payload.message), "队列满terminal必须明确遗漏前缀");
        const held = await read(id);
        require(held.observer.held, "terminal必须在真实held期间送达");
        // terminal后的held流由实际dispose收尾，不伪造尚未resume/fence的release。
      } else if (id === "stale-release-same-generation") {
        before = await read(id); await owner.refreshBaseline(); after = await read(id);
        require(after.generation === before.generation && after.observer.lease !== before.observer.lease, "实际host未重挂同代新lease");
        await staleRelease(before.observer.lease);
        latestRow = await read(id);
        require(JSON.stringify(latestRow.observer) === JSON.stringify(after.observer), "迟到release解锁新lease");
      } else if (id === "disconnect-resync-stale") {
        accept = true; await subscribePaused(); await owner.inject("manual");
        await waitFor(() => events.some(value => value.event === "engine-event" && value.payload.publication_ordinal !== undefined), "旧流真实包");
        await owner.acknowledgementsSettled();
        const old = events.find(value => value.event === "engine-event" && value.payload.publication_ordinal !== undefined);
        await owner.disconnect(); await owner.inject("manual");
        await owner.reconnect(); before = await read(id);
        const acceptedBefore = owner.evidence().filter(value => value.event === "host-accepted").length;
        const staleBefore = owner.staleDeliveries();
        await invoke("plugin:event|emit", { event: "engine-event", payload: old.payload });
        await waitFor(() => owner.staleDeliveries() > staleBefore, "实际host拒绝重放的旧lease包");
        await owner.acknowledgementsSettled();
        after = await read(id); latestRow = after;
        require(acceptedBefore === owner.evidence().filter(value => value.event === "host-accepted").length && before.observer.accepted === after.observer.accepted, "重挂后旧包被实际host接纳或ACK");
      } else if (id === "shutdown-known-archive-failure-cleanup" || id === "shutdown-pending") {
        latestRow = await owner.inject(id);
        require(latestRow.lifecycle.commandClosed && latestRow.lifecycle.archiveLaneReleased, "实际shutdown未清理owner");
        if (id === "shutdown-known-archive-failure-cleanup") require(latestRow.evidence?.shutdown?.kind === "known-archive-failure" && latestRow.archive.blocked, "已知归档失败被伪装durable成功");
        else require(latestRow.evidence?.pendingBefore?.length && latestRow.archive.queued.length === 0 && latestRow.archive.durable, "pending关闭缺少真实durable回执");
      } else throw new Error(`未接线Desktop扩展：${id}`);
      generation = owner.generation(); observer = latestRow.observer;
      return { before, after, row: latestRow, phase: id === "restore-postcommit-io" ? "baseline" : undefined, events: structuredClone(events), actualHost: owner.evidence() };
    },
    transportEvidence: () => ({ kind: "real-linux-tauri-window-actual-host-ipc", events: structuredClone(events), actualHost: owner?.evidence() }),
    fork: async () => createTauriAuthorityProductPort(),
    async close() {
      const errors = [];
      try { await unlisten(); } catch (error) { errors.push(error); }
      if (owner) {
        try {
          await owner.close();
          if (latestRow?.evidence?.shutdown?.kind === "known-archive-failure") errors.push(new Error("已知非耐久关闭被actualhost.dispose伪报成功"));
        }
        catch (error) {
          const expected = latestRow?.evidence?.shutdown;
          if (error?.kind === "confirmed-nondurable-shutdown" && error.receipt?.generation === latestRow?.generation && error.receipt?.lifecycle?.commandClosed && error.receipt.lifecycle.archiveLaneReleased && expected?.kind === "known-archive-failure") {
            events.push({ event: "expected-nondurable-dispose-error", message: String(error), confirmedShutdown: latestRow.lifecycle });
          } else errors.push(error);
        }
      }
      if (errors.length) throw new AggregateError(errors, "实际tauri-host会话清理失败");
    },
  };
}
