# 事实消费与 P0 owner 实施记录

日期：2026-10-03；对应 pipeline-R2-N01/N09/N10。实现和 caller 已落盘，编译与短测试由 root 统一执行；独立审查在途。

PlanChainFactConsumption 收拢四组私有集合，prepare_continuous_round/prepare_auction_round 在每笔 fact 内保留 identity 检查后立即 payload 检查，receipt 校验仍晚于全部事实；返回可丢弃轮次增量。Coordinator 在 market、parent、计划投影和同步全部成功的原尾部 commit_round；contains_operation/contains_receipt 只读查询供 finalizer，测试计数入口不暴露映射。

NpcOrderLifecycleBook 仅借用 GameSession 原序列化 Vec；ensure_order_absent 全局 OrderId panic 仍先于报价期限计算，具体 Session register/remove 由 session manager 在 session.rs 接入。due_at 提供原未排序事实，P0 原调用点按 stock/order 排列。DayEnd 使用 clear，receipt 调用继续通过唯一 GameSession remove。无新增索引、serde 字段或错误来源。

ExpiryOutput::record_release 先 checked_add 账户资源，再 append 明细；失败会保留原 entry 零值插入等局部边界。P0 释放汇总不能回加密封 P1。公开字段不收紧。

同时迁 adaptive/quote/resources 的 GameSession state、Account/Position 和 Parent getters；六个既有测试文件的 caller 由 stream worker 全文迁移，保留断言与负向 fixture。

新增精准短 filters：consumption_tests（2），quote_expiry::ownership_tests（3）。既有 filters：adaptive_plan_chain_tests、npc_lifecycle_projection_tests、initial_candidate_round_tests、quote_expiry_tests、quote_expiry_checkpoint_tests、decision_resources_tests。默认与 simulation-diagnostics 均需独立编译，以覆盖 feature 分支。单条普通测试10000ms，进程树 deadline 和多核参数由 root 记录。

新增边界锁定 discardable prepare、重复身份与 malformed payload 原先后、三个字段 remove/global duplicate/due 迭代，以及 shares overflow 时不 append、原 cash/shares 总量不改。新增测试尚未运行，不伪造红绿证据；精确 rustfmt 与 pipeline diff --check 已通过。

现行依据：ADR-0017 顶部 2026-09-24/25 修订与 ADR-0018 §7。本批不改制度，同股价格时间、实际账户争用、P0/P1/P9 边界保持；没有重新认证官方规则的声明。
