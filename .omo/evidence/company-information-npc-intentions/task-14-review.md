VERDICT: APPROVE

> **最终裁决（2026-09-11 复验后翻绿）**：F1/F2/F-O2/F-O4 已由 `ddf55e1`（装配校验+注释+
> clippy 一词修复）与 `404ae9b`（`game-assumption-operations-pacing` 登记）完整修复并经
> 隔离复验（见文末「Re-verification」节）。原始 REJECT 记录原样保留于下，作为门禁执行
> 依据。未修复项仅剩 F-O1/F-O3（观察级，已移交任务 15/26 契约）。

# Task 14 独立复核 — 「接通经营演化、合同到期及市场行业事件」

- 复核对象：commit `ea87dd3` (`feat(engine): 接通自然日经营与共同经济事件`)，分支 `codex/feat/web-ui-polish`
- 复核人：独立 subagent（未参与实施；AGENTS.md 大A语义与独立复核门禁）
- 隔离环境：`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-14 ea87dd3`（成功；主仓全程只读）
- 原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t14r-{1,2,3,4}.txt`（cmd /c 重定向，含 `EXIT:` 行）

**一句话结论**：engine 代码与测试质量高、K4 红线全部实测通过、无需返工；但本提交携带
**超出计划明文的游戏假设只登记在代码头注与未入库 notepad**（零 docs/fixture 改动）——
与 task-10（60c7c00）/ task-12（3f04b1d）两次 REJECT 同款、且为本复核指令明文
"now-enforced" 的 REJECT 模式；另有 1 个新测试文件 clippy 警告与 worker
"新代码 clippy 0 警告"的自述不符。修复 = 镜像 070ee58/e8210bb 的 docs+fixture-only
跟进提交（+ 顺手 1 词 clippy 修复），engine 逻辑不动的复验即可翻绿。

---

## 一、隔离重跑证据（命令 + 退出码 + 计数）

| # | 命令（worktree wt-review-14 @ ea87dd3） | 退出码 | 结果 |
|---|---|---|---|
| 1 | `cargo test -p engine --test company_operations` | **0** | **21 通过 / 0 失败 / 0 忽略**（t14r-1.txt；与 worker evidence task-14-happy.txt 逐测名一致） |
| 2 | `cargo test -p engine`（全量，31 个二进制） | **0** | **773 通过 / 0 失败 / 4 忽略**（t14r-2.txt；= 3f04b1d 基线 752（含 consolidation 23）+ 本提交 company_operations 21，精确吻合；session 116 测中 112 过/4 忽略；**extraction_replay 3/3 绿**——会话 RNG 流/字节锚未被触碰） |
| 3 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | **0** | 4 crate 全部 Finished（t14r-3.txt） |
| 4 | `cargo clippy -p engine --all-targets` | **0**（无 `-D`） | 警告归属见 F2：1 个既有（behavior/decision.rs:239，任务 1 起登记）+ 3 个任务 17 既有 + 3 个任务 20 既有 + **1 个本提交新文件（tests/company_operations/seam.rs:26 `unnecessary_mut_passed`）**（t14r-4.txt L125-139） |

Worker 证据核对：task-14-happy.txt（21/21，21.80s）与 task-14-failure.txt（同命令、断言面清单）
与我的隔离重跑**完全一致**；task-12-14-verify.txt（21+23）为共享树中间态记录，被本表
#2 的全量数字覆盖。worker issues.md 自述"全量 31 套件 773 过/0 败/4 忽略"——与我隔离
重跑一致，如实。

## 二、AGENTS.md 三问

### Q1 是否符合大 A 语义，依据是否可靠

**符合（engine 语义面）。** 关键裁定：

- **K4 红线「经营事实驱动会计」**：shock/injections 只改经济参数（需求/成本乘数 bp、
  信用风险加成 bp、中断布尔——`state.rs::EconomyAggregates`，全整数）；余额变动全部经
  四行业处理器业务事件过账（produce/sell_credit/purchase/accrue/deliver…）。RNG 只决定
  事件发生与参数，从不随机改余额。`grep f64` 于 `src/company/` **零命中**（金额全部
  `AccountingAmount`/i128，参数全部 bp 整数）。
- **K4 红线「跨行业事件只作用于适用经济字段」**：行业成本冲击按 `IndustryId` 标签门控
  （采样与注入两侧都只打被标签行业，`day.rs` L67-78 / `injections.rs::apply_industry_shock`）；
  银行流不读需求/成本/中断字段（`bank.rs` 头注钉死 + 代码证实：只读
  `credit_risk_add_bp` 与 CreditDeterioration 激活）。金样
  `common_market_shock_diverges_operating_responses` 以**银行账套逐字节相等**锁定该红线。
  观察项（非违规）：公司流采样不按行业过滤事件种类，银行/保险可能采到 ProductionInterruption/
  AssetImpairmentSignal 等对其不适用的激活——其流不读这些字段，经济效果为零（无字段被
  不当改动）；但 `ActivatedShockRecord` 会记录这类惰性事件，**任务 15 公告生成时不得把
  银行/保险的"生产中断/减值迹象" narrate 成经营事实**（见 F-O1）。
- **K4 红线「RNG 分流」**：`rng.rs` 四流（CompanyOperating/MarketShock/IndustryShock/
  InitHistory），seed ^ FNV-1a(tag+稳定id) → SplitMix64 终结子派生；状态 serde 持久化
  （`rng_state_survives_serde_round_trip` 往返后逐字节一致）；与 session 流算法孪生但
  不共享（依赖方向 session→company，不能反向 import——rhe_div 先例）。SplitMix64 三
  常数与 session.rs:76-79 **逐位核对一致**（worker 曾抄错、被 clippy
  unusual_digit_groupings 抓出后修正，learnings 已记）。
- **K4 红线「持久到期队列替代每 tick 扫描」**：`scheduler.rs` 持久化队列，排序键
  `(due_date, id)` 单调 id 稳定序；重复 key 待办期拒绝（弹出后可复用）；DueDateInPast/
  DueSkipped/恢复时 from_parts 全量校验（篡改存档在反序列化边界显式失败）。无整账扫描。
- **默认冲击参数 = 计划 K4 明文**（市场/行业 100bp、公司 200bp、5–30 自然日、
  ±500/±1000/±2000bp、压力场景独立显式数值非倍数派生）：`ShockParams{version:1}` +
  serde 随 `CompanyOperations` 全量入存档——满足"版本化配置持久化"验收口径，且头注
  明示"待校准游戏假设，不声称现实频率"。✅
- **事件目录**：K4 最低目录八变体全数在场（MarketDemandShift / IndustryCostShift /
  CompanyDemandShift / ContractWon / ContractCancelled / CreditDeterioration /
  ProductionInterruption / AssetImpairmentSignal），效果全部是参数变化。
- **前史（K2）**：`generate_history` = [开局年−2 的 1 月 1 日, 开局前日]，独立 InitHistory
  流，完成后 `finish_history` 切 live 流（live 序列与前史长度无关）；1998-01-01 初始化
  下界经日历政策核对、越界类型化拒绝；**不创建历史证券成交、不组装公开报告**（结构上
  无该路径 + history_gold 断言分录全部落在前史窗口/权益恒等）。`HistoryMeta` 供任务 15
  标 SeededPrehistory。✅
- **验收四条**：同 seed 同事件/分录序列（determinism 30 日报告序列+四行业账套+调度队列
  全等、不同 seed 分歧）；违约生成业务风险（PaymentFailed → `PaymentFailureRecord`
  业务状态、现金轨迹 [100,0,0,126,52] 手算钉死、权益恒等、第 6 日仍可推进、无救助）；
  **未执行任何股东分配**（grep 全 company/ 无分红执行路径；ShareholderDistribution 只在
  输入面、提交即类型化拒绝，failures/scheduler.rs 锁定；权益科目净额恒等于开局值三处
  断言）；公司经营不需要成交（seam.rs 休市周六日零 tick 推进+日终再同步）；行业标签只
  读 `CompanySpec.industry`（fixtures 全部 `listed_stock: None` 照常经营）。
- **会话接线（task-5 血统）**：session.rs 仅 +2 行（mod 声明 + 再导出）；
  `CompanyOperationsClockWiring` 是**独立纯函数接缝**，未改 GameSession 内部任何
  RNG/tick/step 路径；extraction_replay 3/3 绿。时钟侧 DueKind 恰好一次语义由 CivilClock
  承担（seam 测试周六派发 4、重排 7），18:00 披露窗口未触碰（归任务 15）。
- 金样数字独立复算全对：+3000bp×10 日 → 工商收入恰 +300,000 分、保险现金恰 +60,000 分；
  行业成本 +1000bp 打化工 → 进项税差恰 1,040 分；信用恶化 → 1231 恰 −5,650→−33,900、
  银行 1303 −640→−16,005（含应计利息进 EAD 的 rhe）；停工 3 日 FG 恰 16 vs 40；地产
  1541 差恰 150,000；减值 2,000bp×¥10,000=200,000。

### Q2 改动是否为需求所必需、是否保持最小范围

**基本最小、additive。** 29 文件 +3,918/−4：company/events+scheduler+rng+operations/
（12 子文件）、session/company_operations.rs、session.rs+2、company/mod.rs（模块声明+
再导出+范围边界头注更新）、8 个测试文件。accounting/、defaults.rs、civil_clock.rs、
behavior/market/strategy **零改动**（过账语义全部复用任务 8–11 已登记处理器；无新增
BusinessKind）。四行业流各只实现"当日节奏"，无越界报表/披露逻辑（任务 13/15 的活没抢）。
无 unrequested refactor。

### Q3 是否存在遗漏的边界测试、跨层语义漂移或不必要复杂度

**有遗漏（详见 F 区），其中登记门禁为 REJECT 主因。** 边界测试覆盖整体扎实（计划 QA
failure 四类全数在场：资金断裂/停工+交付失败/重复 due/股东动作；另有注入校验、日期序、
跳日、前史下界×2、serde 往返、跨休市接缝）。遗漏项：F-O2（同日 due 配置脚枪无测试无
校验）、F-O4（bank.rs 注释与代码触发条件不符）、task-9/10 复核先例类的二线增强（银行
惰性冲击事件无专属测试——F-O1 关联）。复杂度可控；fixtures.rs 470 行属数据表例外
（worker 已登记，见下）。

## 三、登记偏差裁决

| 偏差 | 裁决 |
|---|---|
| 文件清单：operations/ 12 文件 vs 计划单文件 `operations.rs` | **接受**。250 纯行天花板所致（core 合并版 306、day 323），task-8/9/11 多文件先例同款；实测全部 ≤229 行。 |
| `tests/company_operations/fixtures.rs` 470 行（worker 记 439 纯行） | **接受（数据表例外）**。~90% 是 6 家测试公司逐字段装配字面量；defaults.rs(290)/baseline_fixture.rs(304) 先例；已登记"扩前先拆 fixtures/rows"。 |
| real_estate `available_units` 薄适配（pub(super) 接口缺口） | **接受**。任务 11 语义冻结区禁改；同公式公开面推导 + 交叉引用注释；已登记由 real_estate owner 放宽。 |
| 前史 as_of 调用方契约（无机器校验） | **接受（工程缺口，已登记）**。带借款账套有 AccrualNotForward 兜底；后续由行业账套 owner 加访问器。 |
| 计划复选框已被标 `[x]`（.omo/plans L387，指令称复核后才标） | **观察**。未入库文件、可能是 zombie 收尾或 orchestrator 并行动作；不影响代码裁决，提请 orchestrator 知悉。 |
| **游戏假设登记位置（REJECT 主因，见 F1）** | **不接受**。 |

## 四、发现（F 区）

- **F1（REJECT 主因）— 登记门禁不达标（task-10 0c7c00 / task-12 3f04b1d 同款，现为
  enforced lesson）**：本提交 29 文件**全部在 packages/engine，零 docs/、零 fixture
  改动**。计划明文覆盖的冲击参数类（K4 待校准、版本化入存档）合格；但以下**超出计划
  明文的经营节奏游戏假设**只存在于代码头注 + 未入库 notepad（主仓 `?? .omo/`——版本
  控制视角不存在）：
  1. `bank.rs`：存贷量为确定性日程（`deposit_every_days`/`lending_every_days`，每 N 日
     一笔），对任何经济参数无响应（K4 红线的另一半=建模选择，本身合理，但属游戏假设）；
  2. `real_estate.rs`：中断暂停开发支出但**不延长完工时钟**（`development_days` 按开工
     自然日计）——头注自标"登记简化"；
  3. `real_estate.rs`：完工即停新预售，**现售不在本模型**——头注自标"登记简化"；
  4. `events.rs`：保险/地产的信用恶化**仅记录为风险事件、无自动重估面**（部分承继
     task-10/11 已登记边界，但"经营事件到了只记录"的接线语义是本提交新增）；
  5. `insurance.rs`：赔案为确定性保障日日程（elapsed % N），不经调度队列；
  6. （建议顺带）`events.rs`：K4 冲击参数默认值本身入 registry 一条
     `game-assumption-operations-shock-params`，把存档契约显式化。
  registry 语义域先例支持必须登记：`game-assumption-report-schedule`（纯排期假设）、
  `game-assumption-act-365f`（纯数学惯例）已在 policy-sources.json——经营节奏假设同类。
  **修复路径（engine 代码无需返工）**：镜像 070ee58/e8210bb 的 docs+fixture-only 跟进
  提交——policy-sources.json 增 `game-assumption-{bank-operating-schedule,
  re-development-clock, credit-event-surface, insurance-claim-schedule,
  operations-shock-params}` 类条目 + docs/company-accounting.md 相应行内标注（经营层
  新小节或 §2.x 追加）。修复后本复核自动翻绿（语义面 Q1 已全部裁定通过）。
- **F2（次要，新事实）**：`tests/company_operations/seam.rs:26`
  `clippy::unnecessary_mut_passed`（`&mut ops` → `&ops`，一词修复）——worker issues.md
  第 6 条"本任务新增代码 clippy 0 警告"**与 ea87dd3 提交树事实不符**（t14r-4.txt
  L125-139）。既有红（decision.rs:239）与任务 17/20 的 6 个测试警告维持归属不变、
  不在本提交范围。
- **F-O1（观察，任务 15 契约）**：公司流事件种类均匀采样不按行业过滤——银行/保险可
  采到对其不适用的 ProductionInterruption/AssetImpairmentSignal 激活（经济效果为零，
  红线未被破坏），但会出现在 `CompanyDayReport.activated`。任务 15 临时公告**不得**把
  银行/保险的这类惰性激活 narrate 成经营事实；如需根治可在采样层按 CompanyKind 过滤
  目录（行为变更，需独立小任务+金样更新）。
- **F-O2（次要脚枪，建议随跟进提交加守卫或测试）**：期限类参数为 0
  （`receivable_credit_days=0` / `deposit_term_days=0` / `loan_term_days=0` /
  `delivery_lag_days=0`）时，业务在当日派发窗口之后提交当日 due → 次日
  `pop_due_on` 触发 `DueSkipped` 类型化错误且 `next_expected` 不再前进（引擎响亮卡死，
  不静默丢失——符合铁律二，但属可预防的配置级 brick）。fixtures 全部 ≥2 天故未触发。
  建议：装配校验要求 ≥1，或补一条 0 天期限的失败金样。
- **F-O3（观察）**：`CompanyOperationsClockWiring.mirrored` 集合只增不减（滚动利息
  ~1 id/公司/日，长期对局线性增长）。当前量级无害；任务 26 接宿主循环时评估按
  settled_through 裁剪。
- **F-O4（文档级）**：`bank.rs` L106-107 注释称信用恶化重估"仅在恶化事件激活当日执行"，
  但 `|| aggregates.credit_risk_add_bp > 0` 使活跃窗口内**每日**重估（assess_credit 为
  幂等目标化差额，会计结果不变；该子句为注入路径兜底）。注释与代码不符，跟进提交顺手
  改注释或收紧条件。

## 五、结论

| 维度 | 结果 |
|---|---|
| 隔离重跑 | company_operations 21/0（=worker）；全量 773/0/4（=752+21 精确）；check 4 crate 0；clippy exit 0（1 新警告 F2） |
| K4 红线（经营驱动会计/跨行业适用面/RNG 分流/持久队列/版本化参数/事件目录/前史/验收四条/会话血统） | 全部通过（含金样独立复算） |
| 最小范围 | 通过（additive；accounting/市场/行为零改动） |
| 登记门禁（enforced lesson） | **不通过 → REJECT**（F1；修复=docs+fixture-only 跟进提交） |
| 新增缺陷 | F2（一词 clippy 修复）+ F-O2/F-O4 建议随跟进提交 |

**REJECT 范围刻意收窄**：仅登记门禁（F1）+ F2。修复后（跟进提交落地 + 复验
`cargo test -p engine`（773/0/4 不回退）+ `cargo clippy -p engine --all-targets`
（新文件 0 警告））即可改判 APPROVE，engine 逻辑与测试无需返工。

— 独立复核 subagent，2026-09-11（隔离 worktree wt-review-14；该 worktree 留置于
Temp 目录未清理，避免与并发 worker 争抢主仓 git 元数据）

---

## Re-verification after fixes ddf55e1 + 404ae9b（2026-09-11，同一独立复核人）

- 复验对象：`ddf55e1 fix(engine): 经营节奏配置装配校验与注释修正`（6 文件，+75/−3）
  + `404ae9b docs(engine): 登记经营节奏游戏假设`（2 文件，+34/−11）；分支血统
  ea87dd3 → 85115a7/a1b8fc3（task-12 修复，他人范围）→ ddf55e1 → 404ae9b。
- 隔离环境：`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-14b 404ae9b`
  （成功；主仓只读——task-13 zombie 正在编辑 accounting/reports/ + consolidation + closing.rs）。
  原始输出 `C:\Users\msuad\AppData\Local\Temp\opencode\t14f-{1,2,3,4}.txt`。

### 修复项逐条核验（对照原 F 区）

| 原发现 | 修复核验 | 结论 |
|---|---|---|
| F2 seam.rs:26 `unnecessary_mut_passed` | diff 确认 `&mut ops`→`&ops`；clippy 输出无 company_operations 任何警告 | ✅ |
| F-O2 0 天期限 `DueSkipped` 卡死 | `config.rs::FlowParams::validate_durations`：四行业全部节奏参数（receivable_credit_days；deposit/loan term+every；coverage+claim_every；development/presale_open/delivery_lag）<1 → 新类型化 `OperationsError::InvalidDuration`，在 `core.rs::build` 装配处拦截（live `new()` 与 `generate_history()` 共用 build，两路均校验）；新失败测试 `zero_day_duration_is_rejected_without_mutating_config` 断言类型化拒绝 + 构造后配置结构零改动 | ✅ |
| F-O4 bank.rs 注释/代码不符 | 注释改为「恶化事件激活当日及信用风险加成仍在有效窗口时执行」——与 `newly ∨ credit_risk_add_bp>0` 代码一致 | ✅ |
| F1 五项节奏假设未登记 | `policy-sources.json` 新增 `game-assumption-operations-pacing`（kind=game-assumption，五项逐条 + 「不构成真实银行、地产或保险经营节奏声明」免责）；`docs/company-accounting.md` 新 §2.6 🎮 登记同五项；覆盖行 source_ids 追加：bank 5 行（存/贷/计息/手续费/ECL）、insurance 2 行（列报经营类别/赔案）、RE 4 行（预售/交付/开发成本归集/资本化）；**无其他 fixture 条目改动**（diff 仅上述追加；consolidation D6 条目属 a1b8fc3，非本提交） | ✅ |
| F1 可选项（K4 冲击参数入册） | 未做——原复核明示"建议顺带、非必需"（计划 K4 明文 + 版本化入存档已合格） | ➖ 可接受 |

（F-O1 银行/保险惰性事件激活的公告 narrate 禁令、F-O3 mirrored 集合增长——观察级，
已登记 issues.md 移交任务 15/26，不阻塞。）

### 隔离重跑 @ 404ae9b（命令 + 退出码 + 计数）

| # | 命令 | 退出码 | 结果 |
|---|---|---|---|
| 1 | `cargo test -p engine --test company_operations` | **0** | **22 通过 / 0 失败**（=21+1 新 zero-day 测试；t14f-1.txt） |
| 2 | `cargo test -p engine --test policy_manifest` | **0** | **5/5**（新 fixture 条目过结构校验；t14f-2.txt） |
| 3 | `cargo clippy -p engine --all-targets` | **0** | **company_operations 0 警告（seam 警告消失）**；余警全为既有归属：decision.rs:239（任务 1 起）、analysis_profiles ×3（任务 17）、experience_feedback ×3（任务 20）（t14f-3.txt） |
| 4 | `cargo test -p engine` 全量 31 二进制 | **0** | **775 通过 / 0 失败 / 4 忽略**（=原 773 + task-14 zero-day 1 + task-12 修复守卫测试 1（85115a7，consolidation 23→24）；extraction_replay 3/3 仍绿；t14f-4.txt） |

### 复验结论

- F1/F2/F-O2/F-O4 全部修复且隔离验证通过；`ddf55e1` 改动面严格限于原复核指定项
  （无越界 refactor），`404ae9b` 为纯 docs+fixture 登记（engine 逻辑零改动）。
- 原裁决条件（「跟进提交落地 + 全量不回退 + 新文件 0 clippy 警告 → 改判 APPROVE」）
  三条全部满足：775/0/4 ≥ 773/0/4、company_operations 22/0、clippy 新文件 0 警告。
- **最终裁决：VERDICT: APPROVE**（原 REJECT 记录保留于上，作为登记门禁执行依据）。

— 独立复核 subagent 复验，2026-09-11（worktree wt-review-14b 留置 Temp，理由同前）
