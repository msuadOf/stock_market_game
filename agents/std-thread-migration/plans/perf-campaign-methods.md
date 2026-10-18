# 性能战役方法学手册（P0–P2 固化版）

> 依据：std-thread 迁移性能恢复战役（2026-10-17）。战役把 A 口径（runtime_s5_probe，3000 tick/10 组×2000 NPC/5 股）从 71-105 t/s 恢复到 mean 1358（19.2×，超历史基线 663.7 达 2.05 倍）；B 口径（driver_semantics_probe，300 tick/1 组/20000 retail NPC）从 4.3（1000 tick 恶化至 3.18）恢复到 78.1（1000 tick 69.5，21.9×）。
> 证据源：`agents/std-thread-migration/evidence/perf-p1/`、`evidence/perf-p2/`、`reviews/sol-perf-attribution-verdict.txt`、`.tmp/handoff-perf-p2-continuation.md`。
> 用途：后续性能战役与新人的操作手册。每把刀、每条守卫都标注战役实例，照抄即用。

---

## 0. 战役骨架（一页流程）

```
差分 profiling（P0：基线 worktree + 同参数双测）→ 嫌疑点对表（预估 vs 实测）
→ 按实测占比排刀序（预排序只是先验）
→ 逐刀：数据先行 → 突变红 → 修 → 等价（golden/逐位）→ 测量（≥3 轮+长跑）→ 落库归档 → 报数
→ 独立 subagent 复核 → 修复阻断 → 终审关闭
```

关键认知（P0 实证）：wall 落差 = CPU 功放 × 并行度坍塌。本战役 6.7× 落差 ≈ 3.1× 功放 × 2.2× 坍塌——**只看 wall 不看两者分解会选错刀**。

---

## 1. 工具链：差分 profiling 与测量纪律

### 1.1 差分 profiling（P0 定刀法）

**核心思想**：不猜。把当前 HEAD 与历史好基线在**同一参数、同一探针**下各采样一次，逐符号对比占比，让嫌疑点从数据里站出来。

1. **基线 worktree**：`git worktree` 建历史好版本 detached worktree（本战役 `.worktree/perf-baseline-5412fd10`），**保留不删**，供后续复测与配对 A/B。
2. **同参数双测**：两侧同探针、同 tick/组数/NPC 参数、同构建形态（`CARGO_PROFILE_RELEASE_DEBUG=1` release 带符号，否则 perf 无符号可看）。
3. **采样**：`perf record -F 499/999 --call-graph dwarf,16384`（样本量数万级；perf data 存 `.tmp/perf-records/`，1GB+ 不入库，方法与结论入档即可复现）。
4. **定标**：`perf stat` 取 task-clock / context-switches / 内核态周期占比，与探针吞吐（t/s）交叉验证；诊断计数用探针临时输出（用后还原）。
5. **嫌疑点对表**：把专家（Sol/Astra）预估值与实测值逐项对表。本战役实测把预估"P3 低优先级"的逐笔 Accepted 广播提为**首位**（协调者 28 CPU·s 满负荷、4618 ctx-sw/tick、87% 内核周期）；预估"A 首要 40-65%"的 MarketView 重建实锤但次序第二。**预估只作先验，刀序以差分实测为准。**

**降级链**（一层失效换下一层，如实登记口径）：

```
perf record --call-graph dwarf（用户态调用树）
  → dwarf 断栈（P2 实况：该二进制 unwind 断）→ perf report 周期加权叶帧 + 代码结构对照
  → 仍不够 → perf stat KPI 定标 + 探针诊断计数
  → samply（备选 profiler；本战役未实际使用，登记为备选项）
```

**环境前置**（机器重启会还原，重跑前必查）：
```
sudo sysctl -w kernel.perf_event_paranoid=1 kernel.kptr_restrict=0
```

### 1.2 测量纪律

| 纪律 | 内容 | 战役实据 |
|---|---|---|
| 净机 | 正式数字独占时段跑；机器 load 高（如 15min load 14.56=回归余温）时数字只登记不归因，接手者先净机复测 | P2 复核修复轮 300 tick 波动 53-75 无法归因 |
| 静置 | **编译后立即跑探针必现慢轮**（50s 16 线程编译后 52.9 慢轮，baseline/changed 都复现）——正式测量须静置 | p2-3a-green.txt |
| 同窗配对二进制 | 免编译余温：baseline 与 changed 两份二进制**交替执行**同轮窗口（A/B/A/B），效应在噪声边缘时以配对口径（如 perf stat 循环数中位增量）仲裁——对调度噪声不敏感 | p2-3a：+5% t/s 边缘效应由循环数 −5.8%（5/6 轮负）确认 |
| 噪声带 | 300 tick 轮间波动 ±15% 属常态（68.6-83.6 都出现过；k8s/containerd 背景周期性干扰）——小于 ±15% 的单轮结论不作数，≥3 轮记 mean/min/max | p2-3a-green.txt |
| 离群轮 | 偶发离群轮如实注明，不剔除不隐藏 | 6.13 记录、p2r1 53.1 离群 |
| 双口径 | A（裸 World）与 B（全语义 session/driver）分开报，禁止混一个百分比；每刀 A 探针抽查 1 轮——**A 不该变，变了=语义回归** | P2 全部改动 A 维持 1358 |
| KPI 惯例 | perf stat 取 task-clock + context-switches；性能红 left/right=修复前后 KPI 数值 | perf-p0-diff-profile.md |
| 探针覆盖盲区 | B fixture 单日不跨日结（tpd=15300）→ 跨日结行为用小 tpd fixture 或长跑验证；300 tick 全在集合竞价申报窗≠连续竞价 | p2-review-fixes、Sol 判定书 |

**探针命令（战役口径，照抄）**：
```
# B（全语义；events 期望 300:3336 / 1000:11119，fees 0/5705）
ENGINE_STEP_RUNTIME=1 ./target/release/examples/driver_semantics_probe .tmp/driver-semantics-setup.json 300 16 1
# A（裸 World；成交量 6438600/费用桶 647541185/冻结 6438600 逐位）
./target/release/examples/runtime_s5_probe 3000 10
# 回归双绿（默认 + runtime 翻转）
cargo test -p engine --lib && ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib
```

---

## 2. 修复模式库（五把刀：何时选哪把）

每把刀 = 机制 + 画像特征（profiler 里看到的指纹）+ 战役实例与实测收益。

### M1 按组批发（消息条数×组数 → 条数×1）

- **机制**：逐笔事实发给全部组（O(Accepted×G) 次 send）改为按 owner 分桶、一次 `Vec` 批消息/组/轮，消除数千→数次 send。
- **画像特征**：**协调者/发送中枢线程满负荷单线程**（本战役 28 CPU·s≈wall 关键路径）；**内核态周期占比翻盘**（87.1% vs 基线 11%）；**ctx-sw/tick 数千次**；叶符号=futex_wake/select_idle_sibling/ttwu/mpmc send；每次 send 到空闲线程=futex_wake+挑核+IPI+两次调度器穿越+记账 ≈7μs。
- **实例**：P1-A（accb6156）。t/s 71-105 → 517.8。
- **语义守卫**：消息仅合批、顺序与内容不变——同种子受理/成交/事件序列逐字段等价。

### M2 每 snapshot 共享不可变视图（公共计算上移一份）

- **机制**：每个 NPC 重复构建的公共只读数据（MarketView/attention signal/链位观察+聚合+paths）上移为每轮/每 snapshot 构建一次，各 NPC 共享引用。
- **画像特征**：**组线程用户态计算**里同一构造函数被高频重复调用（本战役 build_market_view 每 due NPC 建一次+decide 再建一次，构造次数 3.4× 放大；基线里它已占 22%）；深树复制/历史复制符号集中在公共数据结构。
- **实例**：P1-B（9a4dd018，A 口径 +142%：517.8→1253.5）；P2-3a（93752be5，链位观察同式，B +5%）。
- **边界**：groups=1 无广播放大+链位不走共享路径时收益持平（如实登记）；并行段已消后单人共享放量的墙钟收益被串行段稀释（见 §4 决策规则）。
- **语义守卫**：RNG 消费顺序不变（signal/market 构建不消费 RNG）——同种子决策序列逐字段等价硬验收。

### M3 dirty 掩码 + 稀疏上行（只搬变了的）

- **机制**：全名册深拷贝上行（EchoBelief/EchoAccounts 整组 clone）改为：生产演进面标记 dirty → 组侧只上行 dirty 条目 → 接收侧增量缓存/只装 dirty 位。等价不变式：**非 dirty ⇒ 组端值==session 镜像值**（构造期同源投影+两侧只经 dirty 通道同步）。
- **画像特征**：**深拷贝+分配符号占比过半**——本战役 P2 P0 画像：BeliefEntry::clone 8.87% + BTreeMap clone_subtree ≈21.8% + memmove 6% + dying_next ≈9.1% + jemalloc 家族 ≈12% ≈ **58% 全在一条个人态投影安装链**；驱动器线程独占 wall（72.8%）。
- **实例**：P2-1a 个人态 dirty（4d2ebd6c，B 5.02→10.1，+101%）；P2-2a EchoBelief 稀疏上行+增量缓存（0a9379dd，B 10.4→65.6，**+530%**——组线程全量 clone+缺页税消除）；P2-1b 账户回写事实 dirty（821a5b27，B +3%，收益小也如实登记——大头在组侧上行）。
- **坑**：跨 Start 的注入窗口语义——InstallPersonal 的 dirty 要由 pending 集承载到上行消费（对齐 pending_force_due 模式）；日结后强制全量一次覆盖无事实事件面（Drain/T+1/AdminBatch）。

### M4 写时 undo 替代全量快照（回滚合同不降级）

- **机制**：baseline 每拍全名册 clone 退役；在每域的 runtime 写入口**写前记录首写前值**（幂等判重），失败回滚消费 undo、成功尾清。ADR-0017 回滚合同（本拍成功立即可见完整结算、失败不泄漏部分状态）保持不变。
- **画像特征**：baseline 全量 clone 在画像里成片（AccountPagedMap get_mut/get + clone_subtree + 缺页）；**写入点少而集中**（每 tick 实际写入的账户远小于名册）。
- **实例**：P2-2b 账户域（1cb87aab，B 65.6→78.1，+19%）。后续候选：npc_attention/belief_participants/retail_experience/history_reads 等其余 baseline 域照抄同式。
- **坑（复核实战打回过）**：
  1. **直改面漏记**——绕过统一写入口的直改（如日结 `unlock_t1_positions` 直改 state.accounts，day_end.rs:52）会让 undo 缺口。对策：低频拍（日结拍）恢复一次全量快照兜底（`(tick+1)%tpd==0` 预测下一拍）。
  2. **判定基准差一拍**——日结判定基准是 **step 后 tick**，与直觉差一拍；fixture 循环次数同理（红证循环 4→3 才对）。
  3. **接线缺口**——flag 只在手动路径设置、生产路径不设（force_full_accounts 教训）。

### M5 镜像重建 + heap 快照还原（消无界增长）

- **机制**：每 tick 只进不出的结构（attention heap 全名册重压、无人消费）改为每 tick 从权威源**镜像重建**（O(N) 恒定、恒≤名册）；回滚面若重建不安全（旧管线/混合路径基线可含陈旧项），baseline 保留**快照逐项还原**（clone O(N) 恒定）——先消无界增长，O(N·T²) 大头另刀再消。
- **画像特征**：**长跑随时长恶化**（300→1000 tick t/s 恶化 25%：4.26→3.18）；结构项数随 tick 单调涨（heap 6 项 > 名册 1 的直接红证）；画像里 enqueue/clone 该结构的符号随时长放大。
- **实例**：P1-C（78e265e7，B 1000 tick 3.18→5.24 不再恶化，同长快 65%）。
- **坑**：首次尝试"回滚从权威重建"被 hash 守卫打回（陈旧项混入 business_state_hash 红）——**hash 守卫有判别力，别绕**，改为快照还原。

### 快速选刀表

| 画像指纹 | 选刀 |
|---|---|
| 协调者满负荷 + 内核周期翻盘 + ctx-sw/tick 数千 | M1 批发 |
| 组线程用户态同一公共构造高频重复 | M2 共享视图 |
| clone/memmove/jemalloc 深拷贝链 >50%，全名册上行 | M3 dirty+稀疏 |
| baseline 全量快照成片、写入点少 | M4 写时 undo |
| 长跑恶化、结构单调增长 | M5 镜像重建 |

---

## 3. 守卫模式（等价性与判别力）

性能战役多为**等价重构**——修复前后行为面逐位一致，不存在自然的行为红。守卫体系回答两个问题：**测试真能抓住回归吗（判别力）**、**行为真的没变吗（等价）**。

### 3.1 突变红（判别力实证）

- **做法**：绿态守卫写好后，**把修复禁用（突变：no-op/取反/移除过滤）→ 守卫必红**，红日志含命令/基准 SHA/left/right 归档。突变无独立提交，内容在红证文件头部注明。
- **实例**：P1-A 吞批突变→"组 0 的受理事实必须下行"panic；P1-B 判定取反→golden left=[全 0 挂簿] right=[1,2,2,3,3]；P2-1a dirty 过滤移除→left=8 right=0。
- **要点**：突变要打在**修复的核心机制**上（吞批/取反/移除过滤），不是随便改一行。守卫最好**双向定向**（每组必有本组受理+owner 归属核）。

### 3.2 golden 逐位等价

- **小 fixture golden**：单组确定性 fixture（固定 seed，如 base=0.3、seed=12345），逐字段锁定 cash/fees/volume/shares/decides/挂簿节奏。守卫的是**决策序列输出**。
- **大规模逐位**：探针终态逐位一致——A 口径成交量 6438600/费用桶 647541185/冻结 6438600；B 口径 events/fees（300:3336/0，1000:11119/5705）。**每刀必带、全程保持。**
- **如实登记判别边界**：某突变对该种子恰好鲁棒（RNG 两序 observe 集合相同）——如实登记为边界而非测试缺陷；突变零判别时（paths 置空——决策者全带持仓早退）**换腐蚀面**（行情价腐蚀显红）。
- **B 探针断言边界**：B 只断言股份相等+现金不增，无独立 fee sink 对账——"守恒全绿"应记为**该探针断言通过**，不等同 A 的严格对账（fee sink 强对账为登记欠账）。

### 3.3 长跑不退化判据

- **判据**：300→1000 tick 的 t/s 衰减 ≈ 事件密度增长（3336→11119，3.3×）为**自然**；衰减显著超出 = 存在随时长恶化面（heap 型教训：修复前 4.26→3.18 恶化 25%）。
- **流程**：每刀 300 tick ≥3 轮（mean/min/max）+ 1000 tick 一轮；修复后不恶化（5.02→5.24）即过。

### 3.4 宽 contains 反例教训（血泪）

- **事故**：P2 复核修复首版判定错拍（日结拍快照时机差一拍），红证断言用 `contains("日结")` 宽匹配——fixture 实际落进 run_ticks 自动日界 ? 路径，"日结"字样**误命中**，快照禁用突变下测试**仍绿 = 零判别**。复审突变实证打回，返工才修对（循环 4→3、断言改精确匹配）。
- **规则**：
  1. 红证断言**禁止** `contains("关键词")` 式宽匹配，锁定 location/结构**精确匹配**；
  2. 每个守卫都要跑一次"禁用修复"突变确认**真的会红**，绿证≠有判别力；
  3. hash 守卫（compute_context）有判别边界：**零写入拍对 undo 类突变不可判别**（受理只挪 purse、公开现金不变）——需要写入面的守卫用**必写入形态**（force_full 首拍）或结构断言。

### 3.5 回归与复核

- 全量回归**双绿**：默认 + `ENGINE_STEP_RUNTIME=1` 各跑一遍（战役尾段 1613/0/11×2）。
- 既有 flake 家族（旧管线 rayon 域、disconnected_backend 等）：失败先重跑+单测隔离复现，再 stash 对照原代码定性；**如实登记，不为过批改测试**。
- 每刀独立 subagent 复核（未实施者审完整 diff），阻断修复后**突变复验**再终审；测试同义反复等遗留项降级登记。

---

## 4. 决策规则

### 4.1 占比不符换刀（预估 vs 实测）

- 画像预估收益与实测显著不符 = 信号，不是麻烦：P2-3a 画像（Vec::clone 12.8%+memmove 9.4%）预估 15-40%，实测 +5%——解读为**该面（组线程）已消，收益被并行段稀释，驱动线程（get_mut 5.7%/缺页 ~11%/jemalloc ~17%）才是下一段关键路径**。据此登记后续候选并换刀，不恋战。
- 刀序本身也以差分实测为准：Sol 预估把广播列 P3"低"，差分实测提为 P1 首位（Sol 严重低估）。**预排序只是先验，每批开工前重采样确认嫌疑占比。**

### 4.2 收益小于预估如实登记

- P2-1b 账户 dirty 预估第二、实测 +3%——如实写"+3%，大头在组侧上行"，不粉饰不隐藏。负/小结果照常一件一提交、MANIFEST 归档。±15% 噪声带内的效应用配对口径（循环数）仲裁后如实标注"噪声边缘，配对口径支持小幅正收益"。

### 4.3 负结果归因流程（指针）

拿到负数/无收益**不许直接关闭方向**，先走：

1. CLAUDE.md「性能优化闭环」：负结果 → gpt-6-astra high 独立审查（方案方向错 vs 实现半截假失败 vs 测量问题）→ 实现问题修复重测，方案问题记录原因换设计。
2. memory/`feedback_negative_result_attribution.md`（无收益≠方向死：核查机制是否真生效/是否打中成本/测量是否公平）+ `feedback_verify_before_reject.md`（并行失败先 Codex astra 交叉验证）。
3. 本战役范本：`reviews/sol-perf-attribution-verdict.txt`——std-thread 迁移后 6.7× 落差，Sol 判定"**优先修复实现再复测，不判方向死刑**"：A 有公共计算重复（可核算：MarketView 构造次数 3.2-4.9× 放大）、B 有全量镜像搬运和 heap O(N·T²)，均为"不必削弱交易语义即可回收的实现浪费"；ADR 必要成本（原子提交、失败不泄漏、本拍可见）与实现选择（每拍深复制、每 NPC 重算）边界逐条分清。该判定直接催生了 P1/P2 全部五把刀。
4. 归因先修正口径再下结论（Sol 三纠偏）：探针是否跑完整决策链（A 无 belief/retail 链）、speed_x 定义（B 的 speed_x 就是 t/s，但 fixture 全程在集合竞价申报窗）、成交量是否全程累计量（volume_today 会 roll_day 清零）。

### 4.4 落库与报数纪律

- **一件一提交**（显式 pathspec，禁 `git add -A`）；红绿证据随批归档 `evidence/perf-p1|p2/`，MANIFEST 逐刀追加（文件/命令/红形态/绿形态/SHA）。
- 每刀报数：红绿+3 轮数字+回归态；收益小也如实报。
- stash 规程（共享栈）：push 必带唯一 tag、立即记 SHA、apply 不 pop、drop 前核对归属。
- ts_rs 生成物（`apps/web/src/types/generated/*.ts`）漂移不落库。

---

## 5. 每刀标准流程（checklist，照抄）

1. **数据先行**：动刀前 perf record 复采样确认嫌疑占比（§1.1；sysctl 前置）。
2. **突变红**：绿态守卫 + 禁用修复的突变显红，存红证（命令/基准 SHA/left/right；断言精确匹配）。
3. **等价**：全量默认 + `ENGINE_STEP_RUNTIME=1` 双绿；B 探针 events/fees 逐位（300 与 1000 tick 各一轮）；A 探针抽查（不该变）。
4. **测量**：B 300 tick ≥3 轮（mean/min/max）+ 1000 tick 一轮（长跑不恶化判据 §3.3）；净机、静置、必要时同窗配对二进制。
5. **落库**：一件一提交（显式 pathspec）；红绿证据归档 + MANIFEST 追加。
6. **报数**：红绿+数字+回归态报协调者；收益小如实报。
7. **复核**：独立 subagent 审 diff；阻断修复后突变复验再关闭。

---

## 附：战役账本（复用时的量级参照）

| 刀 | B 口径 t/s | 增量 | 提交 |
|---|---:|---|---|
| 战役起点 | 4.3（1000 tick 恶化 3.18） | — | — |
| M5 heap 镜像重建 | 5.02 | +17% | 78e265e7 |
| M3-1a 个人态 dirty | ≈10.1 | +101% | 4d2ebd6c |
| M3-1b 账户回写 dirty | 10.39 | +3% | 821a5b27 |
| M3-2a EchoBelief 稀疏上行 | 65.6 | +530% | 0a9379dd |
| M4 写时 undo | 78.1 | +19% | 1cb87aab |

A 口径：M1 批发 71-105→517.8；M2 共享视图 →1253.5（+142%）；P1 三件叠加终值 mean 1358。
