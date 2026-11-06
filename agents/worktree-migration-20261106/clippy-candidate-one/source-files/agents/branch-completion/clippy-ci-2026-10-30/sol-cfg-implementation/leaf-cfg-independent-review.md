# 精确 test cfg 叶独立复核

- reviewer 未参与对应叶实施；已阅读 runtime-dead 提交 195d4ef 全部源码 diff、pipeline 3 文件 diff 与 session-dead 4 文件 diff。
- runtime-dead 的 tax-only inspect、值快照覆盖探针、InformationClockContext::date 及三个 Admin 合成 sender 均由现存测试消费；生产继续使用 Arc 覆盖核心、day/cutoff、World 认证的 prepare_admin_ordered_scope。repurchase_funds_candidate 仅由模块内部测试消费，正式资金路径为 admin_repurchase_funds 与现有 Admin source producer，不移除该产线。
- pipeline 的 roots_source_drained 与 due_at 只由现存测试调用，cfg 精确；INLINE_PREPARE_CALLS 的 getter 全仓没有读取者，计数器仅 test 下自增，删除不会删实际验证或运行时副作用。
- session-dead 的六个函数只由单元/合同测试消费；Account 的生产 T+1 释放与 runtime 日终仍保留，SessionCandleBook 的正式 candidate 安装和 runtime desk K线未改。
- A 股语义仍沿用 ADR-0017/0043 与 docs/trading-rules.md；未修改成交/退款/冻结/费用或自然日时钟函数体、分股单位、受理顺序、JSON/API 契约。源码复核没有发现阻断；test cfg 不代表产品未支持的制度已接入。
- 最小范围通过：没有泛 allow、伪调用、强断言删弱；保留无法证明的正式状态字段并等待新诊断。
- 本复核仅源码；尚未运行 Cargo/Clippy/测试，实际feature/target编译矩阵仍须由统一窗口验证。
