# 本人交割历史 Provider 查询独立复核

## 范围与结论

独立复核新增 `apps/web/src/app/private-history-query.ts`、对应测试及 `MarketRuntimeProvider` 对 `queryPersonalTradeHistory` 的接线；不扩展至公开市场历史、本人交割单确认查询或交易语义实现。结论 **PASS**：helper 在发起 Host 调用前要求 host、generation、本人账户均有效；成功或失败响应都在返回/传播前复核同一 host、generation、account，身份/会话已变化时统一拒绝，未发现旧私有结果或错误泄漏路径。

## 复核要点

- account 作为 opaque decimal string 比较，不转为 JS `number`，测试覆盖超过 `Number.MAX_SAFE_INTEGER` 的 `9007199254740993`。
- 缺本人账户时 Host 调用次数保持 0；正常账户能查询。请求期间 host、generation 或 account 变化，对成功结果及失败结果一律抛出切换错误，且不透出旧私有错误详情。
- 上下文未变化时保留并抛出 Host 原始错误，不静默吞错。Provider 从 store 捕获真实 `selectPlayerAccountId` 与 snapshot generation，从 `hostRef.current` 读取当前 host；没有对 public null viewer 调用私有查询或伪造 account。
- 测试 `.tmp/retained-history/private-provider-red.log` 首轮 2 个行为 case 红、1 个错误传播 control 绿；`.tmp/retained-history/private-provider-green.log` 修复后 3/3 通过。已亲读 source 和日志；未运行额外测试。

## 限制

仅审查 Provider 私有日期历史异步结果身份隔离，不代表 `EngineHost` 具体历史查询、Remote 授权、公开 Viewer 流程或全部市场历史矩阵的整体验收。
