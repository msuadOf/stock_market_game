# 当前工作总账（防遗忘单一入口）

> 用途：**所有要做/在做的事的唯一登记处**。任何会话（包括未来的我）接手前必读本文件，完成后更新本文件。
> 建立：2026-10-17（用户指示："把目前要做的东西都维护在一个文件里，防止你再忘掉"）。
> 规则：开工前先读、有变化就改、完成就勾；数字必须带出处路径。

---

## 主线：std-thread 模型迁移落地（用户已拍板："让他落地"）

**目标**：用户 10-14 提出的 std::thread 架构（一股一持久线程 + tick 内决策线程 + join + 各股并行撮合 + 单线程收束，纯 std 无 rayon）从原型落地到生产，最终生产口径 ≥900x（用户口径：900x≈1000x 达标）。

**位置**：worktree `.worktree/std-thread-model`，分支 `exp/std-thread-model`。
**开关**：`ENGINE_STEP_RUNTIME=1` 启用新 desk runtime；当前缺省仍走旧管线；最终通过"翻转批"改缺省。

### 为什么值得做（原型数据，2026-10-14 晚，`.tmp/std-thread-proto/bench-*.json` + design.md）

- 原型串行决策 **982 t/s ≈ ~980x**（用户记的"900+x"即此）；16w 常驻 755 t/s；对照生产 rayon 16w 仅 ~359 t/s（≈355x）。
- 结论：瓶颈在 rayon 调度底座与所有权结构，不在业务数学；决策线程越多越慢（线程起停/通信税）→ 支持按股所有权+常驻线程。
- 边界（勿夸大）：原型复用真实策略内核/金额/撮合语义，但 NPC 按股归属只看本股（散户随机选股退化为单股）、无生产管线证据/校验/存档/公司域负担——是上限信号，不是生产达成。

### 当前进度（2026-10-17 01:10 更新：D5 六红已清零）

- **`d2116991`（10-17 01:09，"D5 终程清零——desk 买腿费用记账+镜像档 save/restore 链"）已把 env=1 六红全部清零**。本会话（d68ee0d0）在提交前对 WIP 内容实测：`ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib` = **1605/0/11 全绿**，与默认口径持平。
- **注意：迁移由另一条活跃会话实时驱动**（01:09 仍在提交）。本会话遵守单所有者规则**不碰该 worktree 的文件**，只做读端验证与独立复核。
- 之前状态：默认 1605/0/11 绿；翻转批 5/5 绿；attention 真迁（c1e5dd62）；wasm 方向 A 落地。
- 交接文档链（按时间读）：`.tmp/handoff-d5-finish.md` → `handoff-d5-11th-finish.md`（§4 开工位、§5 已知坑）→ `handoff-d5-11th-r2.md`。
- 另有 stash@{0} "d2-wip-player-routing"（D2 期 WIP，未处理）。

### 落地队列（顺序执行）

1. [x] env=1 六红清零（d2116991，另一会话完成；本会话独立实测绿）
2. [ ] **独立复核：6e265f92..d2116991 全批**（11 任 8 提交 + 12 任 3 提交，均未复核，翻转前硬门禁）——协调会话（d68ee0d0）曾于 10-17 01:10 派只读复核 agent，因用户结束该会话于完成前中止，**需重新指派**（复核范围与要点见上文"REVIEW 指南"性内容可参考 git log）
3. [ ] wasm CI 面核实（ci.yml 的 wasm 构建，本地 check 已过）
4. [ ] 翻转批：ENGINE_STEP_RUNTIME 缺省改 true（runbook `.tmp/flip-runbook.md`；pin 判定两式须统一为 step_runtime_enabled()；注意 var_os"存在即跳过"vs =="1"语义差）
5. [ ] simulation-diagnostics 编译修复（c79f051c 起 17 错）
6. [ ] 正式基准重测（production-server-probe 普通二进制、默认配置、3000 tick、多跑均值）→ 对比 ~359x 基线，向 ≥900x 逼近
7. [ ] 每批改动后独立 subagent 复核（门禁铁律）

### 迁移期登记债（翻转后处理）

- 散户经历成交面迁移期简化（登记于 docs/trading-rules.md）
- views.rs bound_fallback（±10% 近似，待 desk 广播投影收口）
- 镜像/权威标志判定残留（旧管线空簿+ledger 非局=自身不变量破坏形态跳过交叉核对）

---

## 并行在途（性能主线遗留，worktree `.worktree/spine-simplify-01` + `engine-max-performance`）

- [ ] **#67 按需 worker 两缺口**：260 线程泄漏 + burst 配额（台账 `.worktree/engine-max-performance/agents/engine-max-performance/current-state.md`）
- [ ] **#68 串行脊化繁为简**（Astra 循环）——大部分正被 std-thread 迁移取代，需重新评估剩余价值
- [ ] **crossbeam 调查**（用户 10-14 建议）——`.worktree/spine-simplify-01/.tmp/crossbeam-investigation/` 目前为空，未开工；若 std-thread 迁移完成则此题自动了结（纯 std 方案已弃 rayon）
- [ ] 宽度税 128w（rayon ROUNDS_UNTIL_SLEEPY=32 硬编码 561ms 税底；用户已否决池宽封顶）——同上，std-thread 落地后作废
- [ ] 拍卖增量复用（P0 实测 +5~8%，oracle.patch 在 `.tmp/expl-auction-p0/`）——可与迁移并行评估

## 关键事实速查（防再忘）

- **900+x 的出处**：std-thread 原型串行 982 t/s（`std-thread-model/.tmp/std-thread-proto/bench-s5-serial.json`），换算 1x≈1 tick/s → ~980x。生产正式口径现状 **~352-359x**（10-13/14）。
- 加速比换算锚：3000 tick ÷ wall 秒 ÷ 1 tick/s（如 142.419x = 21.06s）。
- CPU 天花板证据：rayon 16w 只吃 ~6.7/16 核（42%），贵活在串行脊柱。
- 性能总台账：`.worktree/engine-max-performance/agents/engine-max-performance/current-state.md`（最新节在文件顶部"追加"区）；memory/`project_engine_perf_1000x.md` 为主线 memory。
- 全部 worktree 地图：`git worktree list`（spine-simplify-01=任务#67/68 家、std-thread-model=迁移家、engine-max-performance=性能主线家）。

## 环境与流程坑（防再踩）

- `pnpm` 经 corepack 在 Node 25 下启动失败（ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING）——用 `node`/`cargo` 直跑等价命令。
- 本机 `$PWD` 是符号链接（/home/baiyifan/data1 → /data1/...）——跑需路径校验的脚本用 `realpath` 结果；escrow perf-harness 测试需 canonical TMPDIR。
- 全回归 = `node scripts/run-full-regression.mjs`（5 分钟预算）；runner 仅 harness 级异常走 exit 1，断言失败会打印用例。
- 定时任务 ×2（12:00 日报 / 19:00 推进，`f28cd04a` 会话建，10-17 前后 7 天过期）——过期需重建。
- 复核门禁：每批改动由未实施 subagent 独立审全 diff（大 A 语义/最小范围/断言未弱化）；有效发现修复后复审。
- 提交规范：Conventional Commits（type(scope): subject 中文）；不 push 不发版除非用户确认。
