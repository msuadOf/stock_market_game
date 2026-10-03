# sweep26：任务 20/21/22 历史复核的当前实现闭环

## 阅读范围与结论

- 已连续全文读取 `.omo/evidence/company-information-npc-intentions/task-20-review.md`（92 行）、`task-21-review.md`（24 行）、`task-22-review.md`（29 行），共 145 行；三个 verdict 与前后修正一并读取，没有只搜索 REJECT 或 APPROVE。
- 生产代码以 `b76ece3` 的合并同等产品为基线；检查沿当前 plans/experience owner、session root、P3/P4 typed 回写、Settlement 与恢复消费者展开。沿用先前已读的根 AGENTS、principles/open-questions 及总账。没有产品修改、Git 写操作、构建或测试运行。
- 本批没有确认旧总账遗漏的新 G。任务 20 的散户 dated writer/日期衰减后续义务仍未闭环，已由 G08 精确覆盖；任务 22 新旧预算类别生产构造问题仍属 G38。任务 21 的历史到期不可达缺陷和任务 22 的首轮缺陷已在当前源码修正，不能恢复为新缺口。

## task-20-review：92 行逐章

| 原文条款 | 当前代码与消费者 | 状态 |
|---|---|---|
| `task-20-review.md:10` 隔离重跑、`:21` 数量对账 | 历史记录的 25/7/695 及 sibling 测试数量属于当时 commit 的证据，不是现行 b76ece3 测试结论。当前源码仍有 `packages/engine/tests/experience_feedback/` 分组用例。 | 历史证据保留；本轮未重跑，也不因数量变化判删功能 |
| `task-20-review.md:27` 三问，`:29` 心理机制与三时钟、类型化错误 | `packages/engine/src/experience/feedback.rs:33` 固定 20 交易日档位，`:42` ExperienceMoment 独立保存 civil_date/market_minute/trading_day；`feedback/lifecycle.rs:341` 正价和时钟守卫，`:343` 缺持仓返回 NoActiveEntry。 | 模块能力存在；120 市场分钟心理冷静期不是交易所制度 |
| `task-20-review.md:32` 最小范围与复用真实计数，`:35` 生产接线移交 | `feedback/lifecycle.rs:282` dated fill 调 legacy real-fill helper，`:294` 成功后登记时钟；`:358` dated observe 先执行真实观察再登记日期。新事实是 feedback，不再复制账户成本/损益。 | 写入模式保留；生产接线不能由 helper 通过替代 |
| `task-20-review.md:38` K5 锚点：失败日期、部分填去重、20 日衰减、清仓保留、被套 | `experience/feedback/inputs.rs:13` 从最后真实失败日期推档，饱和但不删 failure facts；`:25` 本人最后观察低于调用者当前成本且 held_days≥20；`feedback/lifecycle.rs:335` 镜像 95% adverse 守卫、`:363` 只在确认新受挫时追加事件。 | 纯模块已实现；散户生产部分未接入，见 G08 |
| `task-20-review.md:51` 验收/失败电池：相同损益不同经历、加仓/再入、无成交不失败、Panic/LongTerm | `tests/experience_feedback/seam.rs`、`main.rs`、`reads.rs`、`failures/clocks.rs` 保留相应模块测试；生产散户读 `behavior/heuristics.rs:122` 的原始 consecutive_failed_buys，真实成交在 `session/pipeline/retail_projection.rs:519` 才写 legacy experience。 | 原有心理/真实 fill 有生产消费；不能称日期衰减已被消费 |
| `task-20-review.md:67` 登记偏离 1：三派生接口转交 22/23，`:73` 零生产调用声明 | `plans/allocation/experience.rs:20` 确有 `failure_influence` API 委托，`:27` 委托 is_long_stuck；session 的预算请求 `decision_chain.rs:978`、`:1242` 仍用 AllocationExperience::default。散户 `decision_snapshot_capture.rs:347` 调 observe_position，`retail_projection.rs:519` 调 record_fill_with_order；`session.rs:1540` 开局调用 initialize_holding。 | 三类散户 writer 没全切 dated，明确保持 G08；不以 task20 APPROVE 证明整体完成 |
| `task-20-review.md:70` 空 feedback/旧字节及 TS 移交 | 当前 save-schema 由 `session/persistence/v2.rs:97` 与 `:109` 显式拒绝缺失/旧/未来版本；Experience 结构内可选字段与历史 serde 测试不构成旧 SaveSlot 迁移器。 | 历史保持旧字节的裁决不是当前继续支持旧格式的需求 |
| `task-20-review.md:71` 入口行数登记、`:72` 文档删除、`:75` 非阻断提示 | 当前 experience 已拆为 feedback/lifecycle/inputs/position_transition 等文件；legacy/datetime 半切风险是真实接线提示，现行散户均走 legacy，不由这个提示断言当前必然生命周期爆炸。 | 不新增行数配额或无复现的混写故障 |
| `task-20-review.md:78` 衰减消费义务 | 散户 heuristic 仍原始计数，机构有独立现行路径：`decision_chain/roots.rs:283` 观察本人持仓，`:297` 更新账户风险；`retail_projection.rs:597` record_institutional_fill_dated 消费真实成交和费用。 | 散户 G08 保留；不要求机构重复衰减/改回 Retail 心理模型 |
| `task-20-review.md:80` 质量探针、`:88` 结论 | 历史 TDD/确定性/行数及 verdict 是当时模块审查；当前正式失败/恢复在 `session/persistence.rs:752`、`:1247` 调 feedback.validate。 | 模块/恢复能力当前仍有；未执行现行测试，不宣称历史验收可自动沿用 |

## task-21-review：24 行逐章

| 原文条款 | 当前代码与消费者 | 状态 |
|---|---|---|
| `task-21-review.md:7` 首轮 sound 设计：accept≠fill、真实成交进度、反向门槛、低于已成交目标需理由 | `plans/revision.rs:98` 接受只写 child ID 和事件日；`:134` real fill 必须正数量、链接 child，checked_add 实际填量；`plans/validation.rs` 的 classify_revision 保留方向/目标理由校验。 | 已实现，没有 OrderAccepted 直接增加 filled_qty |
| `task-21-review.md:8` 最小保存/索引重建/不一致拒绝 | `plans/mod.rs:173` 反序列化到 PlanBook::from_parts，`:282` 重建每账户每股非终止索引且重复拒绝；`:275` 校验 plan ID 不超过 next seq。 | 已实现，非只有序列化 derive |
| `task-21-review.md:10` 旧 horizon guard 使 expire 不可达 | `plans/revision.rs:58` 接收 allow_beyond_horizon，`:300` expiry 传 true 但另要求 trading_day>last_valid，`:320` day-end 也传 true；终态与时钟回拨仍校验。 | 历史缺陷已修复，不能重新登记 |
| `task-21-review.md:12` 第二轮修复/TDD | `tests/plans.rs:1107` expiry success，`:1136` late day-end catch-up，`:1169` 不复活，`:1205` 回拨，`:1227` 其他越期事件拒绝。 | 当前对应源码仍存在，本轮未运行 |
| `task-21-review.md:17` day-end≠Completed/跨日存续、生命周期总性 | `plans/revision.rs:320` 到 horizon 才显式 Terminated(HorizonExpired)，普通日终只清 child；正式 `session/pipeline/auction_day_end.rs:2185` 遍历 active plan 应用 TradingDayEnded。 | 已接生产日界；没有将未完成目标冒充完成 |
| `task-21-review.md:20` TS 导出待 task29/纯行数观察、`:24` verdict | 历史未追踪导出文件与 302 行记录不能直接认作现行漏实现；类型工具现行生产契约由总账工具范围交叉复核，本批不据旧生成目录状态宣称当前导出验证通过。 | 非新产品契约，不恢复旧临时文件要求 |

## task-22-review：29 行逐章

| 原文条款 | 当前代码与消费者 | 状态 |
|---|---|---|
| `task-22-review.md:5` 首轮：重复 PlanId、非100股 lot、half-even 无直接用例 | `plans/allocation.rs:46` 在排序前拒 DuplicatePlanRequest；`plans/candidates/targets.rs:56` 必须 A_SHARE_BOARD_LOT；`tests/plan_allocation/gold/scoring.rs:38` 直接验证±0.5/±1.5；`failures.rs:151` 直接验证重复身份。 | 三个旧问题已修正 |
| `task-22-review.md:13` 修正清单、u32 overflow 保留 | `targets.rs:64` raw_qty try_from 返回 ArithmeticOverflow，不截断；`tests/plan_allocation/failures.rs:137` 公共转换溢出负控。 | 已实现 |
| `task-22-review.md:20` i128 中间溢出测试不可达 | 当前评分和目标转换的公开 Money/i32/u32 范围及固定 10000 倍率仍远低 i128 范围；`targets.rs:101` 可持有数量先限 u32，`candidates.rs:61` 只累加有界评分×权重。 | 不要求制造不可达输入/伪造异常 |
| `task-22-review.md:24` APPROVE：cash-reservation-fee、不预花卖出收入、T+1、唯一计划排序、缺信号重权 | `allocation.rs:44` validate_funds 后 cash-frozen，`:76` Buy 请求额加 fee checked_add，`:55` class/confidence/unique ID 排序；`candidates.rs:54` 缺维度不加入 used_weight，`:77` 用剩余权重 half-even 归一；生产 `decision_chain.rs:988` 调 child quote allocator，`:952` 来自 sellable snapshot。 | 主要模块与真实报价预算已有接线 |
| `task-22-review.md:27` 费用与风险游戏参数的后续范围 | 当前 Sell request `decision_chain.rs:975` fee_reserve=0，符合后来的 ADR-0017 零现金 escrow；`allocation.rs:25` child allocator 按 minimum commission 保障可成交买单。当前没有旧现金储备 floor，策略允许现金或全仓，依现行 ADR-0021/领域文档。 | 较新决定取代历史 cash-reserve ties，不复活旧 floor 或卖方 cash 预留 |
| `task-22-review.md:29` focused 28/28 | 历史模块 green 不能证明现行生产新机会类别已接入；`decision_chain.rs:972`、`:1236` 都仍 ExistingPlan。 | G38 保留，不能由 APPROVE 核销；本轮未测试 |

## 跨任务执行、失败与真实成交追踪

- 正式报价预算由 `session/decision_chain.rs:988` 交给计划执行游标；`session/plan_execution/interpreter/resume.rs:65` Replace 仅在匹配 Canceled outcome 后清 child 并续发，`:118` 重新校验 version、status、remaining，成交/修订已改变就 Waiting(PendingReconsideration)，不沿旧数量盲目新申报。
- 第一/第二撤失败由 `resume.rs:79`、`:95` 消费 typed failure；新单拒绝 `:108` remove_empty_linked_parent，不虚构成交或把撤单失败当成功。P3/P4 实际 fill 由 `pipeline/adaptive_plan_chain.rs:1595` 取正数量收据，`:1633` 调 record_parent_order_fills；`plans/revision.rs:134` 才累计真实数量。
- `plans/mod.rs:397` 为 pending 实际事实分批 stage；任何错误不会发布 staged plans，批内已终态的迟到事实消费但不重复转移；外层整个 tick 仍在 candidate 上，`pipeline/candidate_commit.rs:96` 最后一次 swap。
- 同 tick 释放不回补预算仍遵守 ADR-0017，不以“已有计划需要换单”扩展资金可见性。未成交/拒单/cancel 没有正数量 Fill，不能触发真实受挫 writer。

## 新候选反证与既有编号

| 候选 | 反证与当前边界 | 结论 |
|---|---|---|
| 任务20三问通过就证明 dated feedback 已接真实 session | 文档 `task-20-review.md:69`、`:77`、`:78` 明示后续义务；当前散户 init/observe/fill 三个生产点均 legacy，上述链已证实。 | 已有 G08，不新增 |
| 三派生 allocation experience 没生产调用，所以机构个人风险完全没有实现 | 机构 roots/真实成交已有 ADR-0026 的独立事实和风险消费；把 default 变成重复 generic 惩罚会二次扣个人信心。 | 反证成立，不另列 default 字段缺口 |
| 单个 ChildOrderAccepted helper 可覆盖 ID 就能证明当前有两张子单 | 生产 adaptive parent acceptance 在 `adaptive_plan_chain.rs:1552` 校验已有 child，真实路由按对应 typed identity 投影；历史 review 未承诺任意伪造 PlanEvent 流都会代表已路由订单。 | 未找到合法生产入口证据，不新增 |
| expiry 仍永久 Active，task21 原 REJECT 仍适用 | true 豁免只给 expiry/day-end，实际成功路径和 late catch-up 测试都保留。 | 已修复 |
| DuplicatePlanIds/lot/half-even 仍依调用方顺序 | duplicate 在排序前拒绝；100股明确校验；正负 tie 有公共 API 断言。 | 已修复 |
| 分配器排序按 ID 违反跨账户平权 | 这是单账户 strategy 的软预算排序，不是 P4 同股撮合优先；实际订单受理由局部资源链/股票入口另控制。 | 不新增；生产 class 漏分类仍仅 G38 |

全文与当前主链静态复核完成；没有运行历史列出的 cargo 命令或现行长验收。没有确认新增 G 不等于所有输入/负载都已穷尽运行验证。
