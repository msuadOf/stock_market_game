# fixtures2 独立复核

复核者：未参与 fixtures2 实施的 `review_fixtures2` subagent。日期：2026-10-03。

基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。范围为
`fixtures2/actions.json` 登记的六动作、12 个源文件相对基线的完整 diff；复核新 owner
及迁移 caller 的源实现、原测试断言与相关生产 API。工作区其他组的生产改动不在本报告
批准范围内；Account/Position/TradingPlan getter 的接线兼容性已结合当前定义核对。

## 结论

本批通过独立静态复核。发现的注释问题已经由实施者修复并再次复核；当前没有尚待修复的
有效发现。没有发现 A 股交易语义漂移、弱化原断言、自动修坏档、共享个人信息或扩大公共
持久存档入口。此结论不表示 Cargo 类型检查、定向短测或完整回归通过，实际执行由 root
统一负责。

已读 AGENTS、principles、architecture、open-questions、trading-rules，结合 ADR-0016、
ADR-0025、simulation-calendar 核对领域与持久化边界。此批没有新增或修改交易制度，
因此没有重新请求交易所规则原文；现有交易依据核对日期继续以 trading-rules 的
2026-09-22/2026-09-25 记录为准，不将本次静态检查称为现行条款全面重验。

## 三项门禁

1. **A 股语义与依据**：原 T+1、挂单现金/股份预留、费用、FIFO、零股余量、个人获知、
   已发行普通股总股数、归母报表和重述口径保持。2030 春节仍使用现有显式模拟日历，
   不把未来休市预测称为官方日历；休市不伪造 tick/PriceTick 或消费市场注意力 RNG。
   GameSession 内存 SaveSlot 的测试使用没有成为用户日内持久化能力，ADR-0025 门禁未扩大。
2. **需求必要性与最小范围**：六动作均落在登记的 tests 文件。各 owner 持有实际相关状态，
   不是无状态 Utility；没有生产新 API、继承层级、全局共享夹具或第二份权威状态。
   同名 getter 迁移是相邻生产字段私有化所需的调用适配；DTO 保持字段访问。
3. **边界、跨层漂移与复杂度**：原 190 个 `#[test]` 名称全部保留，包含四个有 ignore
   属性的长验收 case。除 session 新增五个 assert_eq 外，12 文件的
   assert/assert_eq/assert_ne/panic 分类计数保持。逐 diff 复核原期望值、错误类型、
   seed、事件顺序与 serde 比较路径，没有发现削弱。新增合法订单 fixture 用例覆盖
   envelope、历史 cursor、现金与卖出股份预留；原拒绝路径没有经过同步修复。

## 逐动作证据

| 动作 | 复核证据与判断 |
|---|---|
| hosts-R2-N05 | civil_clock.rs:86 的 SpringFestivalScenario 持有 session 和自注册 dues；own_due 仍以原 id 集过滤。交易/日结调用显式留在测试，T+1、四个休市日不消费市场状态、重复/乱序/跳日原子拒绝与 observer retry 断言保留。2099 单 due 过滤改为按同一 id 计数，语义相同。 |
| hosts-R2-N06 | fundamental_beliefs/main.rs:107 的 BeliefIssuerInputs 明示 kind/total_issued_shares；Case 独占 Scenario/state/market/book。八个 caller 保留 Industrial、ISSUED_SHARES、原 RNG 和 acquisition instant；构造不自动 acquire，apply_cause 每次短借用 context，context owner 校验错误先于 belief 调用且以 BeliefError 透传。NotAcquired、NoOwnAnnualMaterial 和 MethodDisabled 原断言保留。per_share 两次独立 scenario 保持相同披露事实，不共享 state/book；仍比较流通股变化下的 belief 字节与 market spy。 |
| hosts-R2-N08 | correction_restatement.rs:27 的 CorrectionScenario 保存 Books、ClosingEngine、MemberId 和原 v1 字节；close_month/close_year 同参数调用原生产方法。v1 不变、重述归入历史期间、后来期间利润为零、现金实际期间一次列报和上年同期重述值的手算金样保留。serde 对照解出独立 live/restored 状态，没有把两路径合并。 |
| hosts-R2-N10 | plans.rs:51 的 PlanScenario 将 book/id 成对拥有；accept_and_fill 仍先 ChildOrderAccepted 后 ChildOrderFilled，qty/trading_day/child_complete 无默认改写。负例直接调用 PlanBook，受理不等于成交、超额/未知子单/时间回拨/过期/重复计划等拒绝边界保留。跨日多计划 serde 仍显式解出 book/id。getter 与当前 TradingPlan 定义一致。 |
| hosts-R2-N11 | publications/failures/mod.rs:35 的 Base::valid_q1、publication_request 是原自由函数就近迁移；OPS_SEED/stable_company_offset、2030 Q1、批准/公布时点、scope 与公司身份保持。请求的损坏字段仍由测试显式编辑；公布与公告失败类型、缺字段反序列化失败及跨公司更正拒绝不变。 |
| hosts-R2-N12 | session.rs:93 的 TestOrderSaveFixture 只拥有一个 SaveSlot；消费式 restore 内原 envelope 费用计算、排序、cursor max/checked_add 和全账户 reservation 断言保留，实际只 restore 一次。合法构造六入口完成迁移；player position 构造没有订单可被修复。3613/3627 等坏档测试在取得合法 source 后仍直接篡改独立 SaveSlot 并调用 GameSession::restore，绕过 fixture restore。4378/4464 的真实零股与多部分卖单校验保持。新增336用例显式验证同账户两侧预留、两个 envelope、seq [2,7] 与历史 cursor 42 不回退。 |

## 有效发现与修后复核

- **F1（已修复）**：civil_clock.rs 原新增 diff 将英文自然词 session 机械改为
  `market scenario.session`，且修改后的注释没有遵守中文文档要求。已反馈父协调与实施者。
  当前 855、866、880 行三条说明改为中文，准确表达未完成市场会话、先拒绝后重试与
  rollback 保留 observer；没有改调用、断言或 fixture。再次复核通过。
- **发行人输入增量**：原八参数 Case::new 改为七参数及 BeliefIssuerInputs，八处 caller
  显式保持原两字段；没有引入默认发行人、float_shares 替代或改变错误/获取时点。复核通过。
- **TradingPlan 兼容增量**：plans.rs:951 原单字段 `terminated.status` 编辑变为将完整
  TradingPlan 序列化为 JSON、只替换同一 status 后恢复。当前 TradingPlan 为 derive
  Deserialize，其他字段完整保留；没有额外 PlanEvent 或版本/时间改变，PlanBook::from_parts
  的错误与成功输入保持原型。getter 只读原字段，不改 DTO。复核通过。

## 静态验证与执行限制

独立执行了限定 12 文件的 git diff --check：通过。静态逐文件统计为：civil_clock 12；
centers 2；fundamental failures 4；gold 2；per_share 3；correction_restatement 2；plans 33；
publications failures mod 3、announcement 2、publication 5；session 122→123；main 无 case。
合计 190→191，原名称无丢失。数值 token 对照仅出现 centers serde 用例把原 form_on 的
年度索引 3 展开为两处 annual_ids/annual_instants[3] 的重复，原取值未变。

没有执行 Cargo、普通测试、长测试或完整回归，没有改源码或发起 Git 写操作。
实施者提供 rustfmt 证据只当作语法/格式检查；不据此宣称类型检查、运行结果或 TDD red/green。
需 root 取得代表性短测结果后再宣称该批实施完成。
