# ADR-0029：职责名称与严格当前契约

- 状态：accepted（用户授权完成命名重构；验证与本地提交另行记录）
- 日期：2026-10-04
- 决策者：用户；AI 记录
- 依据：[ADR-0019](0019-draft-market-scope-and-capacity.md)、[ADR-0025](0025-day-end-only-persistence.md)

## 背景

内部类型、模块和机器格式身份混用了实施阶段编号与实现版本后缀，正式规范仍把这些旧拼写
当作应保留的真实业务版本。用户要求全部按实际职责完成重构。草稿阶段已明确不保留旧格式
兼容路径；此次不能以改名为由建立 alias、双字段或静默迁移，也不能改写历史审计和源码见证。

## 决策

1. 当前存档不设 `schema_version` 或代际版本，不以命名重构引入新版本。`SaveSlot.runtime_state` 的类型为
   `SavedRuntimeState`；Rust 模块为 `saved_runtime`，Web 模块为 `runtime-state`，解析入口为
   `parseSaveRuntime`。当前来源 tag 使用 `QuoteExpiry` 等职责名称；字段和 tag 的旧拼写
   不作 alias。类型、函数与模块不加 `V3` 后缀。
2. 公共存档入口只接受完整当前结构，明确拒绝任何 `schema_version` 字段、旧 `runtime_v2` key、旧来源 tag 和不完整结构。
   不建立旧格式转换器、双字段恢复、默认补齐或另一套游戏运行路径。完整自然日日结、
   不可变保存候选和失败时保留旧候选的边界继续按 ADR-0025 执行。
3. `simulation_policy_id` 使用职责身份 `a-share-simulation`。算法、随机流、NPC 策略、
   资金/股份与交易时段均不因身份改名改变；policy 身份不冒充存档版本。
4. 当前验证、性能、checkpoint、manifest、schema、format、source 与 scenario 身份按职责
   命名；真实版本放在独立数值字段中。生产者、exact-key validator、CLI、缓存/复用身份、
   fixture 和消费者同步检查身份与版本。旧当前格式明确拒绝，不通过字符串截尾猜测版本。
5. `TickPhase`、来源 tag 与计时 key 分别表达自己的职责。rank 保留实际顺序，rank 2 计时
   使用 `decision_and_coordinator_work`；`QuoteExpiry` 来源不改成 `ExpiryShadow` phase。
   审计 `priority` 类别与 `priority_rank` 分离，不把建议级别当交易执行阶段。
6. 密封历史输入、旧 gzip fixture、before 快照、已签署审计记录、原 hash 与真实候选来源 ID
   保留原字节。历史 decoder 仅作为证据读取器，核对当时的标签和内容身份；它不接受当前游戏
   存档加载，也不把旧结果改写为新验收通过。活动生成脚本可以改职责变量及当前描述，但本轮
   不执行这些旧生成器覆盖审计产物。

## 保持的领域边界

本决定只改变名称和格式身份，不改变现行沪深 A 股规则及已登记游戏简化：Money 分、股份股、
T+1、申报数量、价格时间优先、实际受理顺序、集合竞价、费用、计划引用和失败原子性均保持。
报价到期释放在分配截点前可见；密封批释放不回补本批账户校验预算；统一结算与单点提交保持。
依据与适用日期继续使用 [trading-rules.md](../trading-rules.md) 已登记来源，本轮不冒称重新核验
交易所或中国结算法源，也不增加新的交易制度。

## 验证与后果

当前契约需覆盖职责字段/tag 的 round-trip、额外版本字段与旧标签拒绝、严格键集、policy 身份及失败前
候选/存储不变；内部纯改名不增加实现镜像测试。工具契约的真实数值版本、会计 `version`、报告 revision、
quarter、ECL 阶段与第三方 API 保留。完整 diff 须由未参与实施者独立复核，大 A 语义、必要性
和跨层契约一致性通过后才可报告完成；测试和 Clippy 结果以实际执行记录为准。

旧草稿存档与工具产物不能由当前公共入口继续使用，这是 ADR-0019/0025 已授权的边界。
历史审计仍能按原始材料被读取，不能由其保留推导出产品兼容承诺。
