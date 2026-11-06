# 日历业务对拍草稿归档

这些文件来自 `calendar-v11-business-2026-10-29` 工作区的 `agents/branch-completion/mixed-calendar-c2c/v11-business-source-33/` 未提交草稿，源工作区 HEAD 为 `b9c28021`。逐文件 SHA-256 在归档时与源文件核对一致；仅保存九个源码和测试文件，未保存运行日志和临时 JSON。日志含本机绝对路径，临时 JSON 不作为验收证据。

**未接入、未验收。** 断言测试使用人工构造的 synthetic trace，只验证 oracle 自身；它不证明真实 native 或 browser 运行通过，也不证明跨宿主业务对拍通过。`browser-business-run.mjs` 未调用 `assertBusinessTrace`，没有完成业务断言闭环；草稿尚未适配当前存档 `policy_digest` 契约。不得把这些文件或 oracle 的测试结果视为交易规则、日历政策或验收结论。

本目录不是 Cargo example，也未注册到构建或测试流程。原始相对路径、浏览器根路径、工作目录、输入封存和产物路径均保留原状；迁移后不能假设可直接运行。恢复开发须重新核对 fixture 引用、宿主接口、路径约束和输入绑定，补齐真实 native、browser 以及各执行模式的业务断言与对拍，并按现行日历政策和官方依据独立核验领域语义。草稿保留历史假设，不改变当前 A 股交易语义。
