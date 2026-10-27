# 真实 Rust 公司查询 JSON 消费证据（2026-10-28）

本批只增加真实 fixture 消费测试与证据，不修改生产实现。fixture 来自 `company-query-host-2026-10-28` 的 `four-host-green-02` 实际测试 stdout；生产源码冻结为 `01d0ac7eb28ef11a0eb92a763e446bfc1cc6def4`，随后 Rust 交付提交为 `aa016fbd435d39ab37b3d9c7d6cce57d3b3d2131`。原始路径、字节数和 SHA-256 见 `fixture-binding.json`。`stdout-extraction-crosscheck.json` 将每行 JSON 与真实 stdout 逐项对照，四份 fixture 分别为 6/6/6/1 行。代码测试与工作证据各保留一份完全相同的 JSON。

## 来源与含义

- `wire-engine.json` 的六种结果来自独立 Rust domain fixture，其中包含非空权益、回执与税账记录；税账当前 `outstanding` 为零，不能描述成非零欠税，也不能将这些独立输入拼成一个已完成实际结算的生产 session。
- `wire-server.json` 与 `wire-tauri.json` 各来自同一个 `FlatWithholding` session 的六项只读查询。三个列表真实为空，能力面和期间解释非空；不是用空值替代错误。实际目录是 `CompanyId=0`、`C-600101`、`StockId=0`、`600101`。
- `wire-tauri-failures.json` 保存期间尚未结算的真实 `BusinessCondition` 拒绝，测试要求保留分类与具体原因。
- `rust-actual-wait-cleanup.json`、`rust-batch-receipt.json`、`rust-artifact-binding.json` 是 Rust owner 提供的原始副本；实际批次退出 0、18/18 case、1001 源文件未变，无残留进程与 scratch。该批是 native 验证，不能据此声称 WASM JavaScript export 或 browser 已验。

## TypeScript 与 App 验证

`company-read-hosts.test.ts` 将三种成功 fixture 逐项通过实际 TypeScript Worker/Remote/Tauri host 与严格 decoder（共 18 次读取），另有真实 Tauri 拒绝消费。Worker 使用 native registry fixture 加受控 Worker transport，不是实际 WASM export。

`use-company-query.test.ts` 新增一项真实 React/Redux/MarketRuntimeProvider/ConnectedCompanyPanel 旅程，经实际 `createRemoteHost` 消费同局 server 六种 JSON；仅 fetch、WebSocket 和 baseline 受控。断言本人认证请求不带 account、实际 `CompanyId=0`、股数、元单位可分配利润、简税、真实空税账与期间解释，且不显示写按钮或结清声明。这不是 browser 或实际网络端到端验收。

`ts-first-consumption/` 四项首次消费均通过。App 前三次失败保留在 `app-first-consumption/`、`app-second-consumption/`、`app-third-consumption/`：依次为受控清理路由遗漏、启动路由遗漏、选择器超出内存 DOM 的支持范围；改用已有个股视图输入后 `app-fourth-consumption/` 通过，未改生产代码、未削弱业务断言。stdout 提取初次脚本未考虑 Rust test harness 前缀；修正提取器后逐行相等，未修改 fixture。

`final-checks/` 首轮四个 strict app root 分片、host 19 项、UI 17 项全通过；新增测试有两条 `no-children-prop` lint warning。依照本文件已有 createElement props 写法修复后，最终结果见 `final-checks-v2/receipt.json`。所有普通命令采用 main 的绝对 deadline helper（10000ms），Node case timeout=10000ms；独立任务最多 7 进程并行。完整 App 原有三条 hook warning 仍按上轮记录保留，不声称整树 lint 通过。

完整 diff 必须经未实施者独立复核后才可提交完成。F6、公司命令与税务实际结算能力不在本批范围内，所有 `ready=false` 保持不变。
