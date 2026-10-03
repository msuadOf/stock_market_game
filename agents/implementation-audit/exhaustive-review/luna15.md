# Luna15：account / GameConfig 全文复核

## 基线与覆盖

- 基线产品提交 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960`；读取 HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`。两提交间本次涉及的 `packages/engine/src/account.rs`、`config.rs`、`session` 及 `apps/web/src` 无产品差异。
- 逐份从头连续读至 EOF：计划 `docs/superpowers/plans/2026-06-29-account.md` 860 行；账户设计 `docs/superpowers/specs/2026-06-29-account-design.md` 152 行；配置设计 `docs/superpowers/specs/2026-06-29-gameconfig-design.md` 120 行。根 `AGENTS.md` 与 `docs/principles.md` 已读。
- 本报告仅为静态审阅，不运行测试、构建或长任务，不改产品或 Git。

## 计划逐任务矩阵

| 计划原文 | 当前实现 / caller | 复核结论 |
|---|---|---|
| 前言及 Global Constraints、布局，L1–35 | Account 私有状态和 COW：`packages/engine/src/account.rs:90-112`；显式错误：`:47-78`；整数成本字段：`:604-618` | 核心契约已演进实现。计划要求的 TDD/提交过程无法由当前源码验证。 |
| Task 1 strategy 骨架，L37–145 | 当前生产策略已成为策略模块；账户装载 `ProductionStrategy`：`account.rs:10-14,206-209`，策略在 session 决策链消费，非旧占位 trait | 已被后续策略体系承接，不因文件布局变化视为遗漏。 |
| Task 2 类型与错误，L147–246 | `StockCode` / `AccountKind` / `AccountError`：`account.rs:17-78`；根导出需核对时为 `packages/engine/src/lib.rs` | 已实现并扩展了输入、费用及 reservation 错误。 |
| Task 3 持仓成本 / 半偶 / 可卖量，L248–380 | 持仓事实：`account.rs:604-618`；可卖及不变量：`:649-654`；成本半偶：`:656-689` | Rust 端实现。Web 端没有遵循同一派生契约，见 C15-01。 |
| Task 4 构造、策略、只读访问，L382–475 | `new` / strategy / 访问器：`account.rs:114-253`；仓位授予：`:255-275` | 已实现；状态封装为 private COW。 |
| Task 5 买入，L477–597 | 校验与入口：`account.rs:287-301`；累计费用：`:303-324`；先计算再原子写入：`:326-383`；真实结算以 charged receipts 分组：`packages/engine/src/session/pipeline/settlement.rs:54-92,95-147` | 买入、费用、T+1 和原子更新已接通。Session 入簿资源检查在 `account_validation.rs:718-740`。 |
| Task 6 卖出，L598–703 | 校验：`account.rs:398-433`；可卖校验、净回款及清仓：`:435-490`；真实消费仍为 receipt：`settlement.rs:117-132` | 已实现；无持仓正数量卖出得到 `InsufficientShares{have:0}`，符合计划内约定。费用不足会显式拒绝，不静默透支。 |
| Task 7 统一入口、派生量、导出及验收，L706–833 | `apply_trade` / batch：`account.rs:518-572`；市值 / PnL：`:574-601`；生产 settlement 使用 `apply_settlement` 而非旧 API：`settlement.rs:117-132` | 核心能力存在且溢出显式传播。`total_assets` 不在 Account，但 session `account_equity` 汇总现金与全部持仓：`packages/engine/src/session.rs:1471-1489`。本轮未运行计划中的验收命令。 |
| Self-Review，L835–860 | 计划把 `total_assets` 延后给顶层聚合；实际聚合如上。T+1 解锁由日终调用：`packages/engine/src/session/pipeline/auction_day_end.rs:2156-2158` | 旧 API 缺失不构成遗漏；T+1 caller 可追到日终。 |

## 账户设计逐章矩阵

| 设计原文 | 实现 / 调用证据 | 复核 |
|---|---|---|
| §1 背景与边界，L10–14 | `account.rs:90-112`；结算职责与 pipeline 分开：`settlement.rs:38-44,95-147` | 符合单账户账务与调用层分离。 |
| §2 六项决策，L16–27 | 现金/持仓/strategy：`account.rs:101-112`；成本、费用与 T+1：`:287-383,435-490,649-689`；日终解锁 `auction_day_end.rs:2156-2158` | 账户主干保留。历史 T+0 配置旋钮不再是现行契约；现行 T+1 固定语义需以 ADR / trading-rules 为准。负成本显示历史措辞不等于要抹去负成本金额，见 C15-02。 |
| §3 类型、API、Trade方向、市值及总资产，L29–67 | receipt 带账户/side，按 account+stock 聚合：`settlement.rs:78-86`；市值/PnL：`account.rs:574-601`；session 资产：`session.rs:1471-1489` | 核心接线存在。UI 对成本和 PnL 的实现与 Rust API 不同，见 C15-01。 |
| §4 买卖账务，L69–87 | commission / transfer fee / stamp tax：`account.rs:303-324,413-433`；gross 才进入 invested/recovered：`:365-369,466-489`；生产实际费用从 receipt 累计：`settlement.rs:151-177` | 费用及成本分离已落实；不按旧 spec 未提过户费认定缺项。 |
| §5 错误，L89–97 | 显式 `AccountError`：`account.rs:47-78`；结算错误转 invariant：`settlement.rs:117-132,180-184` | 未见静默吞错。受信 Position 构造的 invariant panic 应与外部存档入口校验一并理解，不单独判为可达漏验。 |
| §6 模块边界，L99–105 | Account 公开账务 API：`account.rs:287-601`；UI 展示见 `LocalRefreshViews.tsx:23-46,133-138` | UI 属实消费端；负成本百分比歧义见 C15-02。 |
| §7 测试矩阵，L107–121 | `packages/engine/tests/account.rs` 有账户测试；本轮只读，未运行 | 不能据测试存在宣称通过。跨层派生覆盖缺口见 C15-01。 |
| §8 策略前置，L123–133 | 当前 `StoredStrategy` 与生产策略状态：`account.rs:10-14,128-129,206-223` | 已扩展，不是旧最小骨架。 |
| §9 验收，L135–142；§10 风险，L144–152 | 半偶实现：`account.rs:671-689`；收费真实链：`settlement.rs:54-92,151-177` | 源码证据支持主干；没有本轮执行测试、clippy 或 build。 |

## GameConfig 设计逐章矩阵

| 设计原文 | 实现 / caller 证据 | 复核 |
|---|---|---|
| §1 动机 / 旧 Q8/Q9 阻塞，L10–20 | 现有 GameConfig：`packages/engine/src/config.rs:70-96` | 旧阻塞是写稿时上下文，不应当作现行缺项。 |
| §2 配置类型与校验，L22–28 | 公开 serde 字段：`config.rs:70-96`；构造/serde 后校验：`:98-178`；session 领域约束：`packages/engine/src/session.rs:1002-1018` | fail-loud 校验存在，且增加当前 A 股约束。 |
| §3 字段 / 错误 / API，L30–70 | 类型错误：`config.rs:47-68`；默认：`:180-201`；佣金/印花/过户费：`:203-237`；生产 fee delta：`packages/engine/src/session/pipeline/transition.rs:150-184` | API 已实现并扩展。默认费率/成本费用由 pipeline 用累计委托收费；lot size 被强约束为 100：`config.rs:60-62,163-166`。 |
| §4 范围边界，L71–75 | 价格笼子 caller：`packages/engine/src/session/decision_chain/quote.rs:123`、`packages/engine/src/session/pipeline/continuous_matching.rs:454-456`；保存输入校验见 `apps/web/src/save/schema/market.ts:61` | 设置 UI 当前提供 `price_cage_enabled`；不是配置 schema 缺失。 |
| §5 fail loud，L77–82 | `GameConfig::validate`：`config.rs:130-178`；账户交易入口校验：`account.rs:295,405,544`；session setup 约束：`session.rs:996-1018` | serde 绕过 `new` 的边界有显式校验。 |
| §6 测试矩阵，L84–101 | `packages/engine/tests/config.rs`；具体测试本轮未运行。原文提到的佣金示例有 floor 算术错误：10000 元 × 0.00025 为 2.5 元，小于 5 元下限，不应要求结果 250 分。 | 按现行佣金下限结果应为 500 分；不把旧例题当实现回归。 |
| §7 布局 / 导出，L103–111；§8 DoD，L113–120 | `config.rs` 类型如上；GameConfig 在 `session.rs` setup 和 pipeline 多处消费 | 类型与生产消费已接通；本轮不声称验收通过。 |

## 旧结论复核及候选

### C15-01 仍成立：Web 成本 / 浮动盈亏与 Account 契约不一致

- 文档契约：账户设计 §2 L21–25、§3 L56–63、§10 L144–152，以及计划 Task 3 / Task 7（L248–380、L706–833）规定成本按净投入除持仓并 half-to-even 到分；Rust `unrealized_pnl` 使用已舍入成本。当前实现分别见 `account.rs:656-689,587-601`。
- 生产 UI 路径：快照提供 `qty/invested_cents/recovered_cents`：`packages/engine/src/session/snapshot.rs:185-201`；协议解析为安全整数 cents：`apps/web/src/host/protocol/parse.ts:171-183`；`apps/web/src/app/portfolio-selector.ts:9-17` 取行情价格；`apps/web/src/app/LocalRefreshViews.tsx:28-45` 用原始净投入计算 `avgCost`、`marketValue`、`pnl`；`:137` 展示成本及盈亏。
- 可达反例：持有 200 股，`invested_cents=200100`、`recovered_cents=0`，现价 1001 分。Account half-to-even 成本为 1000 分，PnL 为 `(1001-1000)*200=200` 分；UI 未舍入成本为 1000.5 分，按市值减净投入算 PnL 为 100 分。成本显示的小数/formatter 可再单独确认，但盈亏 100 分差异已足以证明两边公式不同；数值远低于 JS 安全整数边界，因此独立于金额范围 Q01。
- 结论：旧 sweep15 的候选仍在当前 HEAD；是否产品所需取决于 UI 的 PnL 要和 Account.unrealized_pnl 契约一致。不得仅以 Rust 单测或“持仓市值减原始投入”经济口径自动核销，应由总审计定该显示语义并补跨层回归。

### C15-02 维持为文档歧义，不升级为代码缺陷

- 账户 spec §2 L25 / §6 L105 说负成本“显示 `-` 而非 `xx%`”，但现有 UI 持仓列展示成本金额，没有个人成本收益率列：`apps/web/src/app/LocalRefreshViews.tsx:137`。ADR-0012 对个人成本收益率不可用的后续约定不等同于抹去负成本金额。因此不应强制 Account 将负值清零。

### 新候选 L15-01：缺失持仓行情被静默按零价估值

- caller 链：`apps/web/src/app/portfolio-selector.ts:12-16` 对每个正仓位取 `markets[code]?.last_price ?? 0`；`LocalRefreshViews.tsx:31-45` 又以 `heldPrices[code] ?? 0` 兜底，随后将该价计入市值与总资产。`apps/web/src/host/protocol/parse.ts:160-166` 校验 snapshot 的字段结构和各 map 项，但没有校验正仓位代码必须同时出现在 markets。
- 对任一结构合法但跨字段不一致的协议 snapshot（正仓位存在、对应 market 缺失），页面会把该股票市值静默置零，资产与 PnL 失真；项目原则要求对网络输入校验并显式报错。Engine 自己的 `account_equity` 对持仓找不到 market 会显式 panic：`packages/engine/src/session.rs:1471-1488`，反证这种状态违反内部不变量，而不是合法的“停牌估值为零”语义。
- 分类：跨字段输入校验 / UI 估值 fallback 候选。接收面有外部 protocol parser，故具有边界意义；应由总审计核验所有合法协议来源能否保证键集合关系，再决定在 parser 返回上下文错误还是在估值 caller 失败。不要把“无行情”悄悄解释成零市值。

## 旧 sweep15 核销核对

- 保留其“总资产聚合由 session 接手”判断（`session.rs:1471-1489`），以及生产结算经 receipt 而不是旧 API 的判断（`settlement.rs:54-92,95-147`）。
- 保留配置过户费、100 股申报单位、当前 ST 涨跌幅与旧 spec 的差异属于后续语义演进；当前配置实现及校验见 `config.rs:10-14,60-62,88-94,163-166,233-236`。本轮未重查官方规则来源日期，不对规则法源作新增认证。
- 保留旧候选 C15-01；C15-02 仅为历史文字歧义。新候选只记 UI 对缺失行情的零值 fallback，不把正常停牌/价格未更新臆测成该路径。
