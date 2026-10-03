# Task 24–26 EOF 全文复核（当前树 08e4fc7）

审查人：luna27（未参与原实现；只读产品树）。范围：完整 EOF 阅读 `.omo/evidence/company-information-npc-intentions/task-24-review.md`（52 行）、`task-25-review.md`（191 行）、`task-26-review.md`（260 行），并按章节追查当前披露/发现、策略迁移、计划执行和 restore caller。原则依据：工作树 `AGENTS.md` 与 `docs/principles.md`。本记录只审计，不改产品文件、不运行测试。

## 章节覆盖矩阵

| EOF 文档章节 | 当前核查对象 | 历史结论复核 |
|---|---|---|
| task-24 §1–7：交易语义、执行通路、优先级、可达性、数量边界、质量、范围 | `session/plan_execution.rs`、`actions.rs`、`routing.rs`、`synchronization.rs`、`execution/records.rs`；当前延续测试 | 原 APPROVE 的核心语义仍有实现和锁定测试支撑；关注的 transient pending 事件已由 task-27 转为显式保存契约。 |
| task-24 §8：移交 task 26/27 | task-26 修复轮、task-27 review、`compatibility-removal.md`、当前 SaveSlot/restore | 前瞻问题大多已由后续提交封闭；保存前需同步的要求后来具体化为必填持久化状态及恢复交叉校验。 |
| task-25 §一–四：语义/范围/测试、候选权重和 watchlist | 当前 `attention.rs`、`experience/watchlist.rs`、`decision_chain/roots.rs`、发现/曝光测试 | 原行为与边界仍一致；接线已由 task 26 实施。 |
| task-25 §五–八：F1 登记拒绝、证据对账、观察、结论 | policy fixture、正式文档、manifest 测试；全文阅读 re-verification 段 | 最新 b2d88a9 翻转结论优先：参数在正确基底登记并复验，F1 已关闭；早期 a885ac1 错基底事故为历史事件，不是当前代码缺失。 |
| task-26 初轮 §发现 1–5、正向项、REJECT | 当前市场测试、决策链、移交清单 | 初轮 REJECT 已被第二轮 d2080fb 取代；不得把其旧缺陷当成当前缺陷。 |
| task-26 R1–R5、第二轮结论 | 当前恢复的 market 测试、日中撤单、证据文件、issues/compatibility-removal | 阻断测试回归已修复；日中撤单已落实；E/残留已不存在；pending 语义由任务 27 定型；无新旧版本冲突阻断。 |

## 旧发现和当前证据

### Task 24：母单、执行器与同步

- 执行器仍复用权威路由：`packages/engine/src/session/plan_execution/actions.rs:1` 经 `route_plan_intent` 进入真实路由；`routing.rs` 检查 parent/link 关联。成交事实仅在 `execution/records.rs:7` 的结算后记录，`filled_qty` 不由报价或接受事件推进。
- 旧审查锁定的采用规则、撤单/重报、板块手数、卖出可用量、冻结与日终复活，没有发现被后续重构绕开的第二撮合通路。当前 `continuation_tests.rs` 保留 late route outcome、active child、真实取消等断言；执行测试亦覆盖 adopted order 与 stale submit。
- 旧 O(8a) “无子单计划可能不收到 DayEnded”是原执行适配器和 task-26 完整链路的职责边界。当前完整决策链日终由 pipeline 推进，且 task-27 改变了计划存档/恢复契约；不据原文字面推断当前未推进。原 O(8b) “pending 队列不入档”已由 K7 改为入档并校验，见下。
- 原 O(8c) “host 直接终止仍有在途子单可能令同步卡死”在 task-26 d2080fb 被封闭：`decision_chain.rs:4029`、`:4068`、`:4188` 的回归测试分别锁定反向先撤单、终止先撤单、不可撤时保留计划/子单。公开 lifecycle 先撤后修复，不再是原始宿主路径。

### Task 25：披露曝光、个体发现与关注迁移

- F1 最新决定是 APPROVE。当前 `packages/engine/src/session/attention.rs:99` 的公共输入明确为 MarketView 和公开曝光；异常 2%、2.0x 和 +1/+1/+2 参数仍与 `packages/engine/tests/fixtures/company-model/policy-sources.json:590`、`docs/company-accounting.md:156` 一致。manifest 测试仍是该 fixture 的校验入口。故初审“新假设未登记”已被正确修复，不是额外发现。
- 原复核所说“曝光仅候选，不等于已读”当前有真实接线：`decision_chain/roots.rs:311` 抽样发现并登记 `record_attention`；`:349` 调 `discovery_candidates`，`:360` 才 `record_acquisition`。获取列表仍按公司与 `as_of` 公共发布时间过滤（`information/acquisition.rs:266`）。未见读取私有经营账来加权或前视到未来公告。
- 新鲜度窗口仍为 2 个自然日（`decision_chain.rs:136`、`:257`），issuer 映射取自 registry；这与 task-25 O-1/任务 26 登记口径一致。候选股票排除非市场项，关注/发现池按 StockCode/BTreeMap 定序，和已登记说明吻合。
- watchlist restore 仍走 `PersonalWatchlist::from_parts` 校验，attention 时钟单调；protected 集合通过调用方组合持仓与活动计划。当前发现流程先以 held / watchlist / 全市场抽样，再写实际关注，未把公共曝光直接写入信息集。
- 章节中早期“任务 26 尚未接线”和 F1 拒绝是按时间记录的原始裁决，不能覆盖后续 re-verification。最新认可建立于正确父链的 b2d88a9；当前树也确实包含源码、fixture 和文档三方。

### Task 26：策略迁移、日终与恢复边界

- 初轮 market.rs 删除造成的 A 股语义锁缺失，已由 market 目录测试补回。当前 `packages/engine/tests/market/price_limits.rs:8` 锁正数 half-up 和最小 tick、参考价优先序、低价十 tick、限价带与边界；`main.rs:38` 锁 daily limits。初轮错误“约 14”已由复核人更正实际测试数，修复轮逐字比对并确认保留非 V 断言。未发现新的涨跌停/价格笼子覆盖回归。
- 日中变向/终止的真实撤单已落实；不可撤阶段维持计划和委托，避免悄悄释放冻结。其规则与集合竞价/收盘集合不可撤窗口一致，属于执行生命周期行为，不引入新 A 股撮合规则。
- task-26 原“pending 队列迟到事实无 drain”问题需读 task-27 最新契约：`session.rs:2691` 存档只保留活跃计划事件；未知/已终止事件明确丢弃；`persistence.rs:1438` 校验 parent-plan link，`:1453` 校验 pending event 目标计划/订单/日期；`compatibility-removal.md` §2–3 明确没有 legacy migration，必填状态缺失即拒绝。`synchronize_owned_plan_execution` 仍原子应用待处理事件，完成 fill 后清队列并移除 linked parent；对应 continuation tests 锁定失败不变、完成后清理、未知 plan 明确拒绝。旧的“任务 27 待定义”已被这些较新决定替代。
- restore caller 现在不是“重建后复位计划”的过渡语义：`GameSession::restore` 先 `validate_save_slot`，随后恢复 SaveSlot 中完整 PlanBook、个人信息/信念/watchlist/price-memory 和 pending facts。`compatibility-removal.md` §1–3 规定 required state、无迁移器、互洽校验；`tests/save_contract/main.rs:188`、`:212`、`:264` 覆盖全状态往返与 restore continuation。此演化为任务 27 明确收口，未发现引用旧语义的当前调用点。
- 初轮发现 5 的 company_assembly 体积登记已见 task-26 R5 修复轮声明；属于过程性 LOC 说明，不构成行为遗漏。修复轮提到的仓库根 E/在当前树不存在（`git ls-files E` 和路径检查为空）；`.omo/evidence` 中 task-26 证据文件存在。

## 新候选与结论

- 未发现新的阻断性 A 股语义漂移、跨层披露/获知混淆、策略迁移遗漏或 restore caller 旁路。
- 最值得强调的“表面疑点”均已有更新证据闭合：F1 参数登记、task-26 market 测试恢复、日中撤单、pending event 保存/过滤契约、K7 restore 连续性。不可再将早期 REJECT/过渡观察作为当前状态。
- 本轮只完成全文和代码审查，未运行测试。现有 task-25/26/27 reviewer 证据在各自记录中；不作新的测试通过声明。
