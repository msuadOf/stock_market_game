# Task 26 Independent Review — 接通完整决策链并彻底删除共同 V（commit 5574a33）

VERDICT: APPROVE

> 初轮（针对 5574a33）为 REJECT；修复提交 d2080fb（父 5574a33）经复核人完整重验后
> 改判 APPROVE。初轮发现与重验结论见下；文末"Re-verification after d2080fb"为改判依据。
> 阻断项已全部修复并带精确证据；残留一条非阻断卫生问题（仓库根 E/ 目录）需后续提交移除。

范围：ccf0490..5574a33（单提交，53 文件，+2933/−1589，branch codex/feat/web-ui-polish）。
复核人重新运行了全部声明的命令（非引用 worker 输出）。核心使命（V 删除 + 决策链接线）
本身实现质量高且全部验证通过；REJECT 仅由一条阻断性发现（测试覆盖回归 + 未登记）触发，
修复成本低、路径精确。

## 复核人重跑结果（reviewer-executed）

- `cargo test -p engine --test company_decision_session` → **11/11 通过，exit 0**。
- `cargo test -p engine`（全量，39 套件）→ **926 通过 / 0 失败 / 4 ignored，exit 0**
  （逐套件手工求和：lib 91 + session 107(+4 ignored) + strategy 71 + … 全绿）。
- `cargo test --workspace` → **exit 0**（server actor 12、api_contract 26、publisher 7、
  ws 4+1 ignored、tauri lib 6 等全绿）。
- `RAYON_NUM_THREADS=1 cargo test -p engine --test company_decision_session same_seed` → 通过
  （worker 的跨线程确定性声明属实）。
- `cargo clippy -p engine --lib -- -D warnings -A clippy::unnecessary_filter_map` → exit 0，
  零警告（既有豁免之外无新增）。
- `git status --porcelain -- packages apps` → 仅 untracked `apps/web/src/types/generated/*.ts`
  （任务 29 既知范围）；**无任何 task-26 产品文件未提交**。commit stat 中无任何 generated .ts。

## 发现（按严重度）

### 1. [BLOCKING] tests/market.rs 整文件删除：约 14 个非 V 测试（A 股涨跌停/价格笼子语义锁）
未迁移、未在 issues.md 登记

**证据：**
- `git show 5574a33 --stat`：`packages/engine/tests/market.rs | 395 -------`（整文件删除）。
- 旧文件（`git show ccf0490:packages/engine/tests/market.rs`）含 24 个测试，其中仅 6 个是
  V 专属（`market_error_and_vparams_basics`、5 个 `evolve_v_*`）。以下 **非 V 测试被一并删除**：
  - `market_new_and_limit_stops`（含非 V 断言 up_stop=1100 / down_stop=900）
  - `price_limits_use_positive_half_up_rounding_and_at_least_one_tick`（涨跌停**正数四舍五入 +
    至少一个最小价位**——A 股核心语义）
  - `price_limit_overflow_is_an_explicit_error_not_a_panic`（显式溢出错误）
  - `continuous_price_cage_uses_the_exchange_reference_price_order`（笼子参考价优先序：
    卖一/买一回退）
  - `continuous_price_cage_uses_the_wider_ten_tick_range_for_low_prices`（低价 10 笼放宽）
  - `market_new_rejects_invalid`、`place_rejects_price_above_up_stop`、
    `place_accepts_boundary_up_stop`、`place_rejects_price_below_down_stop`（带边界闭区间）、
    `place_updates_last_price_on_trade`、`place_match_result_matches_book`、
    `end_of_day_resets_last_close`、`depth_passes_through_book`、`reexport_from_crate_root`
- 全树 grep 确认 14 个测试名**零存留**（`git grep -l <name> -- packages` 全部 GONE）；
  `git grep "up_stop\|down_stop" -- packages/engine/tests` → **零命中**。当前仓库没有任何
  测试断言 up_stop/down_stop 的计算值或舍入规则；笼子仅剩 session 级
  `continuous_limit_orders_obey_102_and_98_percent_price_candles`（只覆盖 ±2%/±10% 施加，
  不覆盖参考价优先序、低价放宽、最小一 tick、舍入方向、溢出错误）。
- issues.md 任务 26 节第 6 条只登记了 session.rs 5 测、strategy.rs、civil_clock、
  allocated_market 的取舍，**完全未提及 market.rs 整文件删除**——诚实性缺口。
- 量化影响：任务 24 基线 939 通过；939 − 29（24 market.rs + 5 session.rs V 测）+ 16
  （company_decision_session 11 + compute 等）= 926，与本次实测吻合。委托方期望的
  "≥950" 缺口正是这批被删测试。

**为何阻断：** 复核轴 5 明确要求"non-V assertions preserved, V-only asserts removed
(not weakened into vacuity)"。涨跌停舍入/边界/笼子参考序是大 A 语义红线区（AGENTS.md
宪章），删除后该规则完全失去测试锁；且删除未登记，违反"最小惊讶 + 诚实"。这不是让失败
测试变绿，但属于"API 变更时放弃迁移、静默收缩覆盖"，与 strategy.rs 里为等价迁移专门搭建
测试壳的做法形成明显反差——迁移显然可行且被 demonstrating 过。

**精确修复处方：**
1. `git show ccf0490:packages/engine/tests/market.rs > packages/engine/tests/market.rs` 恢复文件。
2. 删除 5 个 `evolve_v_*` 测试与 `market_error_and_vparams_basics` 中的 VParams 断言
   （保留/改写其 MarketError 基础断言为 `Market::new` 的 `InvalidParams` 触发）。
3. `mk_market()` 适配新 4 参签名：`Market::new(code, initial_price, limit_pct, tick)`
   （去掉第 4 个 V 参数）；`market_new_and_limit_stops` 删去
   `m.fundamental_value().cents()` 断言行，其余（1100/900）原样保留。
4. 其余 13 个非 V 测试逐字保留（含 `reexport_from_crate_root` 去掉 VParams 引用）。
5. 在 issues.md 任务 26 节追加一条：market.rs 迁移记录（删 5+VParams 断言、保 14 非 V 测）。
6. 复跑 `cargo test -p engine`（预期 ~944 通过 / 0 失败 / 4 ignored）与
   `cargo test -p engine --test market`。

### 2. [NON-BLOCKING] 任务 26 证据文件完全缺失

`.omo/evidence/company-information-npc-intentions/` 下 **0 个 `task-26*` 文件**（此前 25 个
任务均有 task-N-happy/failure/full-suite 惯例文件）。learnings.md 声称的 QA 门禁数字经复核人
重跑全部属实（11/11、workspace exit 0、RAYON=1、clippy），故非虚报，但按计划惯例应补
`task-26-happy.txt` / `task-26-fullsuite.txt`（cmd /c 重定向，勿用 PS `>`）。

### 3. [NON-BLOCKING] 日中计划终止不撤销在途子单（task-24 移交note部分兑现）

task-24 复核要求"cancel children before terminating plans"。日终路径已兑现：
`session.rs` step 日界段顺序 = `record_plan_execution_day_end()` →
`sweep_decision_chain_day_end()`（TradingDayEnded 就地到期终止，任务 21 教训的补账路径）
→ `parent_orders.clear()`（同界清子单）。但**日中**路径：
`decision_chain.rs:692`（below_filled_rationale → 修订即终止）终止计划后，
`execute_plans_for_account` 循环（:885 `!Active → continue`）跳过该计划，其已挂出的子单
不会被撤销，继续在市场成交至日终。账户级不变量安全（真实订单生命周期 + 冻结语义），
子单规模受旧 remaining 上界约束，故不阻断；建议任务 27 持久化契约收口时对终止转移
补"可撤则撤"路径。

### 4. [NON-BLOCKING] 终止/外部计划的迟到事件在 pending 队列中原样保留且无 drain

`synchronization.rs`（任务 26 容错化）：未知/已终止计划的条目 retained 不清。会话内单调
累积（量级极小），restore 不入档（瞬态）。任务 27 定义存档契约时需给出保留/清理规则，
现应在 issues 登记（当前只在代码注释里）。

### 5. [NON-BLOCKING] company_assembly.rs 体积未按惯例显式登记

`decision_chain.rs` 1095 行已在 issues.md 第 4 条登记（诊断/测试面有意暴露）；
`company_assembly.rs` 实测约 342 行（数据装配为主，接近"数据表豁免"），issues 第 4 条
列了文件名但未像 decision_chain 那样显式登记体积与豁免理由。补一句登记即可。

## 已验证为正确的方面（positive findings）

1. **V 删除完整性**：`git grep "fundamental_value\|v_initial\|VParams\|evolve_v\|TrackV\|
   needs_fundamental_value\|fundamental_value_means" -- packages apps/server apps/desktop
   apps/web-wasm` 仅 3 处注释（extraction_replay.rs:34、strategy.rs:498/849 的历史等价说明，
   允许类）。market.rs/compute.rs/strategy{mod,value,institution,factory,params}.rs/session.rs/
   views/snapshot/persistence/lib.rs/engine-gpu（含 evolve_v.wgsl 文件删除）全部清净；
   diagnostics VError 臂删除；Event::VError、SessionSetup.v_params、StockSpec.v_initial、
   MarketSnap.fundamental_value 均不存在。apps/web defaults.ts 仍发 V 字段（serde 忽略，
   issues 第 9 条已如实登记，任务 29 收口）——声明与实测一致。
2. **K4/K5 信息语义**：决策输入只经 `NpcObservationContext`（首次获知时点钉版本）+
   `record_acquisition`（新曝光≠已读）；`world_mutations_between_observations_do_not_change_
   personal_decisions` 用 end_civil_day 前后信念字节全等锁定；诊断/调试面全部 `&self`
   只读，宿主查询零写入。每股估值 = 归母整体估计/已发行总股本（`BeliefInputs.
   total_issued_shares: spec.issued_shares`，decision_chain.rs:366；无流通股字段），
   `per_share_valuations_stay_in_price_dimension_not_total_equity` 以 <10^6 分量级断言
   防整体权益误接；买=乐观端 min 涨停、卖=悲观端 max 跌停再夹笼子（:961-970）；
   `ValuationOutcome::Unavailable` → None → 涨跌停带回退（显式文档化，绝不补 0）；
   机构成本经历诚实 `SignalContribution::unavailable(MissingObservation)`（:536）。
   `believe_chain_params()` 能力探针是唯一入链判据；ActiveTrader/散户/游资结构性不可达
   BeliefBook（decide 只读显式 TargetPolicy）。
3. **RNG 纪律**：decision_chain.rs / company_assembly.rs 全文无 self.rng/self.seed 消费
   （仅文档注释提及纪律）；`derived_stream(seed, tag, id)`（FNV-1a）派生
   analysis-profile / belief-assumptions 独立流（session.rs populate_npcs）；
   公司域 `seed ^ COMPANY_SEED_TAG` 二次派生；发现消费个体注意力流（任务 25 契约）；
   `same_seed_replays_the_whole_chain_bit_identically` 比对事件流+诊断+计划三重 serde
   字节（真实锁，非空洞），RAYON=1 重跑通过。
4. **决策链顺序**：accepted 注意力（能力探针过滤）→ 个体发现+关注列表 → 候选集（持仓∪
   信念∪发现，BTreeSet 定序）→ discovery_candidates+record_acquisition → 信念更新
   （NewMaterial/HorizonExpired）→ K5a 五路聚合 → 计划开/修订（反向跨 ±2000bp
   `reverse_crosses_threshold` 迟滞，:651）→ allocate_soft_budgets → assess_urgency →
   decide_quote → execute_plan_observation 真实路由。日终：时钟 end_day → ops_wiring.
   run_day_end → close_accounting_periods（月末 close_month/12 月 close_year，books_mut
   接缝，非工业分支 unreachable! 诚实标注）→ disclosures.run_day_end → prune_dispatched。
   end_civil_day 不再只推进时钟；step 段 3 由 V 演化替换为 run_decision_chain
   （session.rs:1984）。
5. **夹具迁移诚实（抽查 8 处）**：session.rs（FixedTargetParentOrderStrategy 测试壳，
   `near_fully_reserved_identical_quote_does_not_cancel_and_repost` 断言逐字等价）、auction.rs
   （纯 V 字段删除）、apps/server/tests/actor.rs（纯 V 字段删除）、baseline_fixture.rs
   （V 断言删除）、extraction_replay.rs（三锚重钉，旧→新值在测试注释 + issues 第 8 条
   双登记）、civil_clock.rs（`own()` 过滤公司 dues，非 V 断言保留，issues 登记）、
   strategy.rs（本地壳委托 decide_data 同一路径，TrackV→Fixed 数字等价）、compute.rs
   （V 演化测试替换为 decide_all 等价契约测试）。
6. **测试真实性（误导性成功探针）**：company_decision_session 用真实 GameSession
   （5 股 × 默认表精确股本、27 账户、12 散户/10 机构/4 游资、60-180 step、真实
   end_civil_day/封月/披露），非 stub。
7. **过渡边界诚实**：恢复 = 全量重建 + 经营重放（严格小于比较）+ adopt_all_pending
   （防重复注册）+ linked_plan_id 剥离；作为任务 27 延期在 issues 第 1 条置顶登记
   （含三条后果），未隐藏。
8. **ADR**：0006/0008 只加 superseded 状态指针 + 文末更新节，历史文本未改写。
9. **质量**：无静默 catch/fallback（链内全部 `unwrap_or_else(|e| panic!(…{e}))` 显式带
   上下文）；clippy lib exit 0；既有测试债（analysis_profiles/experience_feedback、
   behavior/decision.rs:239）未动。

## 结论

核心实现与声明的验证全部属实，但 market.rs 非 V 测试的无迁移删除 + 未登记触发
REJECT。按发现 1 的处方修复并补登记（发现 2/5 顺手）后，预期可转 APPROVE；发现 3/4
登记给任务 27 即可。

---

# Re-verification after d2080fb（2026-09-11，复核人第二轮）

修复提交 `d2080fb`（"fix(engine): 恢复A股涨跌停市场测试并修复日中终止撤单"，
父 5574a33，9 文件 +3402/−9）逐项重验，**改判 APPROVE**。以下全部为复核人自行执行。

## R1. 阻断项 1（market.rs 非 V 测试删除）— 已修复，验证通过

- 测试以 `packages/engine/tests/market/{main,price_limits}.rs` 目录形态恢复（250 行
  约定按 task-6 先例拆分，`--test market` 目标名不变）。
- **15/15 通过**（`cargo test -p engine --test market`，exit 0）。
- **逐字性经程序化比对**（node 脚本提取新旧测试体逐行 diff，旧源 = `git show
  ccf0490:packages/engine/tests/market.rs`；初轮我的"24 个测试"说法把辅助 fn 计入了，
  旧文件实际 20 个 `#[test]`）：
  - **8 个逐字节相同**：place_updates_last_price_on_trade、place_match_result_matches_book、
    end_of_day_resets_last_close、depth_passes_through_book、
    price_limit_overflow_is_an_explicit_error_not_a_panic、
    continuous_price_cage_uses_the_exchange_reference_price_order、
    place_rejects_price_above_up_stop、place_accepts_boundary_up_stop。
  - **6 个仅含允许的适配差异**（V 实参删除 / V 断言行删除 / InvalidVParams→InvalidParams
    改名）：market_new_and_limit_stops（仅删 `fundamental_value()` 断言行；1100/900 保留）、
    market_new_rejects_invalid（仅删 v_initial≤0 用例；limit_pct/initial_price 用例原样）、
    reexport_from_crate_root、price_limits_use_positive_half_up_rounding_and_at_least_one_tick
    （17/14/3/1 断言原样）、continuous_price_cage_uses_the_wider_ten_tick_range_for_low_prices
    （295/275 原样）、place_rejects_price_below_down_stop。
  - **丢弃项与处方一致**：5 个 evolve_v_* + FixedRng 辅助（next_f64/next_range_u32）+
    market_error_and_vparams_basics 的 VParams 半边；MarketError 半边以
    market_error_basics（含 InvalidParams）存活。
- issues.md 修复轮第 1 条完整登记（含 926→944 计数对账、拆分理由）——诚实性缺口补齐。

## R2. 阻断项 2 关联（证据文件）— 已补齐，但落位有卫生问题（非阻断）

- `task-26-{happy,failure,fullsuite-raw,workspace-raw}.txt` 已存在且内容真实：
  fullsuite-raw 内 40 套件求和 = **944 passed / 0 failed**；workspace-raw 求和 =
  **1004 passed / 5 ignored**——与复核人本轮重跑完全一致（见 R4）。
- **[残留非阻断] 仓库根 `E/` 目录**：`.omo/` 在 .gitignore（第 109 行），worker 为把
  证据放进提交在仓库根新建 `E/` 并 commit 了 4 个原始日志（~2700 行）。规范位置是
  `.omo/evidence/company-information-npc-intentions/`（未跟踪副本已存在、与 E/ 逐字节
  MD5 相同）；且既有先例（task-12-fix / task-22-happy 等经 force-add 跟踪于 .omo 下）
  表明无需新目录。**处置建议：后续提交 `git rm -r E/`（不得 amend/rewrite）；.omo 侧
  副本已完整，无信息损失。** 另 happy.txt 引用了不存在的 `E/t26-happy-raw.txt`（文档
  内部小瑕疵，随 E/ 一并移除即可）。

## R3. 初轮发现 3（日中终止不撤在途子单）— 已修复，实现与锁定测试均验证通过

- `decision_chain.rs` 新增 `cancel_in_flight_child_before_restructure`：
  无在途子单→放行；有且可撤→**经真实路由撤单**（OrderCanceled 事件入流，冻结随既有
  路径释放；终止路径并 `remove_linked_parent`，反向路径保留母单供 `install_plan_parent`
  覆盖）；不可撤阶段（09:20 后集合/收盘集合/PreOpen）或路由拒绝→false，调用方保留
  计划原状（Keep+PendingReconsideration），本轮只记平静观察，下次观察重试。
  调用点：`will_terminate || flip` 时先撤后修（drive_plans_for_account）。
- **task-24 断言风险场景实测关闭**：反向撤单清空 `active_child_order_id` 后新子单
  可覆盖安装（不再触发 record_parent_order_submission 的第二在途子单断言）；不可撤
  阶段完全推迟重构，冲突单由 IncompatibleExecutionState 守卫抑制；终止路径移除死
  链接母单——无孤儿子单路径。
- **3 枚锁定测试**（src 内 `chain_restructure_tests`，经真实 GameSession 链路播种
  机构 Buy 计划 + 真实在途子单后驱动真实 drive_plans_for_account）：
  反向先撤后翻（Buy→Sell、filled 归零、子单引用清空）、below_filled 终止先撤后终
  （status=Terminated{Cancelled}、母单条目移除）、不可撤阶段不动计划不撤单（子单仍
  在簿、母单仍引用）。复核人重跑 `--lib chain_restructure` → 3/3。
- routing.rs 仅 `route_plan_intent` 可见性放宽（pub(super)→pub(in crate::session)）。
- 借用纪律改造（held_by_code / belief clone 快照）不改变语义（循环内只读）。

## R4. 门禁复核人重跑（全部与声明一致）

- `cargo test -p engine`（全量 40 套件）→ **944 passed / 0 failed / 4 ignored**，
  exit 0（926 + market 15 + chain_restructure 3，与 issues 对账一致）。
- `cargo test --workspace` → **1004 passed / 0 failed / 5 ignored**，exit 0
  （engine 944 + engine-gpu 4 + server 50 + tauri 6；5 ignored = engine 4 + ws 1）。
- `cargo test -p engine --test company_decision_session` → **11/11**（在本轮两 次全量
  中均绿）；`--test market` → 15/15；`--lib chain_restructure` → 3/3。
- `cargo clippy -p engine --lib -- -D warnings -A clippy::unnecessary_filter_map` →
  exit 0，零警告。
- 工作树：`git status --porcelain -- packages apps` 干净（仅 generated TS 未跟踪，
  任务 29 范围）；`git show d2080fb --stat` = 9 文件（issues.md、E/×4、
  decision_chain.rs、routing.rs、market/{main,price_limits}.rs），无 generated TS、
  .gitignore 未触碰。

## R5. issues.md 修复轮登记 — 已核验

新节"W5-Task 26 修复轮（REJECT 复核回应）"5 条：阻断项修复（含计数对账）、发现 3
兑现（实现+3 锁+借用说明）、发现 4 登记（pending 队列清理规则→任务 27 owner）、发现 5
登记（company_assembly.rs ~342 纯行，~200 数据表行豁免说明）、证据文件双份说明。
内容与实际改动一一对应，无虚报。

## 第二轮结论

**APPROVE。** 初轮唯一阻断项（market 非 V 测试无迁移删除 + 未登记）已按处方完全修复
并经程序化逐字比对确认；初轮发现 3 从"登记给任务 27"升级为本轮实际修复且锁定测试
真实；全部门禁数字与证据文件经复核人独立重跑吻合。残留非阻断项：仓库根 E/ 目录应
在后续提交中 `git rm -r`（.omo/evidence 副本已完整）；pending 队列清理规则与
company_assembly 体积拆分维持任务 27/后续登记。
