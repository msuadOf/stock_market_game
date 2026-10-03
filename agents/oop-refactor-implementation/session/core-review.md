# Session 核心改动独立静态复核

后续追加的 10 个 caller/既有测试文件完整 diff 复核见 [caller-review.md](caller-review.md)，其范围与内容指纹单独登记，不扩大下表 11 个核心文件的 SHA 绑定。

复核者：`/root/implement_session/review_core`，未参与实施。基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
复核日期：2026-10-03。范围为最初委托的 10 个文件，加上实施者随后明确追加的 `session/account_book.rs` getter/fixture 迁移；相关 caller 和底层 owner 仅用于核对调用含义，其他 worker 的在途 API 不纳入发现。

## 结论

截至下列内容指纹，未发现这批核心实现的确定行为错误或 A 股交易语义漂移。`GameSession.state` 是唯一可提交状态 owner，没有 `Deref`，`poison` 与测试注入 hooks 仍留在 facade。
发现的 N04 代表性测试覆盖缺口已由实施者补充测试，并经本复核者增量复核关闭。当前无阻断静态发现；静态实现结论不能替代测试通过，不能据本报告宣称测试或整批验收已经完成。

## 三门核对

### 大 A 语义与依据

- 交易阶段边界、T+1、数量单位（股）、货币单位（分）、费用冻结、连续竞价撤单所有权和 seller 实际费用历史的计算/校验表达式保持原样。
- `observation_civil_instant`、`causal_time`、`current_market_minute` 的时间映射仅改访问路径；没有把自然日与交易日合并，没有改变开盘、午休与收盘窗口。
- 个人四成员仅合并原信念机构账户；`npc_attention` 仍覆盖全体 NPC，散户/游资不会由默认值补出基本面认识。个人 `information.owner` 和 `belief.owner` 校验仍在 `validate_save_slot` 边界进行。
- 本批没有新增或修订交易制度。依据沿用 `docs/trading-rules.md` 登记的沪深交易所 2026 版规则（2026-07-06 起施行，登记的内容核对日期 2026-09-22）、零股卖出说明、财政部印花税公告及中国结算费用表。该文档已明确过户费来源的后续访问失败边界；本次只读静态复核没有重新联网核验官方资料，不扩大这些证据的时效承诺。
- 策略个人记忆依据 ADR-0026（2026-10-01）与存档边界 ADR-0025；本批不引入统一止损、强制卖出、补钱或新的公开信息前视。

### 必要性与最小范围

- A01 将既有 tick shadow/commit 字段迁到 `CommittableSessionState`，字段集合完整，保留 `AccountBook::clone_for_shadow` 校验及原错误 location；accounts、npc_attention、retail_experience 的旧页仍并行释放。
- N04 用 `AccountPagedMap<BeliefParticipantState>` 替代四份同键权威索引，没有保留第二份 mutable authority；继续复用原 COW 容器，单账户借用接口没有公开到宿主 API。
- `SaveSlot` 仍保存 information_states、belief_books、watchlists、price_memories 四字段；既有名称、必填要求、账户键、成员 DTO 均未改。
- `SessionCandleBook`、`NpcAttentionScheduler`、Account/Position/Market、ParentOrderPlan、CausalCollector 和 NpcOrderLifecycleBook 的 caller 适配属于同批 owner 迁移所必需。已抽查本文件使用的恢复/生命周期 helper，仍是原字段赋值、检查和 retain，没有引入额外交易业务。
- `clone_for_plan_roots` 删除由同批独立 observation owner 迁移承接；不将其作为本报告范围内的完整调用链验收。

### 边界、跨层语义与复杂度

- `business_state_hash` 四成员继续按 information → belief → watchlist → price_memory 顺序逐字段投影；构建有序 `BTreeMap` 引用投影，保持原 JSON map 形状和账户顺序，没有直接序列化聚合条目。
- scheduler hash 已使用 `.iter().copied()` 收集 `(tick, AccountId)` 后排序；对应原 `Reverse` heap 的 `.map(|entry| entry.0)`，保持 tuple 序列，未漏掉 AccountId。
- new 的个人空构造器没有 RNG 或错误分支；聚合构造顺序变化不改变独立 analysis/belief/policy RNG 与随后 attention RNG。
- restore 先完成 `validate_save_slot` 的四 map 同键/owner 验证，再从四字段构造聚合条目，索引不会将合法外部输入变成 panic。聚合条目中 watchlist/price_memory 较原路径提前覆盖；紧接的 `reconcile_institutional_holdings` 只用账户、market 与 belief，故成功结果/首错不变。
- `PlanPersonalState::take` 仍先移除 attention；缺失聚合条目保留原 watchlist 首错。合并条目消除了过去可由内部手工破坏产生的四 map 非同键状态，不增加外部容错或 fallback。
- `install` 的 duplicate attention 仍在新 attention 写入后 assert；attention 成功而 participant 重复时，只替换原 participant 的 watchlist 后 panic，其他三成员保留原值，符合旧五次 insert 的部分写入顺序。这里的特殊分支是维护已要求的首错行为所需，建议由下面测试固定，避免未来误认为可随意简化。
- tick commit 保留 authority facade hooks；shadow 重置 facade hooks。自然日日结失败的 checkpoint 整体恢复路径保持原样，没有新增 panic 捕获或错误吞没。

## 发现：N04 代表性覆盖缺口（已补测试并静态复核关闭）

新增 `committable_state_tests` 的两项测试使用 `retail_quote_setup()`，其 `inst_count = 0`，因此四个人状态投影为空。它们覆盖 facade hooks、pending_player 与部分 commit 事实，但不能证明非空聚合条目的 shadow COW 隔离和两个机构间的独立性。

原反馈要求在实施者允许的短测预算内补充：

1. 两个机构的四成员内容分别保存/恢复及 take/install 后保持本人状态；同批至少断言散户/游资有 attention 而没有 belief participant。
2. 修改 shadow 中一个机构的四成员，authority 与另一个机构四成员均保持原值；这也固定聚合后的 COW，避免未来误用共享可变态。
3. duplicate install 的 attention 首错，以及 attention 安装成功后只替换 watchlist 再 panic 的已保留行为。

实施者随后在 `personal_state.rs` 补充 `participant_owner_tests` 四 case 和 `duplicate_install_state_tests` 一 case，本复核者完整读取并确认：

- 两个机构分别保存 original 四成员；从 shadow 取出第一机构后修改四成员并复装，完整 JSON 断言 authority 两机构及 shadow 第二机构均保持原值。
- 四个独立 SaveSlot map 分别比较 restore 前后内容，并比较 `business_state_hash`；不是只比较聚合条目。
- 两项 `should_panic` 分别固定 attention 首错、participant 重复的 watchlist 首错。
- 最后一个 case 在 test harness 内 `catch_unwind`，使 attention、watchlist、price_memory、belief 与旧条目有差异；失败后断言 attention 新值、watchlist 新值与其余三成员完整旧值，固定原部分写入边界。生产路径未增加 panic 捕获。

散户/游资 attention-only 人口由现有构造条件及零机构 fixture 保持，本次未强制增加混合人口 case。旧基线 hash 字节兼容仍需要根验收的真实 comparison 或既有 golden；以上 restore hash 相等不冒充旧基线兼容证据。

已有相关守卫可继续复用：`save_contract/failures.rs` 的 missing_personal_state/malformed_price_memory 拒绝边界、`decision_chain.rs` 的 `plan_root_owns_its_personal_state_until_the_private_candidate_installs_it`、`hash_contract_tests.rs` 的 shadow 与 authority hash 相等。它们不能单独证明旧基线 hash 字节兼容；如根验收已有密封旧基线 comparison，需由实施者引用其真实证据，不在本报告编造 golden 或运行结果。

## 执行与内容指纹

最后一次增量复核确认 `session.rs` 仅删除已由 scheduler owner 承接的 unused `Reverse` import；`BeliefParticipantState` 的 information_mut/watchlist_mut/price_memory_mut 仅在 `#[cfg(test)]` 模块真实调用，增加 `#[cfg(test)]` 不删掉生产能力，生产 root 仍通过 `PlanPersonalState` 独占字段更新。`AccountBook` 追加 diff 只用 getter 读取原策略及 T+1 锁定数量，测试 fixture 保留 qty/invested/recovered，仅替换 t1_locked；get_mut 的页缓存失效和 COW 写边界均保持原样。未新增发现。

本报告所列源码 SHA-256 与 `session/source-manifest.json` 对应条目逐项相符；该完整 manifest 有 63 个源码条目，本报告只复核和绑定下列 11 个文件，未将 manifest 的其余条目宣称为已审。

本复核只使用源码、文档和只读 `git diff/show`；没有执行 Cargo、测试、Git 写入或格式化。唯一写入是本复核报告。
完整范围 diff 的 SHA-256：`217494cd1879f92e1622b2c9f782cebeac50cfdb6397710300c46b1d3502ba41`。

| 文件（前缀 packages/engine/src/） | SHA-256 |
|---|---|
| session.rs | c208c39dc832b8fa41f17331c6e0bb628a041007fcfef367f4f136bd51a4e7bb |
| session/account_book.rs | 1385a460975449e967acdb2c2864d1fd61390b565736abbbf6fb20a555984bd0 |
| session/decision_chain/personal_state.rs | f523212df76d13a7d253df349d318828d0f115afb59efc440dd1f6f61f3d5caa |
| session/hash.rs | d171f00dc61d0e847c59daf647c42d2fafb8dbc43995fe0b0e11c1c70658da13 |
| session/views.rs | f4df3b745f6aa229a8b84c930a5266725c39d0a148e8c88d02168473534a58e0 |
| session/self_views.rs | 9f73e4973fbf8e8b3fd8f4cd3e6a8b4f01e2da616a1f07a4901f4d2609eb2e7a |
| session/causal.rs | c2bf7eedfa4d3a97a7f56aa83e16cb0e0613005e5b9284ef07a33e0662520cfb |
| session/envelope_projection.rs | a619bde3a7181aa757938ee0cc3da7cfd9c1a579b8fbacf6f91f1c4b0f9a8144 |
| session/continuous_cancellation.rs | e516c7f0d0baefb4e043ba10e95cda06b778f0e66572c8d9c054328a4686693d |
| session/observation_clock.rs | 2e13f5fc3d3867d9d2decfcad24e8bbeb66b2a53a25902c7d462e4bfb3d7d835 |
| session/failure.rs | 6f1f4fa1c3ba370a10a2442c005c27288734b64f8c5c64480208b0aab2b45adf |
