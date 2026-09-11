VERDICT: APPROVE (converted after remediation ceadac7 + 52b0d16; original rejection preserved in history)

# Task 13 独立复核 — 「生成完整报表、附注及不可变期间版本」

- 复核对象：`0a58b65`（feat(engine): 生成行业财务报表与版本化结账，24 文件 +4101/−7）+ `9978534`（chore(engine): 应用合并模块 rustfmt 残留，consolidation/eliminate.rs + error.rs 纯格式化，已逐行核对 diff 仅空白/换行）。
- 分支：`codex/feat/web-ui-polish`；复核人非实施者（AGENTS.md 独立复核门禁）。
- 隔离环境：`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-13 9978534`（EXIT 0）。主仓树有任务 15 在途未提交内容，主仓未执行任何 cargo；仅只读 git/文件读取。
- 原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t13r-{1..5}-*.txt`。
- 计划勾选状态：plan L378 已被 worker 预标 `[x]`；本复核为门禁，REJECT 期间该勾选不成立。

## 一、隔离重跑结果（命令 + 退出码 + 计数）

| # | 命令（worktree 内） | 退出码 | 结果 |
|---|---|---|---|
| 1 | `git worktree add ...wt-review-13 9978534` | 0 | detached HEAD at 9978534 |
| 2 | `cargo test -p engine --test industry_reports` | 0 | **16 passed / 0 failed / 0 ignored**（t13r-1） |
| 3 | `cargo test -p engine`（全量） | 0 | **791 passed / 0 failed / 4 ignored**（32 个套件；t13r-2） |
| 4 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 0 | 无错误（t13r-3） |
| 5 | `cargo clippy -p engine --all-targets` | 0 | 警告均为既有：lib `behavior/decision.rs:239 unnecessary_filter_map`（任务 1 起登记）、tests `experience_feedback` 3 条、`analysis_profiles` 3 条（均先前任务范围）。**任务 13 新代码（含 tests/industry_reports）0 警告** — worker 声明属实（t13r-4） |
| 6 | 复核探针 `cargo test -p engine --test t13r_probe -- --nocapture`（scratch 文件，跑毕已删） | 0 | 1 passed；输出见 F1 证据（t13r-5） |

worker 证据核对：task-13-happy（16/0）、task-13-verify（16/0）、task-13-failure（7 拒绝路径子集）与隔离重跑**完全一致**；全量 791/0/4 = 基线 773 + 任务 13 前序 side 2 + 新 16，对账成立。

## 二、AGENTS.md 三问

### Q1 是否符合大 A 语义，依据是否可靠？——总体架构可靠，**更正的期间归属存在一处实质性偏差（F1，REJECT 主因之一）**

依据面（合格）：代码头注钉死 CAS 30（2026）§25/§27/§16（资产负债表列示）、§32–38（损益五分类+费用功能法）、§60/§61（权益变动表）、§63（差错更正存在性）、CAS 31（直接法+间接调节）、CAS 28（更正口径）；docs §2.1 对应 provenance 行在任务 2 已建（CAS 30 行 ✅ 带财会〔2026〕11 号原文链接；CAS 28/31 ⛔ 存在性核验）。利润表五分类/双栏（当季+累计）/比较项类型化 `Unavailable(reason)`/权益表 OCI·分配结构性为 0，均与现行口径相容且已按 guardrail 登记。

**F1（REJECT）更正调整分录的损益在后续期间报表中重复计入当期利润表**：
- 机制：`closing.rs correct()` 内构造的重述映射（BusinessEventId→目标期间）是**一次性局部变量**，仅用于本次重述版本生成；`ClosingEngine` 不持久化该底稿，`close_month`/`snapshot_interim`/后续所有生成均以**空映射**运行 ⇒ 调整分录在后续报表中的有效期间 = 实际过账期间，其损益行进入后续期间的 movement/quarter/ytd 桶（window.rs `add_entry`）。
- 后果（隔离探针实证，t13r-5）：FY2030 原报净利 5,000，2031-01 更正遗漏收入 300 后——重述 FY2030 v2 净利 5,300 ✓；但 **2031-01 月报累计净利 = 300**、**FY2031 年报累计净利 = 300**、2031-01 权益表净利行 = 300。同一笔更正损益在两个会计年度的**已公布**利润表中重复出现。CAS 28 追溯重述法要求前期差错更正调整期初留存收益（经"以前年度损益调整"类科目），**不得计入当期损益**；引擎 sanctioned API（correct → close_month）无法表达正确处理（4103 未用、无以前年度损益调整科目、无持久重述底稿、close_month 亦无 adjustments 入参）。
- 与计划锚的冲突：K3「重述用**独立** restatement 工作底稿，**把调整映射到相关历史期间**…防止重记现金」——现金侧确未重记（CF 实际期间口径 + `restated_cash_correction` 配平行 + ledger 恒等，均验证通过），但"映射到相关历史期间"仅在一次调用内成立，底稿不"独立"留存；损益侧跨期重复计入未登记（notepad 任务 13 条目 9 项简化均未提及；learnings 只记了"2031-01 月报 CF 恰 +300 一次"，lifecycle 测试也**恰好只断言 CF、不断言 2031-01 利润表**——该省略使缺陷未被金样捕获）。
- 修复方向（最小）：`ClosingEngine` 持久化 `(ScopeId → BusinessEventId → 目标期间)` 累积重述底稿并在其 Scope 的**所有**后续生成中传入（纯函数 `generate_report_set` 已支持 adjustments 参数，单体路径即刻成立；BS closing 因 `effective ≤ last` 仍含更正、CF 仍按实际期间 +300 恰一次、间接法配平行不变——该方案下 2031-01 净利应为 0）。lifecycle_gold 增加 `2031-01 月报净利 == 0`、`FY2031 年报净利 == 0` 断言。合并 Scope 更正本就类型化拒绝（已登记），不受影响。

### Q2 改动是否为需求所必需、是否保持最小范围？——是，但**同提交登记门禁不达标（F2，REJECT 主因之二，任务 10 同型复发）**

- 范围：24 文件全部在 packages/engine（closing/reports 生成器 + tests/industry_reports + 行业文件纯增量 `assignments()` 表 + accounting/mod.rs 一行 `pub mod closing;`）。9978534 为纯 rustfmt。无越界改动、无 journal.rs 触碰、无投机抽象。
- 文件清单偏离计划字面（7 文件 vs 实际 13；tests 目录形态）已按 task-8/11、task-6 先例登记（notepad 任务 13 条目 1），**裁定：接受**。
- **F2（REJECT）**：本任务引入/固化的游戏简化——(a) 年末不落结转分录（4103 保留未用）；(b) 间接法"有效期间/实际期间"双口径 + 重述现金调整行语义；(c) 工作底稿抵销流量归属合并申报当期；(d) 合并权益列（实收资本=根成员 4001；子公司权益母公司份额并入留存）；(e) 合并 Scope 重述 `ConsolidatedRestatementUnsupported`；(f) 利润表投资/终止经营结构性留空 + OCI 恒 0（分配恒 0 已在 docs §2.1 登记）；(g) 比较项诚实性宽松口径——**仅存在于代码头注与未入库 notepad**；`tests/fixtures/company-model/policy-sources.json` 的 8 个 game-assumption 无一覆盖报表/结账；docs/company-accounting.md 无任务 13 简化登记（仅有任务 2 的 CAS 30/28/31 provenance 行）。与任务 10 被 REJECT 的情形**同型**（22 文件全 engine、零 docs/fixture、登记仅在头注+notepad），当时以 docs+fixture-only 跟进提交（070ee58 模式）修复后过门禁；任务 12/14 亦各有 docs 登记提交（a1b8fc3/404ae9b）。任务 13 分支尖端（9978534）**没有**对应登记提交。
- 修复：镜像 070ee58 模式的 docs+fixture-only 跟进提交——policy-sources.json 新增 `game-assumption-report-closing-simplifications`（覆盖上述 a–g）+ docs/company-accounting.md 相应简化登记（含"不构成 CAS 28/30/31 合规声明"措辞）；若 F1 以"登记简化"而非代码修复处置（不推荐，见 Q1），也必须先补此登记。

### Q3 是否存在遗漏的边界测试、跨层语义漂移或不必要复杂度？

- **遗漏边界测试（F1 的测试面）**：lifecycle 未断言更正后**后续期间利润表**（只断言 CF）——正是缺陷藏身处；修复 F1 时必须补。
- 跨层语义漂移：除 F1 外未发现。reports 层不读 BusinessKind、不 import company/session/market（grep 证实）；行业由 chart 版本推导（v1|2/v3/v4/v5），工业码表镜像 `company::industrial::chart::acct` 有 `UnclassifiedAccount` 全覆盖守卫 + 四行业金样锁定（已登记）。
- 不必要复杂度：未见。window/consolidated_window 分层清晰；`ReportError` 装箱沿用任务 12 先例。
- **F3（轻微，铁律 2）**：notes.rs:176 `closing.sub(movement).unwrap_or(zero)`——溢出错误被静默吞成 0 并进入**已公布附注**的期初栏。i128 分币量级下实际不可触发，但违反"禁止用默认值掩盖异常"；应传播 Result（build_notes 改返回 Result 或向上收紧）。随 F1/F2 修复顺手处理。
- 观察项（不构成发现）：试算不平仅在守卫层可测（复式构造不可达，测试注释已诚实说明）；比较项"上年流量"宽松基准（保守方向）已在 notepad 登记，随 F2 落 docs；prior_year_end 以净额非空判可用，极端全对冲史会以 `NoPriorYearHistory` 隐匿真实为零的比较项（保守方向，量级可忽略）。

## 三、BusinessKind 标签迁移债——显式裁定（任务指令要求）

**裁定：(b) 可接受地顺延，债务本身仍有登记，但任务 13 欠一次显式评估记录（轻微，已由本复核在 notepad 补记）。**

- 事实链：任务 8 learnings 登记"粗标签债"（赊购=CreditSale、生产结转=Depreciation、核销=ReceivableCollection、资本开支=CashExpense+投资 CF），触发条件="任务 9–11 或 13 **需扩枚举**时迁移"；任务 10 已偿还保险部分（7 变体）并留文"存档 kind 迁移问题依旧由任务 13 统一评估"；**任务 13 自己的 notepad 条目对此只字未提**。
- 评估（本复核完成）：报表层只消费科目代码 + CashFlowClass，**不消费 BusinessKind**——任务 13 无需扩枚举，触发条件未成立；kind 是纯标签（任务 6/8 复核已验证不驱动过账），旧存档可继续反序列化，未来迁移无兼容性破坏。故不构成 (c) REJECT 级缺失。
- 但"任务 13 统一评估"的指针被悬置：下一自然消费者是任务 15（披露叙述若需行业化 kind 再扩枚举并迁移）。已追加至 issues.md 任务 13 复核节。

## 四、计划验收项逐条核对（K3 + 任务 13 Acceptance/QA）

| 验收项 | 结果 |
|---|---|
| 四类公司 + 合并 Scope 均产出四张基本表 + 附注 | ✅ 四行业金样 + consolidated_march_gold 逐一断言五产物；合并含少数/归母拆分 |
| 期初期末 / 上期同比 / 当季与累计明确 | ✅ BS 期末+上年年末比较项、利润表 quarter/cumulative 双栏 + 上年同期、权益表期初→期末、附注 opening/movement/ytd/closing 四栏 |
| 缺历史不填零 | ✅ `Comparative::Unavailable{reason}` 类型化（缺原因类型层不可表示）；捏造 Available 拒绝（rejects 测试） |
| 报表由分录推导可重建、快照不独立随机化 | ✅ `reconstruction_is_byte_equal`（结构+serde 字节）；lifecycle 从推进后账套逐字节重建 1 月月报与 Q1 快照；全层无 RNG/f64 |
| 每日定稿→后续月入账→年结→更正年报、原公开版本不变 | ✅ lifecycle 全程走通（唯后续期间利润表断言缺失→F1）；原版本 serde 字节不变已断言 |
| 月末封月、年末结转 | ✅ close_month/close_year（年末不落结转分录为已登记简化——docs 侧待 F2 落账） |
| 已封月份仅可经带更正来源的调整凭证在当前开放期间入账 | ✅ correct() 前向约束 + 底座 ClosedPeriod 守卫 + 拒绝后账套零改动断言 |
| 重述用独立底稿、保留原凭证/原公开版 | ⚠️ 部分：底稿一次性（F1）；原凭证/原版不变 ✅ |
| 现金表直接法 + 净利润→经营现金间接调节 | ✅ 三分类 + `IndirectLine` 序列，恒等式无 plug，四行业+合并金样闭合（手算复核全部吻合：工业年报 5,595+4,680−4,975=5,300 等） |
| 权益期初/净利/OCI/期末对账 | ✅ EquityCrossFootMismatch 守卫（归母/少数两列） |
| 失败面：科目无归属/重复分类/附注合计不符/试算不平/缺原因或捏造 0 | ✅ 全部类型化拒绝且各有测试（rejects.rs 7 项） |
| rhe_div 统一（任务 8 learnings"再议"） | 无事发生且无需：报表层无除法（grep 证实），未新增副本 |
| 任务 14 F-O1（不得为休眠行业编造经营事实） | ✅ 报表行仅由已归类科目的真实运动产生，无叙述、无编造 |
| LOC 天花板（250 纯行） | ✅ 按既有口径（非空、非注释、非属性、非 use）：closing 247 / balance_sheet 247 / income 249 / consolidated_window 244 / window 236 / fixture 247 / 最大 249 ≤ 250；与 worker 登记（≤250，最大 income）一致（±1 为计数口径nuance） |
| 9978534 fmt-only | ✅ 逐行核对仅空白/换行 |

## 五、转为 APPROVE 的必要条件

1. **F1 修复**：ClosingEngine 持久化重述底稿并作用于该 Scope 所有后续生成；lifecycle/新增测试断言更正后后续期间利润表净利为 0（2031-01 月报、FY2031 年报）；CF/BS 语义保持现状（本复核已验证其为正确）。或（不推荐）将"更正损益计入后续期间当期利润表"作为显式简化登记——但该语义与 CAS 28 直接冲突且误导跨年对比，建议走代码修复。
2. **F2 修复**：docs+fixture-only 跟进提交登记 a–g 七项简化（policy-sources.json `game-assumption-report-closing-simplifications` + docs/company-accounting.md），镜像 070ee58 先例；engine 生成器代码无需返工。
3. **F3 顺手修复**：notes.rs 期初栏溢出不再 `unwrap_or(zero)`。
4. 修复后重跑本文件第一节命令组（全量 0 failed 为门禁）并安排再次独立复核（仅针对修复 diff）。

## 六、复核环境清理

- 探针测试文件已删除（跑毕即删，未提交）；worktree `wt-review-13` 留置于 temp（含构建产物），`.git/worktrees` 元数据无害，可由后续任务 `git worktree prune` 回收。主仓树未执行任何 cargo / git 写操作；本复核仅写入本文件与 notepad issues.md 追加。

— Sisyphus-Junior（独立复核 subagent），2026-09-11

## Re-verification after fixes ceadac7 + 52b0d16 (fresh reviewer session — original reviewer lost to infra timeouts; original REJECT preserved above)

- 复核对象：`ceadac7`（fix(engine): 持久化追溯重述底稿并修复更正跨期泄漏，8 文件 +395/−16）+ `52b0d16`（docs(engine): 登记报表与结账游戏简化，2 文件 +44/−5）。逐行读完两个完整 diff。
- 隔离环境：`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-13c 52b0d16`（EXIT 0，detached HEAD）。主树只读（任务 16 在途），主树未执行任何 cargo。原始输出：`t13g-1..5-*.txt`。本节复核人为另一 fresh subagent，非 ceadac7/52b0d16 实施者。

### F1（原 REJECT 主因）——已修复，代码审读 + 独立探针双重证实

- **底稿持久化**：`ClosingEngine` 新增 `restatements: BTreeMap<ScopeId, BTreeMap<BusinessEventId, AccountingPeriod>>`；serde 经 `closing/save.rs` 的 `EngineSave`（版本键/底稿键平铺为值列表——JSON 键必须为字符串，BooksRef/BooksOwned 先例）。worker 回归 `restatement_worksheet_survives_serde_round_trip` 断言 save→restore 后生成的 2031-01 月报与不落盘路径**逐字节一致**且净利为 0。
- **correct() 累积**：`entry(scope).or_default()` + 逐分录 `insert(source, target)`；二次更正累积已由本复核独立探针实证（见下）。
- **全路径穿线**：`close_month`/`close_year`/`snapshot_interim` 共用 `generate_validated`（取该 Standalone Scope 的累积底稿）；`correct` 直接传入刚更新的底稿。合并路径维持 `ConsolidatedRestatementUnsupported` 类型拒绝（D5 已登记；合并五产物仅测试构造，无生产合并结账路径——与原复核接受边界一致，无回归）。
- **更正损益期间归属（CAS 28）**：修正分录损益仅进目标历史期间窗口（有效期间口径）与后续期间的期初留存/比较项（closing 桶 `effective ≤ last` 天然吸收；movement/quarter/ytd 桶按有效期间排除）。
- **window.rs 双向配平行**：`if in_win { sub } ; if actual ∈ window { add }` 四象限代数核验——双在内相消（NI 与 CF 自然对消）/仅有效在内 −cash（重述版 NI 含更正、现金不在窗）/仅实际在内 +cash（NI 零贡献、实际经营现金在窗）/双在外零。间接恒等式由 `validate()` 对**每个**生成集机器强制（`indirect_total == operating`，validate.rs:150–157），无 plug。

**独立探针（本复核自建场景，数字独立于 worker 夹具；worktree 内跑毕已删）**：FY2030 v1 净利 4,000 → 更正一 +500（2031-01 过账）→ 封月 2031-01 → 更正二 +250（2031-02 过账，同目标）→ 封月 02/03 → Q1 快照。结果（t13g-5，1 passed 首跑即绿）：
- 2031-01 月报：累计/当季/权益净利 **0**；期初留存 **104,500**（吸收更正一）；经营 CF **500 恰一次**；间接法行合计 0+500=500=经营 CF ✓
- 底稿累积：二次更正后 FY2030 v3 重述净利 **4,750**（4,000+500+250），CF 保持 4,000（实际期间）✓
- Q1 快照（snapshot_interim 穿线）：净利 **0**；期初留存 **104,750**；经营 CF **750 恰一次**；间接法 0+750=750 ✓
- v1 年报历经两次更正 + 三次封月 + 快照后 **serde 逐字节不变** ✓

worker 自有回归 `correction_does_not_leak_into_later_periods`（基线 5,295+300）与 lifecycle_gold 增补断言（2031-01 净利 0 / 期初留存 105,595 / CF +300 恰一次 / FY2031 净利 0 / 上年同期重述 5,595）经逐行核对，断言面与原复核第五节条件 1 完全一致——原 REJECT 证据（月报净利 300 泄漏）已消除。

### F2 ——已修复（docs+fixture-only，镜像 070ee58 先例）

- `policy-sources.json` 新增 `game-assumption-reporting-closing-simplifications`：D1–D7 与原复核 a–g 七项**一一对应**；条目形态（kind/status/issuer/retrieval_date/D 编号列表 + 「不构成 CAS 28/30/31 合规声明」收尾）与 `game-assumption-consolidation-simplifications` 先例逐字段同构。
- 三行覆盖挂接（五类报表产出 / 合并范围 / 差错更正与重述底稿）；`docs/company-accounting.md` 新增 §2.7（七项 + 不合规声明措辞）+ §2.1 CAS 28/31 两行交叉引用。
- diff 核对无其他条目改动；`cargo test -p engine --test policy_manifest` 5/5。

### F3 ——已修复

`notes.rs` `opening: closing.sub(movement)?`（类型化传播，`build_notes → Result<Notes, ReportError>`，调用侧 `?`）；残余 `unwrap_or(zero)` 仅为缺桶查表（无运动 = 0，语义正确），非溢出掩盖——与原复核 F3 定界一致。

### 隔离重跑（worktree at 52b0d16）

| # | 命令 | 退出码 | 结果 |
|---|---|---|---|
| 1 | `cargo test -p engine --test industry_reports` | 0 | **18 passed / 0 failed**（16 基线 + 2 新更正回归；t13g-1） |
| 2 | `cargo test -p engine --test policy_manifest` | 0 | **5 passed / 0 failed**（t13g-2） |
| 3 | `cargo test -p engine`（全量） | 0 | **793 passed / 0 failed / 4 ignored**（32 套件；t13g-3）＝ 原基线 791 + 2 项新回归，对账成立。注：本复核指令预估 ≈815 系误把任务 15 publications 的 +22 预计入 52b0d16（该提交晚于 52b0d16）；52b0d16 实际正确计数即 793，0 failed 门禁达成 |
| 4 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 0 | 无错误（t13g-4；grep 命中仅为 thiserror 等crate 名） |
| 5 | 独立探针 `t13g_probe`（scratch，跑毕已删） | 0 | 1 passed（t13g-5，结果见上） |

### 其他核对

- closing.rs 拆分（mod.rs 236 纯行 / save.rs 58 纯行，均 ≤ 250 天花板）；公共路径 `accounting::closing` 不变。
- 环境清理：探针文件已删、main.rs mod 行已还原、`cargo check -p web-wasm` 在 worktree 内再生的 TS 类型产物已 `git checkout/clean` 还原，worktree `git status` 干净；主树零写入（仅本文件）。worktree 留置 temp 供后续 `git worktree prune`。
- plan L378 勾选（`[x]`）在本次转化后成立。

**结论：原 REJECT 三项发现（F1/F2/F3）全部修复且经独立复核实证；转为 APPROVE。**

— Sisyphus-Junior（fresh 独立复核 subagent），2026-09-11
