VERDICT: APPROVE (converted after remediation 85115a7 + a1b8fc3; original rejection preserved in history)

# Task 12 独立复核 — 固定集团合并与少数股东权益（commit 3f04b1d）

复核人：AGENTS.md 强制的独立 subagent（未参与实现）。
对象：`3f04b1d` `feat(engine): 增加固定集团合并与抵销`，分支 `codex/feat/web-ui-polish`。
执行环境：隔离 worktree `C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-12`（`git worktree add … 3f04b1d`，exit 0）。主树未运行任何 cargo / git 写操作。

**结论一句话**：engine 代码本身语义正确、最小、可验证（金样全部手工复算精确命中，隔离重跑全绿）；但六项合并口径游戏简化**未按已强化的同提交登记门禁写入 policy-sources.json 与 docs/company-accounting.md**——task-10 REJECT 同款复发（worker 已在 notepad 自认并移交 orchestrator）。修复 = 镜像 070ee58/e8210bb 的 docs+fixture 跟进提交；**engine 代码无需返工**。另有 2 项应在跟进提交中一并处置的次级发现（见 F2/F3）。

---

## 1. 隔离执行证据（命令 + 退出码 + 计数）

原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t12r-{1,2,3,4}.txt`。

| # | 命令（均在隔离 worktree 的 packages/engine 下） | 退出码 | 结果 |
|---|---|---|---|
| 1 | `git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-12 3f04b1d` | 0 | detached HEAD at 3f04b1d |
| 2 | `cargo test -p engine --test consolidation` | 0 | **23 passed / 0 failed / 0 ignored**（t12r-1.txt） |
| 3 | `cargo test -p engine`（全量） | 0 | **752 passed / 0 failed / 4 ignored**，30 个测试目标（lib 89 + 29 个集成目标；t12r-2.txt） |
| 4 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 0 | Finished，无 error/warning（t12r-3.txt） |
| 5 | `cargo clippy -p engine --all-targets` | 0 | 新增 consolidation 文件 **0 警告**；仅存量项（t12r-4.txt） |

全量计数对账：e8210bb 干净树基线 729/0/4 + 本提交新增 consolidation 23 = **752/0/4，逐分吻合**（worker learnings 声称的 752 属实）。

clippy 存量项核对（均非本提交引入，不构成拒绝）：lib 唯一警告 `behavior/decision.rs:239` filter_map（任务 11 起登记）；`analysis_profiles` 3 处 hex 字面量分组、`experience_feedback` 3 处（参数过多/字面 bool）——文件均不在 3f04b1d 触碰范围内。

## 2. 阅读范围（全部整读）

- 8 个源文件：`packages/engine/src/accounting/consolidation/{mod,group,aggregate,eliminate,sale,worksheet,minority,error}.rs`（148/222/134/202/142/67/139/221 行）
- `accounting/mod.rs` 增量 diff（+7：`pub mod consolidation;` + 17 符号再导出）
- 5 个测试文件：`tests/consolidation/{main,gold,intercompany_gold}.rs` + `failures/{mod,intercompany}.rs`（150/287/265/300/197 行）
- worker 证据：`.omo/evidence/company-information-npc-intentions/task-12-{happy,failure}.txt`
- notepad：`issues.md`（W2-Task 12 记录的问题 8 条，含自认登记欠账）、`learnings.md`（task-12 段）
- 政策锚：`docs/company-accounting.md` §2.1 第 49 行（CAS 33 行）、`packages/engine/tests/fixtures/company-model/policy-sources.json` `cas-33-consolidation-2014`（verified-official，逐条核验记录：第七/八/二十一/二十四/二十六/三十/三十一/三十四至三十七/四十/四十一/四十六/五十四条）、`business_events` 第 601 行、`unsupported_contracts` 全量（无任何合并相关条目）、既有 6 个 `game-assumption-*` 条目
- `amount.rs` `apply_basis_points`（半偶舍入落分）、`inventory.rs` `rhe_div`

### worker 证据对账（指令要求的 reconciliation）

- happy 文件：consolidation 套件 23/23，随后附加全量枚举（其 learnings 记 752/0/4）——与我的隔离全量 752/0/4 完全一致。
- failure 文件：19 过 / **4 filtered out** = 过滤执行只跑拒绝类测试，被滤掉的 4 个正是 gold 测试；19 + 4 = 23 对账闭合。
- 指令提到的「appended section 的 33」：全量输出中 `tests\plans.rs` 等**子套件**的计数（33），不是 consolidation 的数字，无矛盾。
- 红灯 TDD 可信：failure 文件记录 E0432/E0433 编译红 + 两个被金样抓住的实现 bug（往来双倍抵销、费用行符号），与 eliminate.rs「按成员对去重 + handled 位图」和 minority.rs「收入/费用行 ΔNI 均为 −Δ净借」的最终形态互证。
- 证据文件曾被 worker 错放在 `packages/engine/.omo/` 下的 E/ 子目录，orchestrator 已挪回仓库根 `.omo/evidence/`（内容未变，校验过与隔离重跑一致）——外观备注，不计发现。

## 3. AGENTS.md 三问

### Q1 是否符合大 A 语义、依据是否可靠 —— 核心语义符合；六项简化偏离 CAS 33 已核验文本且**未登记**（阻断项）

依据可靠性：CAS 33（2014，财会〔2014〕10号）fixture 行为 verified-official 且逐条注明条款核验，retrieval_date 2026-09-10，含原文 URL。核心实现与之相符：

- **合并范围/全额合并**（第二十一/二十四条）：全部直接子公司纳入，`ScopeId::Consolidated(root)` 显式标记。
- **统一会计期间 + 逐项合并抵销**（第二十六条）：`PeriodCoverageMismatch` 强制同覆盖；逐申报生成工作底稿分录。
- **内部债权债务抵销**（第三十条）：一资产一负债、金额精确相等才抵销；不符 → `CounterpartyMismatch` 携带两侧数值，**无任何 plug/差额平账**（单侧缺失同样拒绝）。
- **未实现内部销售损益抵销**（第三十条）：`rhe((转移价−成本)×未售/转移价)` = 教科书口径的毛利比例分摊；金样 200,000 手算命中。
- **少数股东权益/损益列示与分担**（第三十一/三十四至三十七条）：少数 = 调整后子公司口径 × 少数基点（半偶舍入，`apply_basis_points` 单次路径）；归母 = 合并总量 − Σ少数（减法精确，无守恒缺口）。**上游未实现利润冲减卖方（子公司）损益 → 少数自然按比例分担；下游只冲母公司**——主流处理方向，且被 upstream/downstream 对照金样双向锁死（上游少数损益 40,000/权益 240,000 vs 下游 140,000/340,000）。
- **现金红线**：抵销只存在于工作底稿（`WorksheetLine` 无 JournalEntry、无事件 id）；申报科目触现金 → `IntercompanyTouchesCash` 类型化拒绝；合并现金 == Σ 成员现金逐分相等（三个金样显式断言 + 拒绝后 Books 字节不变断言）。

金样手工复算（全部精确命中，单位元）：混合行业 80%（工业母 600,000 NI/8,600,000 权益 + 银行子 200,000/2,200,000 → 少数 40,000/440,000、归母 760,000/10,360,000、现金 7,800,000 = 5,600,000 + 2,200,000，跨 v2/v3 科目表按代码加总 1002/1003/4001）；上游（P=1,000,000/C=600,000/U=500,000 → 未实现 200,000，合并 COGS 300,000 = 集团口径）；下游同形数字；全售出两行抵销（1405=0、COGS=600,000）。80,000/100,000 = 8000bp 精确。

**偏离清单（实现 vs 已核验 CAS 33 文本，均真实存在、均未入库登记）**：
1. 无「长期股权投资 ↔ 子公司权益」抵销——第三十条明文要求抵销长期股权投资；游戏因固定控制开局前既成、无并购计量而省略（理由成立，但 docs 第 49 行目前把第三十条整体标 ✅ 已覆盖，构成跨层语义漂移）。
2. 控制判定简化为持股严格 > 5000bp——第七条控制三要素（权力+可变回报+影响能力）未建模，无半数以下实质控制。
3. 期间覆盖精确相等（休眠子公司缺记账即拒）——真实准则经调整统一期间仍合并休眠主体；属游戏化收紧。
4. 单层固定集团（嵌套 → `NestedGroupUnsupported`）+ 持股必须整除基点（否则 `OwnershipNotRepresentable`，不静默舍入）。
5. 未实现利润按单一申报批次毛利均匀 + 买方存货转移价口径的比例分摊（非逐批 FIFO/个别计价）；亏损内部交易（成本>转移价）显式拒绝。
6. 往来抵销每成员对恰一对申报（多科目往来不支持净额结算）；内部交易申报为调用方派生事实（总账无对手方粒度）。

以上仅在代码头注 + 未入库 notepad。**判定**：实现并非「完全锚定 CAS 33 无额外假设」——至少第 1–3 项是对已核验条款文本的语义偏离，属必须登记的 game-assumption，同提交登记门禁（task-10/11 起强制）未满足。这就是 REJECT 主因（F1）。

### Q2 是否为需求所必需、最小范围 —— 是

14 文件全部在 `packages/engine`（+2481 行），accounting 不 import company（`MemberSpec` 会计域镜像维持依赖方向；测试引入 company 科目表属外部消费者，合法）；无 session/market 耦合；`consolidate` 纯函数只读；任务 13/26 接线全部外推。文件数偏离计划字面（8 vs 计划 mod+4 拆分）已在 notepad 登记且有 task-8/9/11 同款先例。例外一处：`failures/mod.rs` 274 纯行超 250 天花板未登记（F3）。

### Q3 遗漏边界测试 / 跨层漂移 / 不必要复杂度 —— 有（F2/F5/F6）；复杂度合格

见下节发现清单。22 个错误变体各有用途、无投机抽象；worksheet 值类型最小。

## 4. 发现与裁定

- **F1（阻断，REJECT 主因）同提交登记门禁未达标**。`git show --stat 3f04b1d`：14 文件全在 packages/engine，docs/fixture 零改动；`policy-sources.json` 第 601 行 `business_events` 合并行 source_ids 仅 `["cas-33-consolidation-2014"]`（= task-11 修复前保险行的形态）、`unsupported_contracts` 无合并条目、docs 第 49 行无任何简化标注。worker 在 issues.md task-12 第 2 条**自认**「task-10 REJECT 同款……仅登记于代码头注与本 notepad……未写入 policy-sources.json 与 docs/company-accounting.md」并移交 orchestrator。裁定：与 0c7c00（task-10 REJECT）、task-11 修复前完全同构。**修复路径**：镜像 070ee58/e8210bb 的跟进提交——新增 `game-assumption-consolidation-*` 条目（覆盖上述六项，第 1–3 项建议单列并注明偏离的 CAS 33 条款号）、601 行 source_ids 追加、docs §2.1 第 49 行内联 🎮 简化标注（尤其第三十条长期股权投资抵销的省略）。engine 代码无需返工。
- **F2（次级，跟进提交处置或硬约束任务 13）同成员对多笔申报静默重复配对**。`build_worksheet` 用 `balances.iter().position(member==decl.counterparty && counterparty==decl.member)` 找对侧——position 返回**首个**匹配，不排除已消费条目。同成员对第 3/4 笔等额申报（如 AR+其他应收 / AP+其他应付）会把已消费的镜像再配一次 → 负债/资产**双倍抵销**，金额相等时无任何报错（金额不等才会 `CounterpartyMismatch`）。notepad 虽登记「每成员对恰一对申报……不支持」，但代码既不拒绝该形态也不安全，违反铁律二「不支持必须显式拒绝」的项目先例（对照 `NestedGroupUnsupported`/`SaleCostBeyondInvoice`）。建议：跟进提交加 `DuplicateIntercompanyPair` 类 Typed rejection（或至少把「任务 13 派生必须每成员对恰好一对申报」写成其验收标准 + 到期加守卫）。
- **F3（次级）`tests/consolidation/failures/mod.rs` 274 纯行 > 250 天花板，未登记**（.NET ReadAllLines 精确计数：总 300 / 纯 274）。worker 只登记了 `gold.rs`「249 纯行 warning band」——我的口径 gold.rs 为 251 纯行，同样贴线/越线。跟进提交按图形/所有权/期间拆分 `failures/`（对照 insurance tests guards/entities 拆法）或显式登记例外。
- **F4（轻微，备忘）提交信息计数不准**：「23 测试含 20 类拒绝路径」——实际 23 = 4 金样 + 19 个拒绝类测试，覆盖 17 个不同错误变体。「20 类」无论按哪种口径都多计 1–3，属文案不精确，不改语义。
- **F5（轻微，可选补测）5 个错误变体无专属用例**：`UnknownRoot`、`UnknownGroupParent`、`ZeroHolding`、`SaleInvoiceNotPositive` 直接无测试；`UnknownIntercompanyAccount` 经「申报了但账套零发生额」路径可达（aggregate 跳过零发生额科目后 apply_worksheet 触发）也无测试。守卫代码本身对称正确（task-10 同级轻微项）。
- **F6（观察，任务 13 接线契约）申报金额信任账套**：模块不校验申报金额 ≤ 成员账面余额，超额申报会静默过度抵销（可能把合并科目净额翻负）。worksheet.rs 已声明「申报是调用方派生的事实」；任务 13 从 TradeOpenLedger 开项派生时必须保证不超额——建议写入其任务约束。
- 外观备注（非发现）：worker 证据错放 E/ 子目录由 orchestrator 纠正；worker 未勾选计划复选框（流程上由 orchestrator 视本复核处置——本复核为 REJECT，跟进登记提交落地并复验后方可勾选）。

## 5. 指令 §4 红线探针清单（代码层全部 PASS）

| 探针 | 结果 |
|---|---|
| 母公司 80% 子公司金样精确区分少数股东（40,000/440,000） | PASS（手工复算精确） |
| 内部赊销抵销后集团现金不变；抵销只进工作底稿不回记子公司账 | PASS（`WorksheetLine` 无 JournalEntry/事件 id；`IntercompanyTouchesCash` 双保险；`books_unchanged_after_rejection` Books 全量相等） |
| 单体/合并 ScopeId 明确 | PASS（`ScopeId::{Standalone, Consolidated(root)}`，Serde 可携） |
| 无子公司返回类型化 NotApplicable | PASS（非伪空合并） |
| 循环/重复合并拒绝 | PASS（`GroupCycle`/`DuplicateMember`） |
| 控制与持股不一致拒绝 | PASS（恰 5000bp 拒绝，严格 >） |
| 缺相同会计期间拒绝 | PASS（`PeriodCoverageMismatch` 双侧集合） |
| 对手方余额不符显式错误、无 plug | PASS（两侧数值列明；单侧缺失同拒） |
| 上/下游未实现利润方向 | PASS（上游调少数、下游归母，对照金样锁定；主流 CAS 33 口径） |
| 少数 = 调整后口径 × bp 半偶；归母 = 减法精确 | PASS（`apply_basis_points` 半偶 + 总量减法） |
| 固定关系、无并购/股权交易面、cycle 检测 | PASS（纯只读、无变更 API） |
| 同提交登记检查 | **FAIL → F1** |
| worksheet 读 Books 非 bare Journal；无事件 id 消耗；无 f64；无 session/market 耦合 | PASS（`git grep f64` consolidation 零命中；依赖仅 accounting 内部） |
| 源码纯行天花板（≤250） | PASS（最大 eliminate.rs 167 纯行）；测试侧见 F3 |
| 成功路径可达、无死路守卫（task-21） | PASS（4 金样走真实 `consolidate` 全流水线） |

## 6. 复核后处置建议

1. 跟进提交（镜像 070ee58/e8210bb）：policy-sources.json `game-assumption-consolidation-*` + 601 行 source_ids 追加 + docs 第 49 行简化标注（F1）。
2. 同一跟进提交可含 engine 测试改动（e8210bb 先例）：拆分/登记 `failures/mod.rs`（F3）；建议加 F2 守卫 + 测试与 F5 的 4 个直测用例（非强制）。
3. 跟进落地后由 orchestrator 安排对跟进 diff 的快速复核（或按 task-11 先例由本轮复核结论覆盖），再勾选任务 12 复选框。

—— 独立复核人，2026-09-11。隔离 worktree 保留于 `C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-12` 供查证（t12r-*.txt 原始输出同目录）。

---

## Re-verification after fixes 85115a7 + a1b8fc3 (fresh reviewer session — original reviewer session lost to infra timeout; original REJECT preserved above)

复核人：新一轮独立 subagent（未参与实现与原复核）。日期：2026-09-11。
执行环境：隔离 worktree `C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-12d`（`git worktree add … a1b8fc3`，`rev-parse` 核验 HEAD = `a1b8fc35aaae348c541466d9193e30c71de93d68`，`status --porcelain` 干净）。主树零 cargo / 零 git 写操作（仅 Read 工具只读 notepad）。原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t12g-{1,2,3}.txt`（cmd /c 重定向）。

**结论一句话**：F1（登记门禁）、F2（重复申报静默双倍抵销）两个阻断/次级发现已完整修复，F3 已按原复核给出的两个处置选项之一（登记例外）落地；隔离重跑全绿，全量计数与预测精确吻合。REJECT → APPROVE。

### 1. `git show 85115a7`（F2 修复，3 文件 +53）——核验通过

- **类型化拒绝**：`error.rs` 新增 `DuplicateIntercompanyDeclaration { member_a, member_b }` 变体（含文档注释 + `#[error(...)]` 消息），与 `NestedGroupUnsupported` 等先例同形——满足铁律二「不支持必须显式拒绝」。
- **守卫逻辑**（`eliminate.rs` `build_worksheet` 头部，配对循环之前的纯校验遍）：对每笔申报，若其成员对（双向匹配）在先前申报中出现 ≥2 次，或存在同向（member 与 counterparty 均相等）先前申报，即返回类型化错误。逐分支推演：同对第 3 笔（任意方向）被 `pair_count >= 2` 拦截；同对第 2 笔同向被同向谓词拦截；合法的「一资产侧 + 一负债侧」两笔形态不受影响（24 个既有测试全过佐证）。
- **拒绝时状态不变 / 无事件 id 消耗**：守卫只读输入切片、先于任何 worksheet 条目构造；`consolidate` 本为纯只读（原复核 §5 已 PASS），回归测试显式断言拒绝后 `parent`/`sub` Books 与前置快照逐字节相等。
- **回归测试**：`duplicate_intercompany_pair_is_rejected_without_changing_books` 精确复刻原 F2 演示形态——SUB→ROOT AR 1,000,000 / ROOT→SUB PAYABLE 1,000,000 / 重复 SUB→ROOT AR 1,000,000（等额——旧代码静默双倍抵销的触发条件），断言类型化变体 + 双方账套不变。套件 23→24 属实。

### 2. `git show a1b8fc3`（F1+F3 修复，4 文件）——核验通过

- **F1 fixture**：`policy-sources.json` 新增 `game-assumption-consolidation-simplifications`（kind `game-assumption`、status `simulated-game-assumption`、retrieval_date 2026-09-10），逐条覆盖原复核偏离清单全部六项：D1 无长期股权投资↔子公司权益抵销、D2 控制严格 >5000bp、D3 休眠子公司期间覆盖拒绝、D4 单层 + 精确整数基点、D5 批次毛利均匀分摊 + 低于成本拒绝、D6 每成员对一笔申报——**D6 现由 85115a7 真实守卫背书**（不再是纸面声明）；结尾「以上为版本化游戏假设，不构成 CAS 33 合规声明」。
- **F1 coverage 行**：`business_events` 合并行 source_ids 追加该 id（`cas-33-consolidation-2014` + `game-assumption-consolidation-simplifications`）。**diff 核对无其他 fixture 条目改动**（仅新增条目 + 这一行修改）。
- **F1 docs**：`docs/company-accounting.md` CAS 33 行（含第三十条）条款列内联 🎮 简化标注（长期股权投资↔子公司权益抵销未建模）、状态 ✅→✅（简化）、依据列 🎮 指向 fixture；表后新增 D1–D6 清单段，收尾同样声明不构成 CAS 33 合规。跨层语义漂移消除。
- **F1 验证**：`policy_manifest` 5/5（fixture 完整性由测试面验证，t12g-2.txt）。
- **F3 登记**：notepad `.omo/notepads/company-information-npc-intentions/issues.md` 随本提交首次入库（+684），含「## 2026-09-11 Task 12 F3 登记」：`tests/consolidation/failures/mod.rs` 274 纯行超 250 天花板，登记为跟进项。主树 notepad 现存第 682–684 行确认在位（只读核验）。原复核给出的处置为「拆分**或**登记例外」，登记路径合规。
- 另含 worker 证据 `task-12-fix.txt`（修复过程验证记录，consolidation 24/0 + policy_manifest 5/5 与本复核隔离重跑一致）。

### 3. 隔离重跑（worktree `wt-review-12d` @ a1b8fc3）

| # | 命令 | 退出码 | 结果 |
|---|---|---|---|
| 1 | `cargo test -p engine --test consolidation` | 0 | **24 passed / 0 failed / 0 ignored**（t12g-1.txt） |
| 2 | `cargo test -p engine --test policy_manifest` | 0 | **5 passed / 0 failed / 0 ignored**（t12g-2.txt） |
| 3 | `cargo test -p engine`（全量） | 0 | **774 passed / 0 failed / 4 ignored**，31 个测试目标（t12g-3.txt） |

全量计数对账：773（task-13 落地后基线）+ 1 守卫回归测试 = **774，精确吻合预测**；a1b8fc3 早于 task-14 修复 ddf55e1/404ae9b，故不含其增量，符合预期。

### 4. 原发现处置状态

| 发现 | 原级别 | 处置 | 状态 |
|---|---|---|---|
| F1 登记门禁 | 阻断（REJECT 主因） | a1b8fc3 fixture + coverage 行 + docs | **已修复** |
| F2 重复申报静默配对 | 次级（跟进提交处置） | 85115a7 类型化守卫 + 回归测试 | **已修复** |
| F3 failures/mod.rs 274 纯行 | 次级 | notepad 登记例外（原复核两选项之一） | **已登记** |
| F4 提交信息计数不准 | 轻微备忘 | 文案问题，原复核即不要求修复 | 维持备忘 |
| F5 五个错误变体无专属用例 | 轻微（原复核明示非强制） | 未补；新增的 DuplicateIntercompanyDeclaration 变体本身有专属用例 | 维持非强制 |
| F6 申报金额不校验 ≤ 账面余额 | 观察（下游接线契约） | 属任务 13 接线约束建议，task-13 已另行落地与复核，不属本任务阻断项 | 维持观察 |

### 5. 裁定

原 REJECT 的唯一阻断项 F1 与实质次级项 F2 均已按原复核第 6 节处置建议 1/2 完整落地（形态镜像 070ee58/e8210bb 先例），F3 走登记路径；隔离重跑三命令全绿、计数逐分对账。**VERDICT → APPROVE**，任务 12 复选框可勾选。

—— 复核人（fresh session），2026-09-11。隔离 worktree 保留于 `C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-12d` 供查证（t12g-*.txt 原始输出同目录）。
