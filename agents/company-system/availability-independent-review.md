# PublicReportAvailability 独立复核

## 范围

复核 `PublicReportAvailability` 的 engine query、Server actor/routes、Desktop actor/lib、WASM 导出、Web Worker/host、Tauri host、Remote host、严格 normalize 与 `CompanyPanel` 接线。按当前工作区的 staged 与 unstaged diff 合并语义检查；工作区还包含大量其他主题改动，本记录不代表那些改动已审查。

## 结论

在本次指定的 availability 接线范围内，未发现可确认的未公开数值泄漏、请求期间／类型／scope 响应错配、路径公司与 body 公司错配，或改变 A 股交易语义的有效 finding。

- engine 先按 `as_of` 过滤已发布报告；availability 的 `Available` 只投影 `PublicReportSummary`，未发布时只返回枚举原因。查询校验 issuer、自然月末、kind/period 配对与 scope/company 一致性；详情只在已发布报告匹配请求 scope、期间和 kind 时返回。
- Server route 在 session actor 查询前校验 URL `company_id` 与 JSON 请求公司一致；JSON DTO 拒绝未知字段。Server 与 Desktop actor 都将查询交给 engine；Desktop 带 generation 并在 host 侧校验响应 generation。
- WASM worker 在执行查询前校验 generation。Worker、Tauri、Remote host 规范化请求，并在响应端将 `Available.report` 与原请求的 company、period、kind、scope 比对；异步查询同时检查市场／报告查询上下文是否仍有效。
- `CompanyPanel` 只构造当前公司对应的单体或合并 scope；期间、类型、scope、公司变化会清除旧结果或使在途请求序号失效。完整财报只在收到 `Available` 后渲染，不可用时呈现枚举原因，查询异常显式显示。
- A 股月报在项目文档中明确为用户可选的额外公开报告，不冒充法定月报义务；本接线沿用既定 `ReportFrequency` 和披露时钟，未更改交易规则。

## 证据与边界

已通读 availability 相关 DTO、engine query、Web normalize 与 `CompanyPanel` 完整源码，并检查三宿主对应处理分支、Server route 绑定及其路径/body mismatch 测试。实现者报告短测 27/27；另据 root 提供的 fresh 验证记录，host64 五个 Rust 包 `--no-run` 成功、`pureInfo` 10/10、Server mismatch route 1/1、typegen 128/128。此证据不等同于 runtime 三平台完整验证。本审查未独立运行测试，也未覆盖本次 diff 中无关主题。既有共享 `secondOfDay` validator 接受 `86_400` 的边界与本次新增 availability parser 无关，未作为本轮接线 finding 报告。
