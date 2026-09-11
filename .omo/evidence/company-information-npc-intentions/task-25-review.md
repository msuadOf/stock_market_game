# Task 25 独立复核 — 扩展个人关注发现、保留与淡出

VERDICT: APPROVE (converted after remediation — F1 fix landed on-branch as `b2d88a9` (base 94205d1), verified at recovered branch tip `bea13b9`; the attempted `a885ac1` fix was an orphaned wrong-base artifact of the 09:30 reset incident, superseded by orchestrator recovery; original rejection preserved below)

- 复核对象：commit `94205d1` (`feat(engine): 以公开曝光扩展个体关注生命周期`)，branch `codex/feat/web-ui-polish`，parent `2d15e7c`（task-16，已 APPROVE）。提交面 8 文件全部 `packages/engine`（src 3：experience.rs +5 / experience/watchlist.rs 173 新建 / session/attention.rs +169 additive；tests 5：main.rs 441 / exposure.rs 206 / failures/discovery.rs 193 / failures/mod.rs 4 / failures/watchlist.rs 144），1334 insertions，**零 docs/fixture 改动**。
- 复核人：独立 subagent（未实施本改动）。AGENTS.md「大A语义与独立复核门禁」第 8 次强制执行。
- 隔离环境：`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-25 94205d1`（detached；`merge-base --is-ancestor` 验证 task 19/20/21 提交 4e830c5/b1f638d/6cdc219/61eb698 均为祖先，共 184 提交；task-18 未提交工作不在树内——`strategy/fundamental` 不存在、`fundamental_beliefs` 套件缺席隔离全量）。主树仅只读 git/文件读取，**零 cargo / 零 git 写**。
- 原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t25r-{1,2,3,4}.txt`（cmd /c 重定向）。
- **拒绝范围刻意收窄（task-12/13/14 同款）**：唯一阻断 = F1 同提交登记门禁（第 8 次强制）。engine 代码本身经全面核验**无需返工**；修复 = docs+fixture-only 跟进提交。

---

## 一、AGENTS.md 三问

### Q1 是否符合大 A 语义，依据是否可靠？——通过（本任务不触交易制度）

游戏内部注意力/信息架构改动，未触碰订单簿、涨跌停、T+1、费用等任何 A 股交易制度面；领域基线为用户批准的 K4 锚（plan L121–122 逐字核对）+ ADR-0016 §4（已批准决策，qualitative 模型锚）。逐条：

| K4/计划锚 | 实现落点 | 测试锁定 |
|---|---|---|
| 「公共曝光仅改变发现机会…只有实际观察才将内容纳入个人信息集」 | 曝光输入只是 `&BTreeSet<StockCode>`，由调用方从任务 16 `discovery_candidates` 公开面派生；`sample_discovery_stock` 输入面 = MarketView + 曝光集合，`NpcInformationState` 结构性不可达 | `discovery_sampling_never_writes_into_information_state`（1000 次抽样字节全等 + acquired_count=0）+ `mutating_only_private_operating_facts_changes_no_candidate_and_no_order`（私有总账过账前后 weights/候选序列/会话事件+存档字节全等） |
| 「未观察NPC不能直接读最新全部公告」 | 抽样只返回 StockCode 候选，获知仍须走 `record_acquisition`（任务 16 语义原样） | 同上；exposure.rs 头注 + 实现 make it structural |
| 公布前无前视 | `exposed_codes` 经 `discovery_candidates(library, company, as_of)` 派生；公布时点前 as_of ⇒ 空集 ⇒ 权重全 1.0 | `announcement_exposure_enters_weights_only_through_the_public_surface`（17:00 空 / 18:00 后仅所属股票 +2.0） |
| 「一次公共公告不让全体同步交易」（Acceptance） | 每 NPC 独立个体注意力 RNG 流（`NpcAttentionState.rng_state` save/restore，与 `evaluate_attention_candidate` 同款）；发现是概率加权不是广播 | `one_public_announcement_never_synchronizes_all_npc_candidates`（40 NPC × 25 抽，分布断言 P=0.75 ±5σ + quiet_count>0 硬断言 + 同种子逐次重放一致） |
| 「观察是在现有候选注意力被接受时发生」 | 接线显式移交任务 26（模块文档 + learnings 登记；本 diff 不触 session.rs/behavior.rs） | extraction_replay 3/3 绿（隔离全量内）⇒ 存量会话 RNG/字节锚零漂移 |

时间/身份数学全整数（u64 市场分钟、StockCode、BTreeMap 确定序）；f64 仅存在于概率/权重数学——与 attention 模块既有血统一致（`base_probability: f64`、原 0.60/0.70 字面量、`sample_attention_wait(probability: f64)`）。金额阈值比较在 cents 整数域取分子后转比例，无 f64 金额运算。

### Q2 改动是否为需求所必需、是否保持最小范围？——通过

计划任务 25 实施面 = 「修改`session/attention.rs`和`experience/watchlist.rs`」——实际恰好如此 + experience.rs 模块入口 +5（Rust 2018 布局接缝，pub use 3 类型）+ 5 测试文件。无 session/behavior 抢先接线（任务 26 职责，task-16 先例同款 scope cut：`select_discovery_stock` 是新公共发现面，未替换/未触碰 behavior `select_observed_stock`——8 文件清单核对，behavior/heuristics.rs 零改动）；无多余抽象（无 trait、无请求结构体、无 fallback 层）。`MAX_UNHELD_WATCHLIST_STOCKS=8` 复用 experience.rs:27 既有常量（任务 19 血统，K5 L135「持仓+8个未持仓」同一数值），不另立第二常量。serde 走私有 DTO + `from_parts` 恢复校验（PlanBook/PublicLibrary 先例）。

### Q3 是否存在遗漏的边界测试、跨层语义漂移或不必要复杂度？——无阻断项

20 测试（金样 14 + 拒绝 6）覆盖计划 QA 全部条目（见 §四映射）。跨层无漂移：engine-only；ts_rs 导出新增 2 个 lib 绑定测试（WatchedStock/PersonalWatchlist）；extraction_replay 3/3 绿。数值细节：`pick_weighted` 末位候选兜底注释明确（next_f64<1 ⇒ draw<total，浮点边缘归末位区间，非静默吞错）；base 恒 1.0 ⇒ total≥1 无除零；`abnormal_thirty_minute_move` <2 样本/首价≤0 显式无信号（缺样本不伪装异常，铁律二）；`abnormal_relative_volume` 非有限=无信号。

---

## 二、红线与 Acceptance 逐项核验（全部通过）

| 红线/Acceptance | 验证 | 结果 |
|---|---|---|
| 继承原候选概率/个体 RNG（60% 持仓优先 + 70% 关注列表 + 全市场） | `select_discovery_stock` 与 behavior/heuristics.rs:25–53 `select_observed_stock` 逐分支对照：同 0.60 门 + 均匀 `next_range_u32`（持仓分支不吃权重）/ 同 0.70 门（关注池改加权）/ 全市场（均匀改加权）；每分支 RNG 消费次数同构（门 f64 + 选择 1 次） | ✓（`held_priority_base_...` P=0.80/0.20、`held_selection_stays_uniform_...` P=0.5667 手算复核、`watchlist_bias_...` P=0.90/0.10 手算复核） |
| 原注意力测试不变且绿 | attention.rs 原函数零改动（diff 纯 additive）；behavior.rs 零触碰 | ✓（隔离全量：`attention_tests::*` 3/3、`initial_attention_candidates_...`、`held_stock_is_observed_...`、`market_discovery_can_select_...`、`watchlist_pruning_keeps_...`、experience 7/7 全绿） |
| 异常30分钟涨跌/相对量能/公告曝光提高发现权重 | 权重 1.0 基础 + 异常涨跌(\|Δ\|≥2%, 最近≤30 完整分钟, 涨跌对称) +1.0 / 相对量能(≥2.0, 非有限=无信号) +1.0 / 公告曝光 +2.0，最大 5.0 | ✓（`discovery_weights_pin_every_public_signal_dimension` 逐维钉死；窗口边界 `...only_counts_the_last_thirty...`；缺样本 `...ignores_non_finite...`）⚠ 数值为新游戏假设 → §五 F1 |
| 未关注异常股可被发现 | 全市场池保留（base 1.0 恒正 ⇒ 全市场发现机会保留） | ✓（`anomalous_unwatched_...` P=3/6=0.5 vs 均匀 0.25，4 只全可达） |
| 持仓及活跃计划股票不可淡忘 | `prune(&protected)` 只驱逐未受保护条目；protected = 持仓∪活跃计划由调用方组合（price_memory 同款接缝，任务 21 PlanBook 为保护源） | ✓（`active_plan_...` 真实 PlanBook create/Terminated + `cap_overflow_...` 10 受保护全存活） |
| 未持仓/无计划上限 8 | 复用 `MAX_UNHELD_WATCHLIST_STOCKS=8`（experience.rs:27，任务 19） | ✓ |
| 按最后实际关注时间及 StockCode 稳定修剪 | 降序 `(minute, code)` 保留前 8（与 `RetailExperienceState::prune_watchlist`、`PersonalPriceMemory::prune` 约定一致） | ✓（`no_plan_watchlist_prunes_...` 含同分钟代码破同分双向断言） |
| 计划终止后可淡出 | 终止后调用方不再列入 protected ⇒ 恢复可驱逐 | ✓（`active_plan_...` 第二段：Terminated 后 prune 驱逐 600001） |
| 新曝光不等于已读 | 抽样不写个人信息状态（结构性 + 1000 次字节钉死） | ✓ |
| 未披露事实不改变发现权重 | 输入面只有 MarketView + 曝光集合；私有 Books 结构性不可达 | ✓（failure 金样：总账 serde 字节已变而 weights/候选/会话字节全等，前置断言防空洞） |
| 失败电池：非法观察时间/恢复不一致 | 未来观察 `ObservationInFuture`、回拨 `TimeWentBackwards`、恢复越界 `InconsistentState`，拒绝后零状态残留 | ✓（4 测试，`matches!` 字段绑定精确比较） |
| 确定性 | 同种子同序列、流推进、个体流可重放 | ✓（`sampling_is_deterministic_...`；exposure 重放段） |
| 会话接线归任务 26 | 计划依赖矩阵 25 阻塞 26；task-16 先例（纯状态先行、接线后置）同款 | ✓ 裁决接受 |

QA 命令映射：happy/failure 同命令 `cargo test -p engine --test attention_discovery`（failure 面 6 测试经 `failures` 过滤，worker 证据与隔离复跑一致）。

---

## 三、LOC / f64 / 数据表裁定

- src 纯行：watchlist.rs raw 173 / 纯 ~105（实施者口径一致）≤250 ✓；attention.rs additive +169 raw（原文件既有部分零改动）✓；experience.rs +5 入口 ✓。
- tests/attention_discovery/main.rs **441 行**：测试夹具/共享 helper（view/market_of/attention/counts_over_draws/assert_share ≈90 行）+ 12 个金样测试，**非数据表、非 src**。项目先例：task-16 fixture.rs 322、task-14 fixtures 439 均按测试文件例外接受；本文件已按 exposure/failures 子模块分流负向与曝光面。裁定：**接受**；若后续扩场景先拆 scenario/fixtures 子模块（task-16 O-1 同款备忘）。实施者未在 issues.md 登记 LOC 例外（见 §六 O-4）。
- f64：身份/时间/排序数学零浮点；f64 仅概率与权重（模块血统一致）。✓

---

## 四、隔离执行记录（全部 exit code + 计数）

工作树 `wt-review-25` @ 94205d1（detached）。主树零 cargo。

1. `cargo test -p engine --test attention_discovery` → **exit 0**；**20 passed / 0 failed / 0 ignored**（t25r-1.txt；与 worker task-25-happy.txt 精确一致，同一套件二进制语义）。
2. `cargo test -p engine`（全量）→ **exit 0**；35 个结果行合计 **850 passed / 0 failed / 4 ignored**（session 112+4 ignored；doc-tests 0；`fundamental_beliefs` 套件缺席——task-18 未提交工作不在本提交，正确）。计数链条：828（2d15e7c 基线，task-16 复核隔离实测）+ 20（本任务套件）+ 2（新 ts_rs 导出绑定 lib 测试 `export_bindings_watchedstock`/`export_bindings_personalwatchlist`）= **850 精确命中**（t25r-2.txt）。
3. `cargo check -p engine -p server -p web-wasm -p engine-gpu` → **exit 0**；零 warning/error（t25r-3.txt，Finished 1m33s）。
4. `cargo clippy -p engine --all-targets` → **exit 0**；7 条警告全部既有登记项：behavior/decision.rs:239 filter_map（任务 1 起，指令明示不据此 REJECT）、tests/experience_feedback 3（任务 20 登记）、tests/analysis_profiles 3 unusual_byte_groupings（任务 17 登记）。**本提交 8 文件 0 警告**（t25r-4.txt，grep src 命中仅 decision.rs:239）。

---

## 五、同提交登记检查（第 8 次强制：8 文件全 engine、零 docs/fixture）——**FAIL → F1（REJECT 主因，唯一阻断）**

**新游戏假设参数清单**（代码头注自标「版本化待校准游戏假设」；attention.rs L99–107）：

| 参数 | 值 | 计划/先例锚？ |
|---|---|---|
| `DISCOVERY_ANOMALY_MOVE_RATIO` | 0.02（\|Δ\|≥2%） | **无**——计划任务 25 只说「异常30分钟涨跌」无数值 |
| `DISCOVERY_ANOMALY_RELATIVE_VOLUME` | 2.0（≥2 倍） | **无**——「相对量能」无数值 |
| `DISCOVERY_MOVE_BOOST` / `DISCOVERY_VOLUME_BOOST` | +1.0 / +1.0（基础 1.0） | **无** |
| `DISCOVERY_ANNOUNCEMENT_BOOST` | +2.0 | **无** |

非新增项（不计数）：`HELD_PRIORITY_PROBABILITY`=0.60、`WATCHLIST_BIAS_PROBABILITY`=0.70 继承 behavior/heuristics.rs:32/43 既有字面量（命名化，数值未变）；`DISCOVERY_MOVE_WINDOW_MINUTES`=30 计划逐字锚（「异常30分钟涨跌」，K5 30 分钟窗口通例）；cap 8 任务 19 锚。

**取证结果（probe hard，全部实证）**：
- `packages/engine/tests/fixtures/company-model/policy-sources.json` @ 94205d1：grep `attention|关注|发现|曝光|discovery` **零命中**——无任何注意力/发现权重登记条目（不存在可 fold-in 的先验 `game-assumption-attention-weights`）。
- docs @ 94205d1：仅 ADR-0016 §4（qualitative 语义锚，「异常涨跌、成交和公开消息可影响发现」**无数值**）与 gap-checklist 一行（同 qualitative）。无数值登记。
- 本提交 8 文件全 engine，零 docs/fixture 改动。
- 登记现状：仅代码头注 + learnings §Task 25（契约描述详尽）+ 提交信息。**issues.md 无 task-25 条目**（notepad 登记本身也不完整）。

**裁定**：task-10/11/12/13/14 同款复发——新「待校准游戏假设」未按已强化的同提交登记门禁写入 policy-sources.json（fixture 行）与 docs。task-11 复核已裁定「notepad/代码头注登记 categorically insufficient」；task-10 先例（K4 游戏冲击参数，同为纯游戏假设、部分数值甚至在计划文本中）仍要求 fixture+docs 登记。本例数值**连计划文本锚都没有**（比 task-10 更强地构成"新增假设"）。task-16 复核 §3-7 明确把「曝光权重模型」推迟到任务 25 并预期在此登记——本轮未落地。**F1 成立，REJECT**。

**修复路径（engine 代码零返工，先例镜像 070ee58/e8210bb/a1b8fc3/52b0d16）**：docs+fixture-only 跟进提交——
1. policy-sources.json `sources[]` 新增 `game-assumption` 条目（如 `game-assumption-attention-discovery-weights`）：登记权重模型（基础 1.0；异常30分钟涨跌 \|Δ\|≥2% +1.0；相对量能 ≥2.0x +1.0；公告曝光 +2.0；最大 5.0；涨跌对称；缺样本/非有限=无信号；60%/70% 继承声明），issuer=本游戏（计划 K4 + ADR-0016 §4，用户已批准），status=simulated-game-assumption；
2. docs 数值登记（ADR-0016 §4 补数值小节或等价 docs 位置）；
3. `cargo test -p engine --test policy_manifest` 保持 5/5 绿（fixture 有效性测试）。
跟进提交落地后本裁决按 task-12/13/14 先例翻转为 APPROVE（复核仅需验证登记内容与代码常量一致 + policy_manifest 绿，无需重跑全量）。

---

## 六、worker 证据对账

| 证据 | worker 声称 | 隔离复跑 | 一致 |
|---|---|---|---|
| task-25-red.txt | E0432×4 + E0599×18 = 22 编译错（实现不存在时的测试草案） | 逻辑核验：错误行号与最终测试文件导入/调用面对应，TDD red 可信 | ✓ |
| task-25-happy.txt | 20/0 | t25r-1：20/0 | ✓ |
| task-25-failure.txt | 6/0（failures 过滤，14 filtered out） | 同套件子集，主套件全绿覆盖 | ✓ |
| task-25-fullsuite.txt | 872/0/4 = 828+20+「task-18 24」 | 主树口径成立（含未提交 fundamental_beliefs 22）；隔离口径 850/0/4 = 828+20+2。learnings 归因「task-18 24」实际 = 22（fundamental_beliefs）+ 2（本任务自身 ts_rs 绑定测试，误归因）——±2 算术笔误，非缺陷 | ✓（0 failed 门精确） |
| check 4 crate | exit 0（learnings） | t25r-3 exit 0 | ✓ |
| clippy | 新代码 0 警告 | t25r-4：8 文件 0 警告，7 条全为既有登记 | ✓ |

计划复选框：任务 25 在 plan L490 仍为 `- [ ]`（本批未被预标；复核即门禁）。

## 七、非阻塞观察（供后继任务，不设独立任务）

1. **O-1（给任务 26，接线契约）**：曝光新鲜度窗口（公告公布后曝光持续多久）与公司↔股票映射显式留给任务 26（模块文档 + learnings 已登记）；接线时须与 F1 登记条目数值一致，防漂移。task-16 复核 O-1（EarlyRead vs 幂等的组合路径金样）触发条件同在任务 26 接线。
2. **O-2（给任务 26）**：`select_discovery_stock` 的 held 池按 market 成员过滤，而 behavior 原版不过滤——正常市场全上市时无差异；接线时若两 surface 并存须显式选择其一并测试锁定。
3. **O-3（F1 修复顺手项）**：learnings 中「task-18 24」归因笔误如上；无需动作。
4. **O-4（流程）**：实施者未在 issues.md 登记 task-25 条目（LOC 例外、scope cut 等仅进 learnings/代码头注）——后续 worker 恢复 issues.md「记录的问题」惯例。

## 八、结论

三个 AGENTS.md 问题中 Q1（大A语义）/Q2（最小范围）/Q3（边界测试/漂移/复杂度）**全部通过**；全部计划红线与 Acceptance 逐项有测试锁定且隔离复跑精确对账（20/0、850/0/4、check exit 0、clippy 新文件 0 警告）；已登记偏差（接线后置、protected 调用方组合、幂等读法无涉）全部裁决接受。**唯一阻断 = F1：5 个新游戏假设参数（2%/2.0x/+1.0/+1.0/+2.0，基础 1.0）未按同提交登记门禁写入 policy-sources.json + docs（第 8 次强制，task-10/12/13/14 同款）。VERDICT: REJECT**——修复为 docs+fixture-only 跟进提交（engine 代码零返工）；落地后按先例翻转为 APPROVE。

---

## Re-verification after fix a885ac1（2026-09-11，同一独立复核人）

**过程性结果：对 `a885ac1` 本体的翻转验证失败**（该提交为下述事故的错误基底产物，不在分支上）；复核进行中 orchestrator 完成血统恢复，F1 修复以 `b2d88a9` 落在正确基底并通过全部复验 → **最终翻转成立（见本节末尾）**。复核另发现并实证记录了事故 F2（分支血统回退，后由 orchestrator 恢复）。

### F2 事实链（全部实证；针对当时分支尖 a885ac1）

1. **a885ac1 的父提交 = da6c399（任务 15），不是 94205d1**（`git log --pretty=%h parents:%p` 实测）。`git merge-base --is-ancestor 94205d1 a885ac1` → exit 1：任务 25 提交**不是** a885ac1 的祖先；`branch -a --contains 94205d1` 与 `--contains 2d15e7c` 均为空——两个提交仅 reflog 可达（悬空）。
2. **reflog 还原事故序列**（--all，时间序）：09:29:37 `b2d88a9` 修复提交（parent=94205d1，**正确基底**）→ 09:29:46/56 task-18 zombie 提交 49d6af4→amend 6c08c4c → 09:30:11 `reset HEAD^` 丢弃 → de30209/e528007 再试修 → **09:30:50 `reset: moving to da6c399`**（整链回退到任务 15，孤儿化 2d15e7c + 94205d1 + b2d88a9/e528007）→ 同秒提交 a885ac1（同内容、错基底）。
3. **a885ac1 树内代码缺席**：`session/attention.rs` 在 94205d1 有 13 处 `DISCOVERY_/discovery_weights` 命中，在 a885ac1 **零命中**；`experience/watchlist.rs`、`information/acquisition.rs`、`tests/attention_discovery/`、`tests/information_acquisition/` 全部不存在于树内。
4. **登记内容本身正确**（对 94205d1 代码逐值 spot-check，值取自初审读过的 attention.rs 常量）：基础 1.0 ✓（`let mut weight = 1.0`）、\|Δ\|≥0.02 ✓（`DISCOVERY_ANOMALY_MOVE_RATIO=0.02`）、≤30 已完成分钟 ✓（`DISCOVERY_MOVE_WINDOW_MINUTES=30`）、量能≥2.0x ✓（`=2.0`）、+1.0/+1.0/+2.0 ✓（三个 BOOST 常量）、最大 5.0 ✓（1+1+1+2）、缺样本/首价≤0/非有限无信号 ✓、60%/70% 声明为继承项 ✓（与 HELD_PRIORITY/WATCHLIST_BIAS 常量一致）；docs/company-accounting.md 新增 §2.8 镜像同值；3 文件 41 insertions、零 `.rs` 改动、无其他 fixture 条目变动——**内容完全符合 F1 修复规格，位置（基底）完全错误**。
5. **worker 修复证据（task-25-fix.txt）不能佐证该提交**：其 20/20 声称只能在主树工作目录残留（mixed reset 不动工作区）上跑出，而非 a885ac1 的树；证据文件自述「Commit: recorded in final git metadata after this evidence file was added」（写时哈希未知）。

### 隔离复跑 @ a885ac1（worktree wt-review-25b，主树零 cargo；raw outputs t25f-{1,2,3}.txt）

| 命令 | 结果 | 判定 |
|---|---|---|
| `cargo test -p engine --test policy_manifest` | exit 0（$LASTEXITCODE 直读）；5 passed / 0 failed | ✓ fixture JSON 自身合法 |
| `cargo test -p engine --test attention_discovery` | **exit 101：`error: no test target named attention_discovery`**（可用目标清单中 attention_discovery 与 information_acquisition 双双缺席） | ✗ 要求的 20/20 验证结构性不可能 |
| `cargo test -p engine`（全量） | exit 0；**815 passed / 0 failed / 4 ignored** = da6c399 基线精确命中（850 − 20 本任务 − 13 任务16 − 2 ts_rs 绑定） | ✗ ≈850 期望未达——分支已回退到任务 16 之前 |

方法论更正（诚实登记）：首轮 `cmd /c "... & echo EXIT:%ERRORLEVEL%"` 的 `%ERRORLEVEL%` 为行解析期展开，输出值不可信；本轮改用 PowerShell `$LASTEXITCODE` 直读。首轮结论不受影响——其依据是输出文件内容（全部 `ok`、0 failed、无 error 行、Finished 行），与 cargo 语义一致；首轮 4 个 t25r 文件保留为证。

### 恢复路径（git 手术归 orchestrator/人类，复核人不执行写操作）

正确修复提交已存在于 reflog：**`e528007`**（parent 链 e528007→b2d88a9→94205d1，内容与 a885ac1 完全同构，仅证据文件哈希行 1 行之差；b2d88a9 与 94205d1 的 diff = 同 3 文件 41 insertions）。恢复选项：`git branch -f codex/feat/web-ui-polish e528007`（或等价地在 94205d1 上重提交同内容）。悬空提交仅 reflog 可达、会过期——**尽快执行**。恢复后重跑本节三命令（预期 5/5、20/20、850/0/4）即可按 F1 修复规格翻转为 APPROVE，无需再来一轮全量人工复核。

### 恢复后复验 @ 分支尖 bea13b9（F2 已由 orchestrator 恢复；同日 09:35–09:37 落地）

复核期间 orchestrator 已按其事故记录（issues.md「会话事故记录与警告」）完成恢复：reflog 09:35:11 `reset: moving to 6c08c4c` + 09:37:18 chore 提交 `bea13b9`。当前分支链（`merge-base --is-ancestor` 实测全在链上）：`da6c399(15) → 2d15e7c(16) → 94205d1(25) → b2d88a9(F1 修复, 正确基底) → 6c08c4c(18) → bea13b9(chore)`。`a885ac1` 与 e528007/de30209 成为事故残留（不在分支上，无裁决效力）。工作树 wt-review-25b 重指向 bea13b9 复验：

**值 spot-check（worktree 内 attention.rs 代码 ↔ b2d88a9 fixture 条目，逐值）**：

| 代码（attention.rs @ bea13b9） | fixture `game-assumption-attention-discovery-weights` | 判定 |
|---|---|---|
| `DISCOVERY_ANOMALY_MOVE_RATIO = 0.02`（L104） | 「涨跌幅绝对值达到 0.02（2%）」 | ✓ |
| `DISCOVERY_ANOMALY_RELATIVE_VOLUME = 2.0`（L106） | 「相对量能达到 2.0x（倍）」 | ✓ |
| `DISCOVERY_MOVE_BOOST = 1.0`（L108） | 「发现权重加成 +1.0」 | ✓ |
| `DISCOVERY_VOLUME_BOOST = 1.0`（L109） | 「发现权重加成 +1.0」 | ✓ |
| `DISCOVERY_ANNOUNCEMENT_BOOST = 2.0`（L110） | 「公告公开曝光时，发现权重加成 +2.0」 | ✓ |
| `let mut weight = 1.0`（L143） | 「基础发现权重为 1.0」 | ✓ |
| `DISCOVERY_MOVE_WINDOW_MINUTES = 30`（L116） | 「最近不超过 30 个已完成交易分钟」 | ✓ |
| 1+1+1+2 算术 | 「权重最大为 5.0」 | ✓ |
| `HELD_PRIORITY=0.60`/`WATCHLIST_BIAS=0.70`（L112/114） | 「继承既有选股语义」声明（非新增） | ✓ |

docs/company-accounting.md §2.8 镜像同值（b2d88a9 diff 内核对）。结构合规：3 文件 41 insertions（fixture + docs + evidence）、零 `.rs` 改动、无其他 fixture 条目变动、`kind: game-assumption`/`status: simulated-game-assumption`/issuer 沿先例结构——**完全符合初审 F1 修复规格**。

**隔离复跑 @ bea13b9**（raw outputs t25f-{4,5,6}.txt，exit 经 PowerShell `$LASTEXITCODE` 直读）：

| 命令 | 结果 | 判定 |
|---|---|---|
| `cargo test -p engine --test policy_manifest` | exit 0；**5 passed / 0 failed** | ✓ |
| `cargo test -p engine --test attention_discovery` | exit 0；**20 passed / 0 failed** | ✓ |
| `cargo test -p engine`（全量） | exit 0；36 套件全 ok；**872 passed / 0 failed / 4 ignored** = 850（94205d1 态）+ 22（任务 18 fundamental_beliefs 经 6c08c4c 入链）+ 0（chore 零新测试），与 worker 原主树全量 872/0/4 精确吻合 | ✓ |

### 本节最终结论

F1（登记门禁）已按规格修复并落在正确基底（b2d88a9，94205d1 之上、同链同提交要求满足）；三命令复验全过；值 spot-check 逐项精确一致。F2（分支血统事故）已由 orchestrator 恢复（bea13b9），恢复后全量 0 failed；事故本身已由 orchestrator 在 issues.md 登记（含对后续 worker 的 reset/amend 禁令），a885ac1/e528007/de30209 为无害残留（不在分支上；reflog 过期后自然消亡，无需 action）。**VERDICT: APPROVE（翻转成立）**——初审 REJECT 记录（F1）与 a885ac1 否定核验（上表）保留于本文件作为门禁执行依据。
