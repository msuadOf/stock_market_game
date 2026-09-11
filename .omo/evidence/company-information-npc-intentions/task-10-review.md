VERDICT: APPROVE (converted after remediation e8210bb; original rejection preserved in §3/history and §Re-verification below)

# 任务 10 独立复核（保险合同服务与负债计量）— commit 60c7c00

> **状态流转**：初审（2026-09-11，针对 60c7c00）= REJECT，唯一主因为 D1–D5
> 游戏假设未按任务 11 先例登记于 fixture+docs；语义/数学/测试面全部通过。
> 修复提交 `e8210bb`（docs+fixture+1 边界测试）后复审 = **APPROVE**，
> 见文末「Re-verification after fix e8210bb」。

复核人：独立 subagent（未实施本任务）。复核日期：2026-09-11。
分支：`codex/feat/web-ui-polish`，被审提交：`60c7c00`（父提交 `070ee58`）。

**拒绝理由（单一主因，非代码质量问题）**：实现依赖多项游戏假设/Fixture 简化
（贴现曲线构造、风险调整确定方式、获取现金流摊销缺位、亏损成分列报口径、
利率冲击范围），但这些假设**未按任务 11 确立的登记先例**（fixture 条目 +
coverage-row source_ids + docs 行，与代码同提交）登记于
`policy-sources.json` 与 `docs/company-accounting.md` §2.4 —— 60c7c00 共
22 个文件、2918 行插入，**全部位于 packages/engine，零 docs/fixture 改动**。
现有登记只存在于代码头注与 `.omo` notepad，而 notepad 未入库
（主仓 `git status` 显示 `?? .omo/`，checkout 60c7c00 即丢失）。按本次复核
委托的明确分级：「Unregistered assumption = REJECT（same grade as task 11's
finding）」。修复为 docs+fixture-only 跟进提交（镜像 070ee58），**不需要任何
engine 代码改动**（见「修复路径」）。

---

## 0. 隔离执行协议（MUST §3 遵守情况）

- 主仓 `git status --porcelain`：存在兄弟未提交工作（`apps/web/src/types/generated/*`
  修改与新增、`?? .omo/`）→ **主树未运行任何 cargo**；无 company/consolidation/
  company/operations/tests/consolidation 等下一波 zombie 目录出现。
- 隔离环境：`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-10 60c7c00`
  （exit 0，detached HEAD 60c7c00）。
- 原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t10r-{1..4}.txt`
  （`cmd /c "... > file 2>&1"`）。
- 复核人仅写两类文件：本复核文件 + notepad issues.md 追加条目。未触碰产品文件、
  未 commit/stash/reset、未运行 pnpm。

## 1. 命令与结果（全部在隔离 worktree 执行）

| # | 命令 | exit | 结果 |
|---|---|---|---|
| 1 | `cargo test -p engine --test insurance_accounting` | 0 | **17 passed; 0 failed; 0 ignored**（与 task-10-happy.txt 17/0/0、task-10-failure.txt 10/0/0（7 filtered）逐条一致） |
| 2 | `cargo test -p engine`（全量） | 0 | **728 passed; 0 failed; 4 ignored**（= 基线 711 + 保险 17，符合预期 ≈711+17；29 套件全绿） |
| 3 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 0 | 无错误 |
| 4 | `cargo clippy -p engine --all-targets` | 0 | 8 条 warning 全部**非新文件**：`behavior/decision.rs:239`（任务 41 既有，禁止据此拒绝）、`tests/experience_feedback/main.rs` ×3（task-20 范围，既有）、`tests/analysis_profiles/invariants.rs` ×3（既有）。**company/insurance/ 与 tests/insurance_accounting/ 零警告**（worker「新代码 clippy 0 警告」声明属实）。 |

## 2. AGENTS.md 三问

### 问 1：是否符合大 A 语义，依据是否可靠？

**计量机制层面：符合，且依据可靠。**

- **锚点交叉核验**：代码引用的全部 CAS 25 条款（§11/12/20 分组、§21/23–26 履约
  现金流量三元组、§27 初始确认（净流入→CSM / 净流出→首日亏损入损益）、§28
  LRC+LIC、§29–32 责任单元释放与收入不含投资成分、§33/34 保险财务损益、§46–49
  亏损组、§58–72 再保险、§39–44 直接参与分红/浮动收费、§84/85 列报、CAS 30（2026）
  §55(二)）逐条存在于 fixture `cas-25-insurance-2020`（verified-official，44 页
  120 条已核验）与 docs §2.4 行中，无虚构锚点。
- **K3 红线全部满足**：
  - 保费不立即计收入：`establish_group` 仅 Dr 1122 / Cr 2501；`collect_premium`
    仅 Dr 1002 / Cr 1122（不二次贷记负债）；收入只经 `release_service` 释放
    （Dr 2501 / Cr 6051）。金样断言 6051 建立日为 0。
  - 服务后正确释放：责任单元线性分摊（claims/RA/CSM 分量 + FractionUnits 余数
    链守恒 + 末批精确清零）。
  - 赔案发生与支付分离：`record_claim`（Dr 6451 / Cr 2502，NonCash）vs
    `pay_claim`（Dr 2502 / Cr 1002，Operating）两个独立事件。
  - 调节表：claims.rs 金样五恒等式（LRC 逐笔滚动 / LIC = 发生−支付 / CSM =
    初始+再计量−释放 / 现金 = 保费收讫−赔款支付 / 损益 = 保费−已发生赔付 经济
    恒等式）全部精确断言。
  - 亏损组显式建模（首日亏损、加重、转回、转盈利 CSM）；重估三向分流
    （ΔPV→CSM 吸收不过账 / ΔF→6541 即时过账 / 亏损分量→6451）。
- **手算复核（本复核人独立重算，全部吻合）**：PV = rhe(80_000×3_650_000/
  3_796_000) = 76,923（292,000,000,000 = 3,796,000×76,923 + 492,000，半偶
  舍入向下）；CSM₀ = 100,000−76,923−5,000 = 18,077；F = 3,077；部分释放
  100 单元：claims 21,918（余 −70）、RA 1,370（−50）、CSM 4,953（−145）、
  财务 843（+5）、LRC 72,602，恒等式 E+RA+CSM−F = 58,082+3,630+13,124−2,234
  = 72,602 ✓；重估 ΔE=+8,000@265d：ΔPV=7,774、ΔF=226、CSM=5,350；
  ΔE=+50,000：ΔPV=48,589、亏损 35,465；ΔE=−30,000：ΔPV=−29,153、转回
  15,917、CSM 13,236。终态净利恒等式（1000−800=200、600−800=−200、
  1000−1000=0、600−300=300 元）全对。
- **整数纪律（K2 血统）**：company/insurance/ grep `f64|rand|std::time|unsafe`
  零命中；全 i128 分 + `rhe_div` 半偶舍入孪生 + 逐组件余数守恒；`checked_abs`
  对 i128::MIN 类型化拒绝不 panic。`DiscountAssumption` 为版本化显式配置、
  无默认构造、测试标注 Fixture。
- **失败电池**：负/零服务单元、非法贴现率（0 与 −1）、支付超现金 = PaymentFailed
  且 `assert_eq!(ins, before)` 字节不变、支付超未付余额、保费超应收、越保障期
  释放、重复组/赔案、未登记对手方、开局子账种子守卫；分红/投连/再保险（分出/
  分入）四类 `UnsupportedContract` **按类命名**拒绝且状态字节不变。无股东分红
  支付路径（grep 无命中）；投连/分红只以 UnsupportedContract 出现。
- **post_with_commit 协议**：validate→post→apply 血统保持；拒绝（含
  PaymentFailed）时 next_event_id 不推进、账套与子账字节不变；零过账事件槽位
  照常消耗（ecl.rs 同语义）。
- **其他探针**：journal.rs +17 纯 additive BusinessKind 标签（serde 普通 enum，
  旧存档可反序列化，任务 9/11 同款先例）；chart v4（bank v3 / real_estate v5
  无冲突，AccountChart::new(4)）；`reports/insurance.rs` 纯读分类层（不生成
  报表、不结账、不触现金）；无 session/market 耦合（imports 仅
  accounting/calendar/company）；defaults.rs 未触碰（无默认保险股票）；
  存档 serde 往返金样断言 ✓。

**但**：以下五项为对已核验 CAS 25 条款的**实现简化**（机制上可辩护，见 §3
裁定），其登记状态构成问题 → 见「登记门禁发现」。

### 问 2：改动是否为需求所必需、是否保持最小范围？

**是。** 22 文件 / 2918 行全部落在任务 10 范围：`company/insurance/` 10 文件 +
`accounting/reports/insurance.rs` + journal.rs/company mod/reports mod 三处
additive 接线 + `tests/insurance_accounting/` 8 文件。无无关改动、无顺手重构、
无推测性 fallback。纯行数：groups.rs 227、premium.rs 200（均 < 250 天花板，
notepad 登记数字与实测**逐字吻合**）；gold.rs 215 纯行（登记为接近上限 +
拆分指引）。`#[allow(clippy::too_many_arguments)]` 两处与 ecl/bank 先例一致。

### 问 3：是否存在遗漏的边界测试、跨层语义漂移或不必要复杂度？

- **边界测试遗漏（轻微，修复时顺手补，不单独构成拒绝）**：`InvalidDiscountRate`
  上界（rate_bp = 10_001）无用例 —— guards.rs 只测 0 与 −1；guard 代码本身
  对称且正确（`!(1..=10_000).contains`）。
- **跨图表科目码复用（信息项，非发现）**：1122（保险=应收保费 vs 地产=应收
  账款）、2501（保险=未到期责任负债 vs 地产=长期借款）在不同行业各自的
  AccountChart/Ledger 命名空间内运行，当前无交叉污染；任务 13/33 报表生成与
  合并必须经各行业 presentation_lines 分类层取数，**不得**跨 chart 原始科目码
  合并 —— 留作后续任务注意点。
- **rhe_div 第 N 份孪生**：延续 amount/inventory/industrial::loans/bank::loans/
  real_estate 血统并在 issues 登记（任务 13 统一入口时议），非本任务新债。
- 无不必要复杂度：代码密度与既有行业账套一致。

## 3. 登记门禁发现（REJECT 主因）与简化项裁定

**事实链**：

1. 60c7c00 改动 = 22 文件全部在 `packages/engine`，`git show --stat` 证实
   **零 docs/ 与零 fixture 改动**。
2. `policy-sources.json` 的 game-assumption 条目仅有：early-adoption、act-365f、
   report-schedule、inventory-method、borrowing-capitalization —— **无任何保险
   条目**；保险 5 条 coverage 行 source_ids 均只含 `cas-25-insurance-2020`。
3. docs §2.4 的 8 行全部 ✅ 纯官方锚点，无 🎮/简化标注；其中行 3
   「CSM 调整、责任单元摊销、保险服务收入/费用（不含投资成分）、**获取现金
   流摊销** | CAS 25 §29–32 | ✅」**列明了实现并未建模的子业务**。
4. 实际登记位置：代码头注（csm.rs「游戏假设（版本化显式配置，Fixture 标注）：
   单一平坦年利率、ACT/365F 简单贴现（非复利期限结构）；风险调整不折现；
   预期赔付视为保障期末一次性支付」等）+ `.omo` notepad（未入库）。
5. 登记先例：任务 11 因同类问题被 REJECT，修复提交 070ee58（**早于本提交仅
   28 秒**）以 docs+fixture 双登记 + coverage-row source_ids 补齐
   `game-assumption-borrowing-capitalization`。本任务未跟随该先例。

**裁定（Registered-in-notepad-only ≠ Registered；按委托分级全部判
unregistered → REJECT）**：

| # | 简化项 | CAS 25 依据 | 语义裁定 | 登记裁定 |
|---|---|---|---|---|
| D1 | 亏损成分不循环摊入保险服务收入（首日一次入损益 + 备查随单元释放归零） | §46–49（允许抬升收入列报） | 净损益与循环摊销**完全等价**，纯列报口径差异；游戏可接受 | 未登记于 docs+fixture → 须补 |
| D2 | 贴现曲线构造：单一平坦 rate_bp、ACT/365F **简单**（非复利）贴现、预期赔付期末一次性支付；RA 与应收保费不折现 | §21/§25 | 参数粒度简化，版本化显式配置、诚实标注；游戏可接受 | 未登记 → 须补 |
| D3 | 风险调整确定方式：逐组显式金额输入（不按 §26 技术计算） | §26 | 输入面简化，可接受 | 未登记 → 须补 |
| D4 | 获取现金流摊销（§29–32 CSA）整体缺位 | §29–32 | 无佣金输入面，范围选择可接受；**但 docs 行 3 现状声称该子业务属覆盖行** | 未登记 → 须补（docs 行 3 加「简化/未建模」标注） |
| D5 | 贴现率变动不在重估面（利率冲击留给后续任务） | §29(b)/§33 | 范围选择，可接受 | 未登记 → 须补 |

**修复路径（docs+fixture-only，无需 engine 代码改动）**：镜像 070ee58 做一个
`docs(engine)` 跟进提交 —— (a) `policy-sources.json` 新增保险游戏假设条目
（如 `game-assumption-insurance-gmm-simplifications`：D1–D5 逐条列入
game_assumptions；或按 D2/D3 拆分条目），并把相关保险 coverage 行的
source_ids 追加该 id；(b) docs §2.4 相应行加 🎮/简化内联标注（至少行 3 的
「获取现金流摊销」标「简化：未建模」），并在 §2.4 末尾复述 K3 红线处登记
D1–D5 清单；(c) 顺手（可选）补 `rate_bp=10_001` 边界用例。修复后本复核结论
可翻转为 APPROVE——五项简化的**语义**均已被本复核裁定为可接受，代码、测试、
数学与协议面无需返工。

## 4. 通过项清单（修复周期内无需重查）

隔离重跑全绿（§1 表）；金样手算全对（§2 问 1）；五恒等式调节表存在且精确；
UnsupportedContract 四类按名拒绝 + 字节不变；PaymentFailed 类型化 + 状态不变 +
险企继续运行；无 f64/rand/time/unsafe/dividend；journal additive + 存档兼容；
chart v4 无冲突；纯行数登记准确（227/200/215）；clippy 新文件零警告；
defaults.rs 未触碰；无 session/market 耦合；事件 id 单调协议保持。

## 5. 证据文件

- 本文件：`.omo/evidence/company-information-npc-intentions/task-10-review.md`
- 原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t10r-1.txt`（保险套件
  17/0/0）、`t10r-2.txt`（全量 728/0/4）、`t10r-3.txt`（check 0）、
  `t10r-4.txt`（clippy 0，8 条既有警告定位）
- worker 证据：task-10-happy.txt（17/0/0）、task-10-failure.txt（10/0/0，
  7 filtered）、task-10-verify.txt（17/0/0）—— 与隔离重跑一致
- notepad：issues.md / learnings.md 2026-09-11 W2-Task 10 节（登记来源；注意
  notepad 未入库，不构成提交内登记）

---

## Re-verification after fix e8210bb（2026-09-11 复审）

**结论：修复达标，VERDICT 翻转为 APPROVE。** 初审唯一拒绝主因（登记门禁）
已按修复路径完整消除；初审「通过项清单」全部沿用（60c7c00 引擎代码在
e8210bb 中**零改动**，初审的手算/协议/红线结论不受影响）。

### 隔离与主树状态

- 主树 `git status --porcelain`：存在 task-14 zombie 未提交中途工作
  （`packages/engine/src/accounting/consolidation/`、`company/events.rs`、
  `company/operations/`、`company/rng.rs`、`accounting/mod.rs` 修改、
  `session/company_operations.rs` 相关 + web generated types + `?? .omo/`）
  → **主树未运行任何 cargo、未做任何修改**（除本证据文件）。
- 隔离：`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-10b e8210bb`
  （exit 0）。原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t10f-{1..4}.txt`
  + `t10f-diff.txt`（git show 全量 diff）。

### Diff 核验（`git show e8210bb`，恰好 3 文件，+44/−7）

1. **`packages/engine/tests/fixtures/company-model/policy-sources.json`（+21/−3）**：
   - 新增 `game-assumption-insurance-gmm-simplifications`（kind=game-assumption、
     status=simulated-game-assumption、issuer=本游戏（计划 K3，用户已批准）），
     `game_assumptions` 数组逐条登记 **D1–D5**，结构与
     `game-assumption-borrowing-capitalization`（070ee58 先例）完全同构。
   - 3 条 CAS 25 保险 coverage 行（GMM 行 / 保费行 / 赔案·亏损组·重估行）
     source_ids 追加该 id；2 条 CAS 30 列报行未动（正确——不携带 GMM 简化）。
2. **`docs/company-accounting.md` §2.4（+14/−4）**：
   - 行 3（履约现金流量）内联 🎮 简化标注（D2：单一平坦利率、ACT/365F 简单
     贴现、RA/应收保费不折现、预期赔付期末一次性支付；D3：RA 逐组显式输入）；
   - 行 4（CSM/责任单元/获取现金流摊销）标注「简化：获取现金流摊销未建模
     （D4）」——**初审点名的行 3 覆盖声明失实问题以行内标注解决**；
   - 行 5（金融变动额）标注 D5；行 6（亏损合同组）标注 D1（等价列报口径）；
   - §2.4 末尾新增登记段落：`game-assumption-insurance-gmm-simplifications`
     全列 D1–D5 + 「均为版本化游戏假设，不构成 CAS 25 合规声明」。
3. **`tests/insurance_accounting/failures/guards.rs`（+16）**：新测试
   `discount_rate_above_upper_bound_is_rejected_at_construction`（rate_bp =
   10_001，断言类型化 `InvalidDiscountRate { rate_bp: 10_001 }`）——与既有
   `illegal_discount_params_are_rejected_at_construction`（0 与 −1）风格
   一致，补齐初审轻微发现（上界无用例）。
4. **D1–D5 语义交叉核对**（对照 60c7c00 已读代码头注，e8210bb 未改引擎源）：
   csm.rs（单一平坦利率/简单贴现/RA 不折现/期末一次支付）、config.rs（RA 逐组
   显式输入）、groups.rs（亏损成分不循环摊入收入、备查随单元释放）、premium.rs
   （保费一次性挂账不折现）、remeasure 面（利率冲击不在重估面）——fixture/docs
   登记文字与代码文档语义**逐项一致**，无漂移、无超范围声明。
5. 引擎源码、journal.rs、其余 docs 均零改动（`git show --stat` 3 文件证实）。

### 复跑结果（隔离 worktree，全部 exit 0）

| # | 命令 | exit | 结果 |
|---|---|---|---|
| 1 | `cargo test -p engine --test policy_manifest` | 0 | **5 passed; 0 failed**（fixture 结构/窗口/未来条目校验全绿——新 game-assumption 条目合法） |
| 2 | `cargo test -p engine --test insurance_accounting` | 0 | **18 passed; 0 failed; 0 ignored**（17 + 新上界用例） |
| 3 | `cargo test -p engine`（全量） | 0 | **729 passed; 0 failed; 4 ignored**（= 60c7c00 实测 728/0/4 + 1，与预期完全一致） |
| 4 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 0 | 无错误 |

### 三问终评（沿用初审 + 增量）

1. **大 A 语义/依据**：D1–D5 现已按任务 11 先例完成 fixture + coverage-row
   source_ids + docs §2.4 三重登记且与代码语义一致；其余结论沿用初审
   （锚点全部真实核验、K3 红线全部满足、金样手算精确、整数纪律无 f64）。
2. **最小范围**：e8210bb 恰好 3 文件、零引擎代码改动，未夹带无关内容。
3. **边界/漂移/复杂度**：初审轻微发现（10_001 上界无测试）已补；其余沿用
   初审（1122/2501 跨 chart 复用留任务 13 注意；rhe_div 孪生血统已登记）。

**最终判定：APPROVE。** 任务 10（60c7c00 + e8210bb）通过 AGENTS.md 独立复核
门禁。
