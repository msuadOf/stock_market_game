# sweep18：market 与 Money 历史条款全文复核

审计产品基线为 `b76ece3`；实际 checkout 为 `4ad5a2e`（审计 merge，产品相同）。本轮只新增此工作记录，未修改产品、未执行 Git 写操作、未运行测试或编译；下列“已实现”指静态代码与生产 caller 核对，不代表本轮重新验收通过。

## 全文阅读登记

以下三份文件均从首行连续读至末行，没有用关键词检索代替全文阅读：

| 文件 | 全文行数 | 覆盖范围 |
| --- | ---: | --- |
| `docs/superpowers/plans/2026-06-29-market.md` | 663 | Goal、Global Constraints、File Structure、Tasks 1–5、Self-Review、风险 |
| `docs/superpowers/specs/2026-06-29-market-design.md` | 152 | §1–10 全部 |
| `docs/superpowers/specs/2026-06-29-money-fixed-point-design.md` | 113 | §1–9 全部 |

同时阅读 `AGENTS.md`（123 行）、`docs/principles.md`（92 行）、ADR-0019、open-questions；核对 ADR-0016、现行 trading-rules、公司计划 Task 26 及既有总账 R09/R13/Q01 与 candidate-checks。`agents/` 下没有更深层 AGENTS.md。

## market 计划任务映射

| 原文入口 | 状态 | 当前实现与生产 caller |
| --- | --- | --- |
| Goal/Architecture/Global Constraints，:5、:7、:13–20 | 已实现与已取代混合 | `market.rs:50` 仍包装 OrderBook、Money 最新价/昨收；:60 使用整数 limit_bps；:300–319 涨跌停先拒单、错误透传、末笔成交更新。旧共享 V 已取代。 |
| Task 1，:34–143：MarketError/VParams/导出 | 非 V 部分已实现；VParams 已取代 | `market.rs:18–38` 包含 LimitExceeded、OrderBook/Money 透传与 InvalidParams；`lib.rs:52–53` 导出 Market/MarketError。InvalidVParams 改为 InvalidParams 是移除 V 后的对应调整。 |
| Task 2，:147–282：构造、初始价/昨收/涨跌停 | 已实现；价格取整依据后续规则修订 | `market.rs:93–124` 校验比例、基点、正初始价格及 tick；:128/:133 返回 Result；:210–238 以 i128 计算 tick 对齐的正数四舍五入，溢出显式 Err。`tests/market/main.rs:38、47` 覆盖构造；`price_limits.rs:125、133、141` 覆盖拒单与边界。 |
| Task 3，:286–398：place、拒单不变簿、更新 last_price | 已实现 | `market.rs:287–319`；生产 `session/pipeline/continuous_matching.rs:480` 使用 place_recording，同一 place_inner 接受集。无成交不改 last_price；撮合价格读取末笔 trade。`tests/market/main.rs:103、112` 对应成交/MatchResult。 |
| Task 4，:402–539：evolve_v、种子、均值回复/跨零 | 明确取代 | ADR-0016:14–21 取消共同正确 V；公司计划:499–506（Task 26）明确删除 fundamental_value/v_initial/VParams/evolve_v。不得要求恢复此算法或其私有舍入 helper。 |
| Task 5，:543–638：日终/深度/导出 | 已实现；book 跨日保留条款已修订 | `market.rs:448–450` 昨收赋值并清簿；:454–468 双向深度及 limited 入口；生产 `auction_day_end.rs:1461–1473` 已记录 DayEnd Release 事实后日终，`incremental_continuous_stock_shadow.rs:459–460、473` 消费五档并日终。 |
| Task 5 的 clippy/build/test，:619–631；Self-Review，:642–663 | 有对应现行测试入口；本轮未重跑 | 当前 tests/market 拆文件不等于缺旧单文件；不要求固定旧测试数量 ≥95。旧 expect 与 NaN fallback 模板已不在现行 market 的业务边界。 |

## market spec 逐章映射

| 原文章/行 | 状态与证据 |
| --- | --- |
| §1 :10；§2 :18 | 单股容器、纯逻辑、撮合驱动价保留，代码 `market.rs:50、287`；共同 V 与旧银行家涨跌停舍入被后续决定取代。 |
| §3 :27 | code/last_price/last_close/best_bid/best_ask/book/双向深度均存在（:242、247、252、323、328、333、454、463）；cancel 由受控方法 :440 承担。旧 `book_mut` 名称缺失不构成取消能力遗漏。 |
| §4 :79 | 闭区间限制已实现（:302）；整数 tick/bps 的涨跌停路径代替 apply_rate。V 演化已取代。 |
| §5 :99 | 价格越界与 Money/OrderBook 错误显式返回（:18–38、300–319）；未静默 clamp 玩家原始委托。 |
| §6 :106 | account/编排/策略不混入 market；原“集合竞价延后”已进一步实现，由 session/pipeline/auction_day_end 编排，不是此模块缺失。 |
| §7 :111 | 构造、停止价、拒单、成交、盘口、日终测试入口存在；V 测试不再适用。 |
| §8 :126；§9 :138 | market 模块、导出、独立测试存在。文件重组不应误报。验收命令本轮未执行。 |
| §10 :147 | StockCode 复用（market.rs:7）、边界合法（:302）保留；V/helper 要求已取代。 |

## Money spec 逐章映射及生产路径

| 原文章/行 | 状态与证据 |
| --- | --- |
| §1 :10；§2 :18 | i64 分权威存储已实现 `money.rs:42`。市场价格 :56/:58、存档/账户金额使用 Money；显示换元见 `apps/web/src/utils/format.ts:134–135`。 |
| §3 :26：纯整数构造/加减/乘股/比较 | 已实现 `money.rs:46–94`，checked 报错、不 impl 回绕 Add/Sub；派生 Eq/Ord/Copy :26–34。 |
| §3 :41–46：字符串防御解析 | **新增缺口 S18-01**，详见下节；精度、正常数字与大数溢出路径已有，但无数字单小数点被误接纳。 |
| §3 :48–55：比率桥接与舍入 | `money.rs:210–245` 检查非有限、溢出与半偶舍入。真实费用 caller 为 `config.rs:212、229、234`，生产结算 `session/pipeline/transition.rs:146–185` 消费 commission/stamp_tax/transfer_fee，经 fee_delta 结算。 |
| §4 :57 | 比例设置归 GameConfig，tick 归 market；没有恢复旧涨跌停 apply_rate 的必要。 |
| §5 :63 | transparent serde 裸整数（money.rs:36–42）；`tests/money.rs:222、234` 正反序列化测试。WASM 生产 `apps/web-wasm/src/lib.rs:28–40` 序列化并把错误返回；Web `host/protocol/wire-values.ts:48–49` parseMoney 安全整数检查。跨端 i64 范围不统一属于已登记 Q01，非本轮新条款。 |
| §6 :69 | 三类 MoneyError 已有（money.rs:10–21）；字符串无数字边界遗漏同 S18-01。 |
| §7 :77 | `tests/money.rs:95、110、138、152、161、172、182、198、216、222、234` 分别覆盖正常解析、非法解析、半偶、非半值、佣金、负值、非有限、溢出、零率、serde；没有单小数点用例。:90 将精确 -5 分写成 -4 分是旧算术错误，代码测试 :175 保留 -5 正确。 |
| §8 :97；§9 :107 | 源码/测试/模块导出已有，`lib.rs:19–21`；本轮未重跑验收。 |

## 新候选与排除依据

### S18-01：Money 元字符串只含小数点时被成功解析为零

- 原文契约：money spec:45 “空串 / 非数字 / 多个小数点 → Err”；:72 规定 ParseFailed，AGENTS/原则要求外部输入显式失败。
- 代码事实：`Money::from_yuan_str(".")`、`"-."`、`"+."` 以及包围空白形式，都会在 `money.rs:122–133` 得到 `int_part=""` 与 `frac_part=""`；:144 与 :151 都允许空串并跳过数字校验；:159 与 :170 各产生 0；:203 返回 `Ok(Money(0))`。该流程完全确定，无需假设 RNG、运行数据或溢出。
- 缺失测试：`tests/money.rs:110–135` 只验证空串、abc、多个点、多个负号与非法尾部，没有“总共零个数字”路径。
- 影响范围：公开 Money 解析 API 的输入接受集与原文不一致。对 `packages/engine/src` 的全局调用搜索仅找到该 Money 函数定义，未找到生产调用；同名 AccountingAmount 解析是独立函数。不能将它夸大为当前 Web 下单或存档可注入现金漏洞。
- 去重：现有总账 R13 说解析已有；candidate-checks:8 仅核销小数 expect 是否可触发 panic。它们均未讨论无数字小数点，S18-01 为新增库级防御缺口，供主控独立复核。
- 最小补齐建议：先增加 `. / -. / +. / 空白包围形式` 应为 ParseFailed 的测试，保持已有 `.5`、`12.`、`+3.50` 的接受行为，再检查整数与小数部分是否合计包含至少一个 ASCII 数字。本轮不修改实现。

其他差异已排除：共享 V/evolve_v/VParams（明确删除）；book 跨日保留（现行当日委托日终失效，trading-rules:31、45）；涨跌停半偶/market helper（现行 tick/bps 正数四舍五入）；book_mut（已有 cancel 受控能力）；裸整数跨端范围（既有 Q01）；旧 -5→-4 算术示例；代码文件/算法命名变化。未发现第二项可确认的新条款。
