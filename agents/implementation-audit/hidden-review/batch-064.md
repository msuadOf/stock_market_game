# Batch 064 隐藏复核

## 范围与来源校验

复核基线为产品 worktree `.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；来源根为主仓。先读主仓 `AGENTS.md`、`docs/principles.md`，再按 scan-plan batch 64 的三项来源从头读至 EOF。行数与 SHA-256 均匹配：

| 来源 | 行数 | SHA-256 | 章节范围 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-02-manager-delta.md` | 26 | `7286017b37d84c2d1aa5138e5e4b0f306d8a5c69bf79abb9524fcf3c2ad31b97` | ES02 管理者增量：A03 分类、缺陷/保留项及单元记录一致性 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-02.md` | 13 | `910eaad88597795a57c107faaff77dadba1978710b5fd0398c232fa603f2a3a3` | ES02 最终复核入口及其继承边界 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-03-manager-delta.md` | 27 | `1288105e2ad99f99f8435812b37074eed36a274304d753e2d1eebec46d3f04eb` | ES03 A01/A02 增量复核、输入所有权及 continuation 边界 |

另读当前 ES02/ES03 的 items、modules、实现审计 G01–G68/Q 主账、相关 ADR 与生产 caller/consumer。仅静态读取；未运行测试/构建、未做 Git 写操作、未修改产品或调查源文件。此报告及配套 JSON 是本批新增工作记录。

## 当前 owner 与 caller

- **ES02-A03 `SessionCandleBook` 已在基线落地。** `packages/engine/src/session/candles.rs` 定义其 `histories`、`active` 两个私有 map；`record_trade_or_mark` 归并活动 OHLC 更新、零量昨收占位替换与 `DailyTradeStats` 记录，`commit_active` 负责日界追加历史。`GameSession` 方法现为窄转发器，连续撮合 finalizer 与 `auction_day_end` 仍是实际成交/日界 caller。snapshot、minimal snapshot、protocol/save restore、hash 仍读取/写回两个独立字段。故 ES02 items 的 action `engine-session-02-A03` 在候选文本中的 `status: proposed` 对 43b1aa5 已过时，应标记为已实现基线能力；本批没有发现需要新增的 candle owner 抽取。
- **ES03-A01 `RootReadContext` 与单账户 root 已在基线落地。** `decision_chain/roots.rs` 定义只读上下文与 `InstitutionDecisionRoot`；上下文持有账户、公司注册表/经营事实、公开资料库、PlanBook 和时钟事实，不持有市场簿、历史、个人状态 map 或委托游标。`plan_chain_candidates.rs` 的 `start_ready_accounts` 捕获并共享 `Arc<RootReadContext>`，取出账户私有 `PlanPersonalState` 后调用 root；结果由原协调器接收、装回私有 candidate 并处理 typed operations。`DecisionChainObservation` 仍单独承载冻结市场/路径/技术观察。A01 提案的核心输入分离已实现，不应当作待实施动作。
- **ES03-A02 的 continuation 保留边界有现行 caller 支持。** 当前 operation batch/adaptive coordinator 依 `(account, code)` 路由身份跟踪等待项，消费 typed route outcome 后操作当前私有 candidate；生命周期动作未塞入 root 返回值。该条在 items 里本来就是保留边界而非 action，没有反证要求改变。

## 旧结论与现行缺口

ES02/ES03 报告是历史审阅入口，明确依赖先前完整源码复核或限定 delta；不能把其“候选仍为 proposed/未实施”状态直接套用到指定产品基线。当前代码直接反证了 ES02-A03 与 ES03-A01 的候选未落地状态。更准确的复核结论是“候选已实现，保留交易/存档契约，不需新抽取”，不将它们描述为尚缺实现。

ES03-A01 的落地**不核销 G16**：`RootReadContext::capture` 仍 clone 完整 `PlanBook`，总账记录的长期/全量复制成本边界仍开放。A03 也不核销与 candle 无关的 G 项。没有发现这三份材料足以确认或解决任何 Q 条目。

## G01–G68 / Q 交叉核对

- G01–G15、G17–G68：本批两项 owner 候选与这些宿主、市场/交易策略、UI、公司闭环及验收工具缺口没有对应生产修复，不核销；G27 沿用主账既有核销状态。
- G16：`RootReadContext` 把此前的部分 `GameSession` 输入收窄，但其 `capture` 仍复制完整 `PlanBook`。现行性能/历史复制缺口继续保留；不能以新对象存在或 worker 并行宣称已解决。
- Q01–Q23：本批候选没有相关决策或实现变更，不核销、不重开。没有从 OOP owner 结构推导新的 A 股规则。

## ADR 与领域门禁

ES02-A03 所引 ADR-0011/0018 与当前 candle 行为、完整历史目标之间没有发现冲突；ADR-0018 仍为 proposed，不能把其中未接受部分转成要求。ES03 的隔离/暂存设计与 ADR-0017 的 candidate/提交边界相符。也对照 ADR-0019、ADR-0023–0028 的最新范围、存档、个体经验及发布决定；没有一项取代这两个内部 owner 的职责，也没有改变本批结论。相关源码把成交量以股数、金额以分记录；首笔真实成交替换零量昨收 K，日界再提交完整历史。此处只审阅既有语义是否保持，没有提出制度新主张，也未重新查询交易所/中国结算规则。

## 结论

三份来源的冻结指纹匹配，来源历史结论的 caller/边界描述与基线相符，但候选实施状态已经陈旧：ES02-A03 `SessionCandleBook` 和 ES03-A01 `RootReadContext`/root 均已在 43b1aa5 生产路径中落地。ES03-A02 仍为协调器/当前 candidate 的保留职责。无新增 OOP 抽取或交易行为修复建议；G16 继续开放，其他 G/Q 不因本批核销。未运行测试、构建或官方规则查询，不能据此报告实现验收通过。
