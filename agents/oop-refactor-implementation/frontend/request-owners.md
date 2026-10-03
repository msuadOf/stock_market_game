# frontend 请求 owner 实施记录

## frontend-R2-N01

- `IndicatorResultRequest` 持有挂载级 `IndicatorRequestGate` 的借用、本次 generation ticket、calculator/input 引用；`capture`、`matches`、`pendingRecord`、`resolveRecord`、`rejectRecord` 统一身份与结果转换。
- `useIndicatorResults` 的 effect 创建一次请求对象，同步写 pending，再保留 `Promise.resolve().then(() => calculator(input))` 的 microtask；React 继续唯一持有结果 record。cleanup 仍直接 invalidate 原挂载 gate。
- `PriceChart` 与 `MobileStockDetail` 仍通过同一个 hook 接入，不改 calculator、价格单位、MACD/KDJ 权威实现。parse 错误与 rejected promise 均返回显式 error state；旧 generation 不交出 record。
- 测试先新增 owner 行为断言，首次精确命令 exit 1（缺失 `IndicatorResultRequest`，不是 import 错误）；实现后通过。原 parser/gate/input 校验保持。
- 新增覆盖引用身份、旧响应/旧拒绝、长度错误、Error 与非 Error 文案。`use-indicator-results.test.ts` 用隔离 dispatcher fixture 驱动实际 hook 的 render/commit/cleanup，验证 microtask、A/B 逆序、输入/calculator 引用变化、disabled/unavailable 优先级、unmount 后旧结果忽略与 parse/reject 错误。未运行 DOM/浏览器 React 集成或 E2E；fixture 不等同真实浏览器挂载验收。

## frontend-R2-N04

- `CompanyRequestRegistry` 唯一拥有 `requestSequence` 与 `activeRequests`，提供 `begin`、`matches`、`finishIfCurrent`、`clear`。
- `CompanyQueryCoordinator.query`、`queryReportById`、`installBaseline`、`dispose` 与 `isCurrentRequest` 已迁全部生产登记 caller；generation/disposed/cache/DTO/Redux/event coverage 保留 coordinator。
- 同 key force 旧成功/失败/finally 特征测试先于提取，提取前与后均通过；额外验证 page/by-id 去重、clear 不重置 sequence、旧 finally 不删新 ticket、capability 缺失立即结束 ticket。
- 不改报告披露日期/revision/public ID、A 股交易语义、权威 engine 或 generated bindings。

## 验证

- Node `/home/linuxbrew/.linuxbrew/bin/node` 为 `v25.8.2`，满足现有 web runner `>=24.18.0`；先全文阅读 `scripts/run-with-deadline.mjs`、`scripts/run-web-tests.mjs` 与所属源文件。
- 命令：`node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=2 apps/web/src/components/indicator-results.test.ts apps/web/src/host/company-query-coordinator.test.ts`。提取前成功，新增请求目标后红灯，两个 owner 接入后成功；普通 command/case 均 10000ms，两个 file 真实并行。
- 额外请求测试详细输出：`node scripts/run-with-deadline.mjs 10000 -- node --test --test-isolation=none --test-timeout=10000 --test-concurrency=2 apps/web/src/components/indicator-results.test.ts`，4 case 全通过，约 0.24s。
- 增补边界后：`node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=3 apps/web/src/components/indicator-results.test.ts apps/web/src/components/use-indicator-results.test.ts apps/web/src/host/company-query-coordinator.test.ts`，3 文件通过，约 0.47s。
- 七个所属代码/测试文件的 `oxlint --threads=2` 受 10000ms supervisor 约束。首次 fixture 匿名 render 触发 `react-hooks/rules-of-hooks`；改为具名 `IndicatorFixture` 后通过，随后重复三个精确文件通过（约 0.47s）。断言和生产行为未改变。
- tsc 交由 root 统一执行。独立完整 frontend diff 复核尚待所有互斥簇完成后进行。
