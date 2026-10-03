# Luna57：OrderBook 与 RealEstate 复核记录全文扫描

- 日期：2026-10-03。
- 对象：产品 `08e4fc7`（merge worktree 同一实现）；指定三篇记录至 EOF，共 253 行。
- 方法：通读 AGENTS、`docs/principles.md` 和三篇指定记录；沿记录中动作、旧发现、真实生产 caller 与测试位置只读追查源码。检查领域语义时，以现行 A 股概念/单位和仓库正式交易约束为准；地产会计沿用其结果记录引用的正式会计依据。本轮未联网重验法规、未运行测试/长测、未写产品源码或执行 Git 写命令。
- 范围限制：这是针对三个工作记录及对应核心实现/caller 的全文扫描，不是全仓完整 diff 再审；没有把历史记录中的测试通过或静态结论写作本轮执行证据。

## 记录全文/章节矩阵

| 文档行号/章节 | 扫描内容及当前 caller 核对 | 结论 |
|---|---|---|
| `orderbook-review.md:1–13` 范围、主体结论 | Account/Market/OrderBook 动作、baseline、复核边界和“未运行全量”的措辞与文档后续证据边界一致。 | 未把局部审阅冒充全仓/运行验收。 |
| `:14–19` 大 A 语义 | `Money` 单位为分，委托量为股；`OrderBook` 价时键仍是买盘 `(Reverse(price), seq)`、卖盘 `(price, seq)`，成交取 maker 报价。成交优先级不取决于 ID treap 的 priority；A 股 T+1、资源/费用 owner 不在该订单簿。规则依据仍只引用 `docs/trading-rules.md` 既有来源/日期，并披露没有本轮联网复核。 | 语义边界和依据限制表述相称。不是官方规则最新性认证。 |
| `:20–29` 动作必要性 | A03 私有化账户状态、N02 进度校验、N03 限定价格写入、N37 `BookState` 聚合、N38 treap 重用，均没有把撮合顺序或资金责任移交给身份索引。serde 字段/形状保持是记录所述约束。 | 变更意图聚焦；未发现记录声称新增业务能力。 |
| `:30–39` 边界和复杂度 | 逐查 checked 金额/数量、`Arc::make_mut`、恢复顺序、maker/taker 写顺序、簿身份转换、cancel/restore/apply_changes 逐项操作。`book_state.rs` 中 `apply_changes` 先校验全部 before/after 期望，再逐条 cancel/insert；因此前置 projection 校验不等价于整个变更批次原子提交。 | 文档已说明后续插入失败可留前序变更；不可泛称该 API 或 tick 候选具备回滚。 |
| `:40–46` F1 | 旧测试同时令 maker 与 taker overflow，无法直达 taker 分支；当前 `state_contract_tests.rs::taker_money_overflow_leaves_book_status_and_cursor_unchanged` 让 maker 正常、taker 从 `i64::MAX` 累加 2 分，并检查簿、filled 身份、游标。 | F1 的测试缺口已由静态读到的针对性断言覆盖；本轮没有执行测试。 |
| `:48–71` caller 待办与追加核销 | 记录列明的 AccountBook、persistence、Market 及 failure/decision-resource setter caller 后有迁移核销；Market 三个价格限制测试迁入 `#[cfg(test)]` 内部模块，原 10 个 case 的 7+3 分布及执行 filter 警示清楚。当前 `Market::place` 仍调用 `book.place`，连续交易 `place_recording` 也经 Market 到 OrderBook；tick 路径的真实候选入口见 `continuous_matching.rs`。 | 对已列待办有闭环证据；不将“caller 已迁移”推成全仓编译证据。 |
| `:73–88` 指纹及验证边界 | 文件 hash 表、旧 hash 说明、未运行 Cargo/测试的限制直至 EOF 均已读取。 | hash 是该记录所绑定的版本证据，不替代本轮 diff 或测试执行。 |
| `real-estate-result.md:1–18` 逐动作核销 | N04 开局 guard 在 `RealEstateBooks::new`；N22 暂态 `BorrowingCostAccrualPlan` 由公开 `accrue_interest` 实际创建/消费；N25 lifecycle guard 在 `ProjectState` 并由 development 命令调用；N26 单贷款 guard 由 debt service 调用；N27 `PresaleContract` guard/余额由 Books collection 与 delivery 调用。上游 operations/dispatch 仍进入 Books 命令。 | owner 迁移有真实生产消费路径，不是无 caller 的孤立方法。 |
| `:19–31` 测试与验证状态 | 记录区分先写测试与实施顺序，但明确迁移前 baseline 未跑；指定 Cargo filter/结果属于实施记录或 root 汇总，而非本 reviewer 执行。 | 保留其 TDD/验证证据限制。 |
| `:32–48` 语义/失败面 | 阅读对计息 split、ACT/365F 双余数链、事件槽、过账顺序、零天/零金额、项目/预售与资金隔离的逐项说明。`accrue_interest` 的 post → loan apply → project 汇总/apply 顺序是明确的部分失败边界；其记录没有宣称跨账全原子。 | 单位/owner/失败语义在记录内一致；A 股撮合/T+1 未被地产改动触及。 |
| `:49–69` 门禁及复核回填 | 实施者与独立 reviewer 身份有区分；RE-R1 后续修正后的 fixture 为开发日 date(2)、完工日 date(1)，断言早于关系，且生产 guard 未改。最终明确静态复核不是运行结果。 | RE-R1 修复闭环成立；独立报告与结果记录相互对应。 |
| `:70–96` 最终版本绑定至 EOF | 完整 baseline diff 字节数/hash、9 个生产文件和测试 hash、reviewer 本人取证/读取限制均已逐项读取。 | 版本绑定仅在其后文件未变时适用；SHA 不能证明测试通过。 |
| `real-estate-review.md:1–9` 审查边界 | reviewer、baseline、完整地产目录 diff 与 ownership tests 范围及读取 caller 的声明清楚。 | 不外推到全仓其他改动。 |
| `:10–19` RE-R1 | 旧 fixture 开发/完工同日，无法保护早于开发的完工接受；修复只改测试 fixture 与状态断言。 | 初始问题被精确定位，后续修复回填可证伪。 |
| `:20–43` 三项门禁 | 大 A/会计依据、需求必要性/API 与 serde 保持、各动作跨层 owner 被分别论证。 | 结论遵守最小范围；地产假设不冒称 CAS 参数全覆盖。 |
| `:44–57` 边界、发现及运行限制 | 分录次序、首错、失败写入范围逐条列出；记录承认极端项目汇总/应用分支主要静态对照，并未运行测试。 | 诚实披露剩余证据边界。 |
| `:59–66` 修复后复核 | 复核明确读取最终 diff 与测试全文，日期关系断言已有效，未找到新的有效问题。 | RE-R1 关闭有具体修复证据。 |
| `:68–96` 最终版本绑定至 EOF | reviewer 以完整 diff hash 和各文件 hash 绑定版本；明确未运行 Cargo。 | 绑定避免对后续不同版本自动继承结论；不升级为执行验收。 |

## 旧发现与旧结论复核

- **OrderBook F1（orderbook-review.md:42–46）：** 旧测试对 maker 金额溢出优先顺序的局限属实。新增 taker-only overflow case 的夹具满足订单进度，错误断言检查 `MAX + 2`，并比较状态与 `next_sequence`；静态看确实覆盖目标分支。没有声称用例实际运行。
- **RE-R1（real-estate-review.md:10–19、59–66）：** 初始同日 fixture 不能证明早于开发日的接受边界。修复后日期严格早于且生产 guard 未变，断言把目标语义纳入覆盖，发现可关闭。
- **已知 OrderBook 部分失败面（luna16.md:26）：** 旧扫描确认多档撮合中较后金额溢出可使此前 maker 已变更；当前 `OrderBook::place_inner` 仍边撮合边改 `BookState`，`Market::place` 仅在簿成功后更新 last price。实际 continuous caller 在 private tick candidate 上处理；fatal tick 最终由外层事务丢弃候选，不等于裸 `OrderBook`/`Market` 单次方法有强 rollback。此行为并非本次抽取引入，正式本批也没有升级该局部 API 的原子性承诺，故不作为本批新增缺陷；报告仍应保留该边界，不能说“OrderBook 方法失败无写入”。

## 正式契约校验：数量/价格、资源和原子范围

- 订单簿直接承诺限价价时撮合、合法数量进度与显式错误；`OrderBook` 无账户现金、持仓或费用对象。账户预算/可卖股/T+1 在上层校验、封存/资源 reservation 与结算路径负责。因而不能用“裸 book 无余额”判作透支；反过来也不能仅凭订单簿测试证明实际账户路径不透支。
- 大 A 的分、股单位及 maker price、同价顺序与现有规则/ADR 一致。各市场阶段和 securities 的交易规则由正式规则文档/上层阶段处理；本次只确认抽取未移走该 owner，不重新裁定规则真实性。
- ADR-0017/0018 的 tick atomicity 属于 authority 的 shadow candidate/commit 边界。单个 book state 的 `apply_changes` 或地产跨账 `accrue_interest` 都不该据“候选/plan”命名推导为端到端原子；地产记录已明确 post 后部分失败。检查本批残余风险时，应保持局部方法失败面、业务拒单和权威 tick fatal rollback 三者的层级区别。

## 新候选与反证

未发现可成立的新产品缺陷候选。

| 表面疑点 | 反证/处置 |
|---|---|
| `OrderBook::place` 可能在后段失败后保留前序 maker 修改，是否违反不透支/原子性？ | 这是局部簿操作失败面的已知旧行为（见上）；资源校验不属于该类型。生产 tick 以 private candidate 隔离，fatal 不发布 authority。正式动作没有要求本批增强裸 API 原子性。应继续如实限定承诺，不登记重复 G。 |
| 订单簿没有余额/可卖股份字段，是否漏掉资金透支保护？ | A 股账户资源检查在上层路径，ADR-0018 §7 明确同账户实际现金/股份争用按接收顺序裁决。订单簿不承担账户账本是分层设计。不能把它当做 caller 路径已经通过运行测试的证据。 |
| `apply_changes` 全部校验后逐项写，后项写失败会局部变更，是否违反事务原子？ | BookState 层写入错误可能在已校验的逻辑状态异常下发生；既有测试故意保护顺序与部分写入。外层 stock worker 候选提交路径另有事务边界，不能将接口局部提交等同 authority 提交。若未来要改变其契约，需单独授权与测试。 |
| 房地产地产 guard 只读，计提计划是否令 posting 全路径原子？ | 不会。`BorrowingCostAccrualPlan` 仅规划本次数据；其结果文档明确 post 后 loan apply/project aggregate 仍有部分失败边界。记录未做过度核销。 |
| 最终 hash 或独立审查结论是否足以报告全部测试通过？ | 否。三篇记录均明确区分静态复核、版本绑定与 Cargo/产品测试执行；本轮也未运行测试。 |

## 结论

指定三篇记录已完整读至 EOF，动作与真实 caller、旧 finding 修复、契约层级和残余失败边界互相一致。F1、RE-R1 有可核对的修复后测试/断言文本；没有新确认遗漏。A 股依据沿用正式文档而未本轮联网更新；产品运行测试结果不在本报告验证范围内。
