// 实际浏览器中的Server HTTP+WebSocket适配；不得用于生产UI或模拟transport。
export function createNativeAuthorityProductPort(baseUrl) {
  const events = [];
  let frontend, market, bridge;
  let token, session, generation = "1", socket, lease, oldLease, oldCut, latestRow, lastObserver;
  let accept = false, serial = 0, received = "0", applied = "0", fatal;
  const responses = [], pending = [];
  const keys = {};
  const require = (condition, message) => { if (!condition) throw new Error(message); };
  const body = extra => ({ session_id: session, generation, ...extra });
  const request = async (route, data, method = data === undefined ? "GET" : "POST") => {
    const response = await fetch(new URL(route, baseUrl), { method, headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...(data === undefined ? {} : { "content-type": "application/json" }) }, body: data === undefined ? undefined : JSON.stringify(data) });
    const text = await response.text(); require(response.ok, `${route}: HTTP${response.status}: ${text}`);
    return text ? JSON.parse(text) : undefined;
  };
  const wait = (name, predicate = () => true) => new Promise((resolve, reject) => {
    const found = responses.findIndex(value => value[name] !== undefined && predicate(value[name]));
    if (found >= 0) { resolve(responses.splice(found, 1)[0][name]); return; }
    const item = { name, predicate, resolve, reject };
    const timer = setTimeout(() => { pending.splice(pending.indexOf(item), 1); reject(new Error(`真实WS等待${name}超时`)); }, 5000);
    item.resolve = value => { clearTimeout(timer); resolve(value); }; pending.push(item);
  });
  const send = value => { require(socket?.readyState === WebSocket.OPEN, "真实WS尚未连接"); socket.send(JSON.stringify(value)); };
  const receive = raw => {
    let value;
    try { value = JSON.parse(raw); } catch (error) { fatal = error; return; }
    events.push({ type: "received", value });
    const index = pending.findIndex(item => value[item.name] !== undefined && item.predicate(value[item.name]));
    if (index >= 0) { const item = pending.splice(index, 1)[0]; item.resolve(value[item.name]); }
    else responses.push(value);
    if (value.ObserverCut) { oldCut = String(value.ObserverCut.capture_id); lease = String(value.ObserverCut.observer_lease); }
    if (value.Baseline) { generation = String(value.Baseline.timeline_generation); lease = String(value.Baseline.observer_lease); }
    if (value.ObserverTerminal?.observer) lastObserver = value.ObserverTerminal.observer;
    const packet = value.PublisherFrame ?? value.PublicationFailure;
    if (packet) {
      require(typeof packet.publication_ordinal === "string" && typeof packet.observer_lease === "string", "V2发布缺少真实lease/ordinal");
      received = packet.publication_ordinal;

    }
  };
  const connect = async () => {
    const { createBridge } = await import("/src/host/remote-authority-contract.ts");
    market = await request(`/api/market/context?${new URLSearchParams({ session_id: session })}`);
    bridge = await createBridge(baseUrl, token, market, {
      accept,
      onNetwork(raw) { receive(raw); },
      onEvent(event) {
        events.push(event);
        if (event.type === "actual-remote-host-accepted") applied = event.value.publication_ordinal;
      },
    });
    frontend = bridge.host;
    socket = bridge.socket;
    require(socket.readyState === WebSocket.OPEN, "actualremotehost未建立真实WS");
  };
  const inspect = async () => {
    if (!socket || socket.readyState !== WebSocket.OPEN) { require(lastObserver, "断线前没有实际observer证据"); return lastObserver; }
    const requestId = String(++serial); send({ InspectObserver: { request_id: requestId } });
    const value = await wait("ObserverEvidence", result => String(result.request_id) === requestId);
    const evidence = value.observer ?? value;
    for (const field of ["generation", "lease", "sent", "flushed", "accepted"]) require(typeof evidence[field] === "string", `真实WS inspector缺少${field}`);
    lastObserver = Object.fromEntries(["generation", "lease", "sent", "flushed", "accepted"].map(key => [key, evidence[key]]));
    return lastObserver;
  };
  const waitAccepted = async previous => {
    const deadline=performance.now()+5000;
    let actual;
    while(performance.now()<deadline) {
      actual=await inspect();
      if(BigInt(actual.accepted)>BigInt(previous)) return actual;
      await new Promise(resolve=>setTimeout(resolve,5));
    }
    throw new Error(`实际frontend应用接纳未推进:${JSON.stringify(actual)}`);
  };
  const disconnect = async () => {
    if (socket?.readyState === WebSocket.OPEN) {
      await inspect(); const closing = socket;
      await new Promise(resolve => { closing.addEventListener("close", resolve, { once: true }); closing.close(); });
    }
    socket = undefined;
  };
  const capture = async (release = true) => {
    oldLease = lease;
    send({ CaptureObserver: {} }); const cut = await wait("ObserverCut");
    oldCut ??= String(cut.capture_id);
    lease = String(cut.observer_lease ?? cut.baseline?.observer_lease);
    require(/^[1-9][0-9]*$/.test(lease), "ObserverCut缺少实际lease");
    send({ FenceObserver: { capture_id: String(cut.capture_id), timeline_generation: generation, publication_ordinal: String(cut.publication_ordinal) } });
    const fenced = await wait("ObserverFenced", value => String(value.capture_id) === String(cut.capture_id)); require(fenced.accepted, "实际cut拒绝");
    if (!release) return;
    send({ ReleaseObserverV2: { capture_id: String(cut.capture_id), timeline_generation: generation, observer_lease: lease, publication_ordinal: "0" } });
    const released = await wait("ObserverReleased", value => String(value.capture_id) === String(cut.capture_id)); require(released.accepted, "实际lease release拒绝");
  };
  const secondaryObserver = async () => {
    const url = new URL("/ws", baseUrl); url.protocol = "ws:";
    url.search = new URLSearchParams({session_id:session,delivery:"push",observer_fencing:"true",observer_protocol:"2"});
    const extra = new WebSocket(url,["stock-game",`stock-game.auth.${Array.from(new TextEncoder().encode(token), byte=>byte.toString(16).padStart(2,"0")).join("")}`]);
    const packets = [];
    extra.addEventListener("message", event=>packets.push(JSON.parse(String(event.data))));
    const take = async name => {
      const until=performance.now()+5000;
      while(performance.now()<until) { const index=packets.findIndex(value=>value[name]!==undefined); if(index>=0)return packets.splice(index,1)[0][name]; await new Promise(resolve=>setTimeout(resolve,5)); }
      throw new Error(`第二真实observer缺少${name}`);
    };
    const baseline=await take("Baseline");
    return { packets, baseline, take, send:value=>extra.send(JSON.stringify(value)),
      async inspect(){this.send({InspectObserver:{request_id:"secondary"}});return take("ObserverEvidence");},
      async hold(){this.send({CaptureObserver:{}});const cut=await take("ObserverCut");this.send({FenceObserver:{capture_id:String(cut.capture_id),timeline_generation:generation,publication_ordinal:String(cut.publication_ordinal)}});const fenced=await take("ObserverFenced");require(fenced.accepted,"真实第二observer hold未安装");return cut;},
      close(){extra.close();}
    };
  };
  return {
    host: "native",
    async capabilities() { if (!token) ({ token } = await request("/api/auth/guest", {})); return request("/api/host-parity/authority-contract/setup"); },
    async create(setup, seed) {
      if (!token) ({ token } = await request("/api/auth/guest", {}));
      const created = await request("/api/new", { setup, seed }); session = created.session_id; generation = String(created.generation);
      require(created.can_control, "真实产品合同需要controller权限"); await request("/api/running", body({ running: false })); await connect();
    },
    async observeBefore(id) {
      if (id === "without-observer") { await inspect(); await bridge.dispose(); bridge = undefined; frontend = undefined; socket = undefined; }
      if (id === "slow-observer") { accept = false; await connect(); }
      if (id === "prefix-failure") { accept = true; bridge.setAccept(true); }
      if (id === "new-lease" || id === "stale-release-same-generation") { oldLease = lease; await frontend.refreshBaseline(); }

    },
    async action(id) {
      if (id === "prepare-rejected") {
        const before=await request("/api/host-parity/authority-contract/snapshot",body({id}));
        const invalid=structuredClone(await frontend.save()); invalid.setup.ticks_per_day=0;
        let failure;
        try {await frontend.load(invalid);} catch(error) {failure=error;}
        require(failure?.outcome === "rejected", "实际remote-host未接纳已证明未提交的HTTP拒绝");
        latestRow=await request("/api/host-parity/authority-contract/snapshot",body({id}));
        require(latestRow.generation===before.generation&&latestRow.tick===before.tick&&latestRow.seq===before.seq,"actualfrontend prepare拒绝改变旧代事实");
        events.push({type:"actual-remote-restore-rejected",outcome:failure.outcome,message:String(failure),before,after:latestRow});
        return;
      }
      if (id === "replaced") {
        const sealed = await frontend.save(); await frontend.load(sealed);
        latestRow = await request("/api/host-parity/authority-contract/snapshot", body({ id }));
        generation = latestRow.generation; return;
      }
      let checkpoint;
      if (id === "duplicate-receipt") checkpoint = keys.durable;
      if (["old-receipt", "bad-receipt", "old-generation"].includes(id)) checkpoint = keys.failed;
      latestRow = await request("/api/host-parity/authority-contract/action", body({ id, ...(checkpoint ? { checkpoint } : {}) }));
      require(latestRow && typeof latestRow.generation === "string", "实际action未返回状态"); generation = latestRow.generation;
      if (id === "slow-observer") keys.durable = latestRow.archive.durable;
      if (id === "archive-full") keys.failed = latestRow.archive.queued[0];
    },
    async observeAfter(id) {
      bridge?.assertHealthy();
      if (fatal) throw fatal;
      if (id === "old-lease") {
        require(oldLease, "缺少真实旧lease");
        send({ AcceptPublication: { timeline_generation: generation, observer_lease: oldLease, publication_ordinal: "0" } });
        const result = await wait("PublicationAccepted", value => value.observer_lease === oldLease); require(result.accepted === false, "旧应用ACK被接受");
      }
      if (id === "prefix-failure") {
        const terminal = await wait("ObserverTerminal"); require(terminal.failure, "terminal缺少真实故障");
      }
    },
    async snapshot(id) {
      const row = latestRow?.id === id ? latestRow : await request("/api/host-parity/authority-contract/snapshot", body({ id }));
      return { ...row, observer: await inspect(), client: { received, accepted: applied } };
    },
    async extension(id) {
      if (id === "prepare-rejected-running-preserved" || id === "restore-postcommit-io") {
        bridge.setAccept(true); accept=true;
        await request("/api/host-parity/authority-contract/action",body({id:"complete-day"}));
        await frontend.refreshBaseline();
        const sealed=await frontend.save();
        if (id === "prepare-rejected-running-preserved") {
          await frontend.start(update=>{events.push({type:"actual-remote-running-update",update});return true;},failure=>events.push({type:"actual-remote-running-failure",failure}));
          const before=await frontend.readSpeedMetrics(); require(before.running,"实际frontend未启动运行意图");
          const invalid=structuredClone(sealed);invalid.setup.ticks_per_day=0;
          let failure;try{await frontend.load(invalid);}catch(error){failure=error;}
          require(failure?.outcome==="rejected","实际frontend未区分precommit rejection");
          const after=await frontend.readSpeedMetrics();require(after.running,"prepare拒绝改变旧owner运行意图");
          // 真正成功的后续控制请求证明pending fence已因typed rejection正确解除。
          await frontend.stop();
          latestRow=await request("/api/host-parity/authority-contract/snapshot",body({id}));
          return {row:latestRow,before,after,failure:{outcome:failure.outcome,message:String(failure)},events:structuredClone(events),assertions:{actualFrontendRejectedBeforeCommit:true,runningPreserved:true,writeFenceReleasedAfterKnownRejection:true}};
        }
        const previous=generation;
        await frontend.load(sealed);
        const installed=await inspect();generation=installed.generation;
        require(BigInt(generation)===BigInt(previous)+1n,"实际frontend load没有接纳新代");
        const committed=await request("/api/host-parity/authority-contract/snapshot",body({id}));
        latestRow=await request("/api/host-parity/authority-contract/action",body({id:"postcommit-catalog-failure"}));
        const terminal=await wait("ObserverTerminal");
        const deadline=performance.now()+5000;
        while(!bridge.events.some(event=>event.type==="actual-remote-host-failure"&&event.value?.code==="CATALOG_BASELINE_FAILED")&&performance.now()<deadline) await new Promise(resolve=>setTimeout(resolve,5));
        require(bridge.events.some(event=>event.type==="actual-remote-host-failure"&&event.value?.code==="CATALOG_BASELINE_FAILED"),"实际frontend没有接纳独立catalog故障");
        let rejected;try{await frontend.setSpeed(1);}catch(error){rejected=error;}
        require(rejected!==undefined,"已提交后的catalog terminal未关闭实际frontend写门");
        require(latestRow.generation===committed.generation,"catalog失败伪回滚已提交代际");
        return {row:latestRow,phase:"separate-catalog",committed,terminal,writeRejected:String(rejected),events:structuredClone(events),assertions:{actualFrontendCommitted:true,committedGenerationRetained:true,actualFrontendTerminalWriteFence:true}};
      }
      if (id === "multi-observer-stalled" || id === "terminal-queue-full-prefix-or-gap") {
        const other=await secondaryObserver();
        try {
          if(id==="terminal-queue-full-prefix-or-gap") await other.hold();
          bridge.setAccept(true); accept=true;
          const before=await inspect(); await this.action(id);
          if(id==="terminal-queue-full-prefix-or-gap") {
            const terminal=await other.take("ObserverTerminal");
            require(terminal.missing && BigInt(terminal.missing.through)>=BigInt(terminal.missing.from),"真实held满队列terminal缺少完整缺口");
            const primaryTerminal=await wait("ObserverTerminal");
            require(primaryTerminal.failure && bridge.events.some(event=>event.type==="actual-remote-host-failure"),"actualremotehost没有处理真实terminal");
            return {row:latestRow,secondaryTerminal:terminal,primaryTerminal,events:structuredClone(events),assertions:{terminalGapObserved:true,actualFrontendHandledTerminal:true}};
          }
          const secondary=await other.inspect(),after=await waitAccepted(before.accepted);
          require(secondary.accepted==="0" && BigInt(after.accepted)>BigInt(before.accepted),"慢observer阻塞主observer或主应用未实际接纳");
          require(latestRow.archive.durable!==null,"多observer下实际归档未完成");
          return {row:latestRow,primary:after,secondary,assertions:{independentObservers:true}};
        } finally {other.close();}
      }
      if(id==="disconnect-resync-stale") {
        bridge.setAccept(true); accept=true;
        const seedBefore=await inspect();
        await this.action("manual");
        await waitAccepted(seedBefore.accepted);
        const before=await inspect(), replayOldPacket=bridge.retainStalePacket();
        await bridge.dispose(); socket=undefined;
        await this.action(id); await connect(); await frontend.refreshBaseline();
        const installed=await inspect();
        const rejection=replayOldPacket();
        require(rejection.wireKind==="PublisherFrame","断线旧包证据必须是实际更新，不能只用初始baseline");
        const after=await inspect();
        require(after.lease===installed.lease && after.accepted===installed.accepted,"旧handler包改变当前真实观察证据");
        require(Number.isSafeInteger(frontend.snapshot().seq) && BigInt(frontend.snapshot().seq)===BigInt(latestRow.seq),"actualremotehost断线恢复基线不是最新提交");
        require(events.some(event=>event.type==="actual-network-received" && event.value?.ObserverReleased?.accepted===true
          && event.value.ObserverReleased.observer_lease===after.lease),"重连未取得匹配当前lease的真实release接纳回执");
        return {row:latestRow,before,installed,after,rejection,events:structuredClone(events),assertions:{actualFrontendReconnected:true,oldPacketRejected:rejection.oldPacketRejected,actualReleaseAccepted:true}};
      }
      await this.observeBefore(id); await this.action(id); await this.observeAfter(id);
      if(id==="stale-release-same-generation") {
        const installed=await inspect();
        send({ReleaseObserverV2:{capture_id:oldCut,timeline_generation:generation,observer_lease:oldLease,publication_ordinal:"0"}});
        const rejected=await wait("ObserverReleased",value=>value.observer_lease===oldLease);
        require(rejected.accepted===false,"真实旧release被接受");
        const after=await inspect();
        require(after.lease===installed.lease&&after.accepted===installed.accepted,"旧release改变当前lease");
        return {row:latestRow,installed,after,rejected,assertions:{staleReleaseRejected:true}};
      }
      require(latestRow.assertions && Object.keys(latestRow.assertions).length && Object.values(latestRow.assertions).every(value=>value===true),`扩展${id}缺少真实断言证据`);
      return {row:latestRow,events:structuredClone(events)};
    },
    transportEvidence: () => ({ kind: "real-browser-websocket-v2", events: structuredClone(events) }),
    fork: async () => createNativeAuthorityProductPort(baseUrl),
    async close() {
      const failures = [];
      try { await bridge?.dispose(); socket = undefined; } catch (error) { failures.push(error); }
      if (session && latestRow?.registryRemoved !== true) { try { await request(`/api/session?${new URLSearchParams({ session_id: session, generation })}`, undefined, "DELETE"); } catch (error) { failures.push(error); } }
      if (failures.length) throw new AggregateError(failures, "真实Server产品会话清理失败");
    },
  };
}
