# 后续性能路线

- 首批资源成功缓存的红灯与绿灯已完成。1 核前 1000 tick 从 101.6x 到 218.3x，但 16 核含连续竞价 3000 tick 仅从 78.2x 到 81.5 / 83.1x，不足以完成目标。
- time -v 显示 16 worker 实际 CPU 使用约 225–228%，系统时间约 23s / 36s wall，voluntary context switches 54 万、involuntary 550 万。这是调度开销证据，不能仅以配置 16 threads 宣称满核计算。
- 首批旧 probe 用 pool.install 包含整轮 host coordinator，使其消耗一个 Rayon worker；已改为 global pool + host 调用线程，后续 baseline/优化应统一纠正后的入口，不能把改探针的收益冒充产品优化。
- Browser probe 从 HostUpdate.type=='protocol' 的 TickBatch.frames 计市场提交，不用 host.tick() 的 cached baseline 值；排除休市日 warmup，再独立计算 ticks / wall。旧错误探针的空样本已覆盖，不作证据。
- 继续测资源快照缓存命中与 cache misses 成本，625 page 的 Rayon trivial tasks 应考虑先查 cache、仅并行计算失效页（必须保留并发和错误显式报告）。
- 下一大项：live ledger 每 tick 多次深 clone、多次全量 conservation/evidence 校验、rebase，及普通股票输入反复 project / compare 全盘口。考虑不可变 page COW + 以 mutation stamp 缓存已验证事实；所有 mutation 必须使 cache 失效，严禁直接删门禁。
- 原生 NPC stream 每 tick 新建 std::thread consumer，也应测试可复用或 Rayon in_place_scope，在保持完成即受理和玩家/NPC ingress 语义的前提下减少 spawn / barrier。
- Browser 逐 tick task yield 必须保留。使用 MessageChannel 代替 zero-delay 嵌套 timers 可消除 timer clamp，先做取消/停止/消息接收/credit 边界测试，保持一个 task 一步的已有规则。
- 独立 reviewer 是一次性 Luna high；待读其 findings 修复、复核，然后本地聚焦提交首批。无需 push。目标保持 active，1000x 未完成。
