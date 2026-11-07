# 日历业务对拍草稿归档

这些文件来自 `calendar-v11-business-2026-10-29` 工作区的 `agents/branch-completion/mixed-calendar-c2c/v11-business-source-33/` 未提交草稿，源工作区 HEAD 为 `b9c28021`。逐文件 SHA-256 在归档时与源文件核对一致；仅保存九个源码和测试文件，未保存运行日志和临时 JSON。日志含本机绝对路径，临时 JSON 不作为验收证据。

**未接入、未验收。** 断言测试使用人工构造的 synthetic trace，只验证 oracle 自身；它不证明真实 native 或 browser 运行通过，也不证明跨宿主业务对拍通过。`browser-business-run.mjs` 未调用 `assertBusinessTrace`，没有完成业务断言闭环；草稿尚未适配当前存档 `policy_digest` 契约。不得把这些文件或 oracle 的测试结果视为交易规则、日历政策或验收结论。

本目录不是 Cargo example，也未注册到构建或测试流程。原始相对路径、浏览器根路径、工作目录、输入封存和产物路径均保留原状；迁移后不能假设可直接运行。恢复开发须重新核对 fixture 引用、宿主接口、路径约束和输入绑定，补齐真实 native、browser 以及各执行模式的业务断言与对拍，并按现行日历政策和官方依据独立核验领域语义。草稿保留历史假设，不改变当前 A 股交易语义。

## 当前产品 host 验证入口

历史 serial 草稿仍未接入；`browser-host-probe.ts` 使用当前产品 Worker host 和真实 runtime。
`supervise-host-business.mjs` 使用进程外 `run-long-validation` 监督整个批次，最后一秒预留清理。
必须先准备与当前 Rust 源码匹配且具有 release provenance 的 WASM package，并将
`apps/web/wasm-pkg` 指向该 package；还需安装本仓库 Web 依赖、Chromium 和 Rust toolchain。
从仓库根目录传入 package 的绝对路径和一个尚不存在的证据目录：

```sh
node scripts/run-long-validation.mjs 300000 -- node agents/worktree-migration-20261106/calendar-v11-draft/supervise-host-business.mjs <wasm-package> <新证据目录>
```

批次重建 native example，以同一输入运行 native 与产品 browser，分别执行业务 oracle 后比较共同公开投影。
`browser-raw.json` 记录 server 实际发送的响应 Buffer SHA 和 browser 状态、完成信息；它不证明
browser 直接读取的 response body SHA。`manifest.json` 保存来源、输入和输出摘要，`supervisor.json`
记录内层监督命令与用时；最外层进程外 supervisor 另覆盖该记录写入和进程退出。2030 mixed calendar 是显式 synthetic 测试政策，不代表官方未来休市安排。
