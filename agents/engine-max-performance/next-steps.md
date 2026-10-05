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

## 最新用户约束

- 每轮独立复核均新建，不复用 reviewer；减少轮次，稳定批次只审一次，有实质修复才新建下一位。
- Luna仅固定文件/固定算法/固定测试清单的小任务或快速检查；复杂根因/算法主agent负责。OpenAI官方说明已实读，见review-policy.md。
- 当前paged map初稿的retain多行删末尾group bug已由root真实红测修复；无关递归fmt已逐文件证明仅格式后还原78文件，保护实际代码。不复用被拒绝的git restore。
- 当前paged+稀疏rebase约99x；precommit从约5.8秒降到3.1秒，receipt aggregation从4.2秒降到2.8秒，但private stock ledger初始化/decision阶段变贵（10.5→13.2秒）。下一步直接构建paged rows，避免先BTreeMap再分页的双遍历/分配。
- 真正单局UI已测：16 page/Worker报告均16，MAX通过页面控件；103→696tick/10.007s=59.26x；Chrome全进程CPU约678%，预算42.4%。这和另建host无UI的28x是不同路径/样本，不混作配对收益。
- 新增外部宿主NPC结果消费线程测试脚手架正在build（session52554需实时核实）。目标从host caller消费，减少每tick std::thread::spawn；被Rayon worker调用的分支仍须保留防饥饿方式，1worker inline与既有并发session/fast-slow/Player ingress tests都不能删。
