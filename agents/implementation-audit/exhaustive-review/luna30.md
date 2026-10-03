# Luna 30：Task 33 / 34 / 36 独立全文复核

## 基线与全文覆盖

- 产品基线：`08e4fc75b52a71a3262a8a938c57b44f8b5b4960`；当前 worktree merge HEAD `a7c7ce3` 与产品基线同源，目标是产品 08e4fc7，不把其他文档提交当作代码变更。
- 先读仓库 `AGENTS.md` 与 `docs/principles.md`。三份指定 review 均连续读取到 EOF：`task-33-review.md` 26 行（1–26）、`task-34-review.md` 24 行（1–24）、`task-36-review.md` 57 行（1–57），总计 107 行；不是摘录阅读。
- 逐章追查当前公开报告查询/展示、价格图输入单位和日K来源、DEV 决策 trace 生产写入、真实订单及 CausalReport 路径、WASM 发布隔离、覆盖区间与因果负控 caller。
- 未运行测试、构建、浏览器、线上请求或长验收；本报告不把历史验证结果称为本轮通过。

## Task 33 全章矩阵

| 原文行/章节 | 当前代码/证据 | 复核 |
|---|---|---|
| 3–12 日期、初始结论、scope | 历史范围是 `company-slice.ts`、`company-query-coordinator.ts`、Task33 harness/ADR-0010 | 历史描述，不扩成新要求。 |
| 14 Confirmed：atomic baseline、stale fencing、decimal 字符串、显式状态 | `apps/web/src/store/company-slice.ts` 的状态联合与替换 reducer；`apps/web/src/host/company-query-coordinator.ts` 的 generation/request fencing；public DTO normalize/CompanyPanel | 当前仍以完整新 baseline 替换、校验 generation/ticket，精确财务金额保持字符串；loading/error/unavailable/empty/ready 无默认伪数据。 |
| 15 civil/disclosure 独立于 trade、affected-company refresh | coordinator 的 civil update/disclosure 遍历与受影响公司 query | 行为由 civil/disclosure 事件驱动，无须先有成交；仅刷新已缓存/受影响公司是当前明确策略。 |
| 16 shared EngineHost、无 adapter UI 分支、无 private exposure | coordinator 只经 EngineHost 公共报告接口；CompanyPanel 接 public state/DTO | 只确认此共享接口边界，不替其他宿主全协议或 private diagnostics 背书。 |
| 18–19 reversed empty coverage finding/remediation | `apps/web/src/host/company-query-coordinator.ts:188-205` 先拒负、不安全、逆序，再核前序、连续事件、尾序；测试保留逆序拒绝 | 旧游标回退缺陷有当前生产守卫反证；不复报。 |
| 21 residual：旧 219/220 suite 的 WASM Map restore failure | `apps/web/src/host/serde-normalize.ts` save/restore Map 桥与恢复测试 | 旧测试计数只是历史。Map 跨 JS/Rust 的桥接与 malformed key reject 当前存在；未在本轮重跑。 |
| 22 residual：civil-date/second primitive validation 由 Rust contract 提供 | `apps/web/src/host/public-report-normalize.ts` 公历往返、second-of-day 范围、发布/批准时间顺序校验 | 当前前端校验已超出旧 review 所述 primitive-only 范围；不成立为遗漏。 |
| 24 rendering 留 Task34 | `apps/web/src/components/company/CompanyPanel.tsx` 的报告状态/表格/说明分支 | 后续已落地；不把有意分期视为 Task33 欠实现。 |
| 26 Final APPROVE after remediation | 历史结论 | 当前复核仍支持其 Task33 局部结论，不代表整批/全应用已重验。 |

## Task 34 全章矩阵

| 原文行/章节 | 当前代码/证据 | 复核 |
|---|---|---|
| 3–4 日期与 scope | React、App/mobile、start-date parser、production-preview E2E | 历史审查范围。 |
| 8 Independent finding：WASM `next_cursor` undefined、`supersedes` 缺失 | `apps/web-wasm/src/lib.rs:33-36` 的 `public_dto_to_js` 显式 missing-as-null；`:404-423` page 与 by-id 均实际调用；engine `Option` DTO 未跳过字段 | 上游 producer 修复已接到两条公共查询路径；strict normalizer 仍在，UI 没有吞掉不完整 DTO。旧 REQUEST_CHANGES 不能直接作为当前阻塞。 |
| 9–10 UI 不改语义、ready/error/unavailable/empty/loading、exact decimals、compare explanation、sticky table | `apps/web/src/components/company/CompanyPanel.tsx`、财务展示组件与 `public-report-normalize.ts` | 当前实现保留显式错误与精确字符串值，缺少比较期仍不当作零；没有证据显示 UI 自行推导账务事实。 |
| 11 start-date 2000–2099/default 2030/共享 lifecycle | start-date parser/setup 与 App 的同一 SessionSetup 启动流程 | 现行入口仍共享日期校验与 setup，历史一致性结论成立。 |
| 13–20 历史 test/type/lint/build/E2E/screenshots | review 和 happy/failure evidence 记录的是 2026-09-12/13 的运行；当前 E2E 源仍包含真实 WASM 报告 ready 与非法 page-size 显式错误场景 | 不把旧通过数字冒充当前运行，也未重新验像素。 |
| 22–24 REQUEST_CHANGES 原因与禁止 UI workaround | public WASM serializer 修复 + strict Worker normalizer | 对 nullable 传输采用 producer 修复；历史限定的阻塞现已解除。 |

## 当前公开资料与图表语义

- “公开报告”调用 `Session::query_public_reports` / `public_report_by_id`，展示当前模拟自然日可见的已发布材料；CompanyPanel 读到的是 public DTO。公司目录是游戏内虚构公司（`apps/web/src/components/company/company-catalog.ts:9-18`），不应把这些报告描述成现实上市公司的真实披露。本复核没有发现 UI 将游戏报告冒称为真实交易所公告。
- 报告输入保留独立 `period`、`kind`、版本和 `supersedes`；normalize 校验 scope、版本关系、日期、exact decimal 和比较期的 tagged availability。不能据此说当前模拟报告符合现实监管报送制度；这不是这三份 review 所检验的交易规则变更。
- `PriceChart.tsx:19-28` 的日 K 来源是 Rust Snapshot 已完成日K；`tradeStats` 只在逐笔可对账日存在，预置合成历史不伪造该统计。`price-chart-indicators.ts:13-24` 日K指标取 OHLC/close 和日成交量，窗口裁剪发生在指标计算之后；量柱按 `volume / 100` 显示手数。分时使用价格序列。上涨红、下跌绿由收盘与开盘或 MACD DIF/DEA 关系着色；本次没有改色彩规则。
- 图表语义范围没有新候选：此处核对的是字段单位、真实/合成样本界线和日K/分时输入选择，不把第三方 TradingView 标识、历史截图残片或未重新目视的响应式观感臆断为当前缺陷。A股交易制度没有本轮变更，也未以非官方规则来源作制度结论。

## Task 36 全章矩阵

| 原文行/章节 | 当前代码/证据 | 复核 |
|---|---|---|
| 1–4 review rounds / reviewer attribution | 原文记录两轮独立只读 review | 历史过程。 |
| 6–10 Fixed 列举 | `session/pipeline/auction_day_end.rs` 的实际 auction receipt/成交身份；`diagnostics/causal/aggregate.rs` 对事件来源、交易量、value、endpoint 和守恒校验；`session/observation_clock.rs` 被 decision-chain/causal 共用 | 修复路径仍在。receipt 聚合的是已提交真实 fills，不伪造 acquisition 或 settlement；本轮不运行其历史 tests。 |
| 12–15 Accepted：midpoint observational、auction 无 aggressor、连续同股方向、depth-loss/recovery | `diagnostics/causal/microstructure.rs` 的分股方向、首个合法后续 midpoint、auction 标记与 depth censor 逻辑 | 语义是观察指标/删失原因，非反事实“成交影响”；成立。 |
| 18–21 Remaining 午休 clock issue | `session/observation_clock.rs:4-33`；`decision_chain.rs` 决策观测实际调用，`session/causal.rs` 共享同源 instant | 共享权威 clock 已修；后续 review 明确 supersede 旧 qualification。 |
| 22–23 gross-overflow 旧 auction 分支未发 AuctionCompleted | 当前 auction 先校验后在成功路径发布完成事件；失败走显式错误/transaction | 旧分支背景不能推导溢出时必须发成功完成事件；未形成现行错误行为的新证据。 |
| 24–25 `filled_value_before=0` | `session/pipeline/stock_auction.rs` settlement audit 使用实际 before/after；auction diagnostics 从 receipt 读取 value before/after | 旧零值观察不适用于当前实现。 |
| 26 first-stock calendar selection | session create/restore 同用首只证券 calendar policy，civil clock 文档说明现阶段沪深共用策略 | 是显式既有政策，不从 review 推导本批要改；不核销其他总账交易日历覆盖问题。 |
| 28–30 attribution clarification | causal submission 用真实 `OrderOrigin`、account/stock/plan/当前 decision；真实 execution caller 在 adaptive plan chain/auction path | 只支持“提交时当前 decision + PlanId 父关联”，不承诺计划所有 revision 的完整因果重建。 |
| 32–33 verdict 与 clock qualification | 35 行之后的 re-review 明确接受 clock repair 并 supersede 前述限制 | 保留过程时序，不把旧 REJECT/残余意见当当前事实。 |
| 35–48 Source Clock Repair Re-review | shared clock source、午休秒跳与 legacy restore 字段回归测试源码仍存在 | 核验到修复接线；5+4 等通过数字是历史记录，不是本轮执行结果。 |
| 50–54 nonblocking observations | rollover、压缩 session constants 的维护/专门断言建议 | 建议测试不等于已发现的错误行为；不新增生产缺口。 |
| 56–57 handoff | 要求 primary 最终接受后再改 checkbox | 协作门槛，不是产品功能；本轮未改计划。 |

## DEV、真实订单与负控 caller

- DEV inspector 是独立 trace collector：`session.rs` trace state/方法由 `simulation-diagnostics` 控制；WASM `host_capabilities` 只在 `simulation-diagnostics + debug_assertions` 报告能力，`:455-460` 导出同样双门控。发布隔离的负控/导出检查约束的是 private 信息边界，不能证明 trace 内容已关联订单。
- G37 仍成立，且与 Task36 causal attribution scope 区分：生产 root caller `packages/engine/src/session/plan_chain_candidates.rs:376` 调 `record_plan_root_diagnostics`；`decision_chain.rs:764-770` 实参仍是 `&[]`；`:800-810` `order_ids` 只从给定 `OrderAccepted`/`OrderCanceled` events 提取。该调用点的 record 因而永远得不到随后实际 order IDs。另一个提交 causal ledger 有真实 order origins/fills，不是 DEV inspector 的 consumer，也不核销此问题。
- DEV 记录其余字段的承诺也有限：`:812-817` `budget_constraints` 是当前 terminal plan status 的派生，不是候选分配过程实际拒绝/约束明细；报告仍以“Budget/状态”展示，不能把它误读成历史约束事实。只将“真实订单 ID 断链”保留为既有 G37，不另立重复发现。
- Causal negative controls（复制 submission/更换 owner、伪造 fill 或篡改 execution price/budget）是传入 `aggregate` 后必须拒绝的伪造输入，验证的是拒绝/守恒边界；不是生产 caller 已输出坏数据的证据。restore 后显式 restart 记录同样不应伪造缺失历史。

## 旧结论复核与候选反证

| 旧/候选结论 | 复核 |
|---|---|
| Task33 逆序空 coverage 可让 cursor 回退 | 已由生产输入守卫先拒绝，再推进 state；测试有负控，旧发现已修。 |
| Task34 WASM nullable 序列化阻塞 ready UI | page/by-id producer 均显式序列化 null；消费者仍 strict；旧阻塞已修。 |
| Task33 前端缺少日期/日内秒校验 | 当前 public normalizer 已执行公历、秒数及时间顺序检查；旧残余已覆盖。 |
| Task36 decision-chain 午休时间戳错误 | 生产 decision 与 causal 读取共享 observation clock；旧 issue 已修并有 re-review。 |
| Task36 auction settlement 前值为零 | 现行生产 receipt audit/诊断都取真实累积值；旧 issue 已修。 |
| causal ledger 有真实 order ID，所以 G37 不存在 | 负控/反证无效：这是不同 consumer。DEV root 仍传空 events；维持 G37。 |
| 新候选：公开报告实为真实市场披露或日K合成交易统计冒充真实 | 目录明确虚构；合成历史的 tradeStats 不伪造，未发现此类冒称。 |
| 新候选：历史 chart TV 片段或 E2E 测试记录证明当前视觉坏/好 | 没有本轮当前像素观察；两者都不能从历史证据外推，保持为未重新目视的验收边界而非生产 bug。 |

结论：三份 review 的旧阻塞项在当前源码中均有修复反证；唯一仍成立且与这些修复相互独立的是既有 G37（DEV NPC trace 未接真实订单事件）。未发现新独立漏实现。本审查不改产品、Git 或正式计划，不报告未运行的测试结果。
