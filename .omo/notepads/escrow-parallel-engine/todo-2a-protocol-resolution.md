# Todo 2A 最终协议澄清

用户最终裁定已解除旧 canonical-order 阻塞：逐 tick TickFrame、帧内事实多重集、seq 仅为覆盖/去重/重连游标。不得把旧事件数组相对顺序作为业务因果，也不得跨 tick 重排或丢弃中间时序点。

历史 blocker 证据不删除。当前实测与失败过程见 `.omo/evidence/escrow-parallel-engine/task-2/protocol-resolution-and-verification.md`；完整复用校验必须通过 collector verify，不得直接复用早期 attempt。

最终当前产物为 `baseline-corpus/attempt-12/`，独立复核 PASS 与摘要见 `task-2/done-claim.md`。计划复选框不修改；Todo 2B 尚未开始。
