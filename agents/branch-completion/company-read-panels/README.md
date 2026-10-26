# F4b 公司只读面板候选

本片从 `3984e14167202dc645ec340fbd2a73e661c71c8a` 开始，仅新增四个只读面板、共享 query lifecycle hook 及五个测试文件。不会挂载 App、提交认购或改动账户。CompanyId 使用当前数值身份；股数、Money 分、AccountingAmount 元保持精确字符串；税务语义沿用 ADR-0040，未新增交易制度。

## 红绿证据

- `red-node.stdout` / `.stderr`：真实 ReactDOM `createRoot` 与 `React.act` 捕获旧实现两项业务失败：移除 host 后旧 owner 税表残留；`latest_collection=null` 被误报为已结清。
- `red-source/`：上述红灯实际源码及 SHA-256。`red-rights.stdout` / `.stderr` 与 `red-rights-source/`：显示遗漏认购价、登记结算日及锁定期的后续有效红灯。
- `green-focused-final-2.stdout`：历史 Node v25.8.2 的五文件并发 5，实际 21 case 全绿；不把它改记为 Node 24。最终 Node 24.18 结果为 `final-tests-node24.stdout`，21 case 全绿，strict TypeScript 与 lint 同时通过；独立 jobs=3、测试文件并发=5，case 与每个进程树 deadline 都为 10000ms。
- `red-lifecycle*.stdout` / `.stderr` 与 `browser-attempt.test.ts.snapshot`：Chromium 启动/清理失败的诊断原件，不属于有效业务红灯，不宣称 browser E2E 已通过。

## 生命周期验证范围

`use-company-query.test.ts` 约 45 行内存 DOM 仅提供 ReactDOM 所需的 Node 操作。执行真实 React hooks、调度、effect、cleanup、卸载、文本渲染与 React handler；没有替换 dispatcher。用例覆盖逆序成功/失败、公司/owner/generation 切换、callback 更换/移除、卸载、稳定 provider 和含 NUL 的 tuple key。四面板 SSR 只证明零 query 与尚未查询/unsupported 文案。

此验证不覆盖浏览器原生 date input 行为、布局、样式、可访问性树或完整宿主/存档旅程。六 query 的非空资料在此作为 callback 层 fixture；不冒称来自 Rust 实际运行或三宿主 producer matrix。F4a 的严格 parser 类型闭包及其证据属于独立依赖，最终 receipt 记录实际依赖提交与源码 SHA。F4a 完整候选 tip 为 `835f0f873e6a25ffd40b04c4b688dc695a1c2965`，包含 `369a6ed4` 业务与补纳诊断原件；当前等待协调独审结论。

## 未交付边界

实际投资者结算仍按 `cash_settlement` 与 blocker 展示；财务数据可读不能外推结算能力。配股排队表示受理，缴款、弃配按真实 DTO 分列；当前 DTO 无退款事实字段，不能推断已退款。股息税历史 collection 只描述其 day/operation_seq 时点；没有 collection 时不推断资金不足、已尝试或结清。机构/企业差别计税仍未实现，Flat 机构同样代扣，Exempt 免卖出印花税但佣金/过户费照付。
