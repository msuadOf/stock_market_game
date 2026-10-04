# Q01 Web 测试与当前 fixture 校准

## 契约与范围

用户选择 Money 使用规范十进制整数分字符串传输；本批只校准现有 Web 测试与当前 fixture，不提供旧 number 兼容，不增加 schema 版本。PositionSnap 的 invested_cents、recovered_cents 虽为 Rust 裸 i64，也属于同单位金额，按统一边界保存字符串。股数、时间、事件序号、比例和 AccountingAmount 两位小数元字符串保持原语义。

current-schema-save.json 依据 strict parser 的具体 Money 拒绝路径以及 Snapshot 生成类型校准。完整结构对比确认仅有 367 个安全整数叶子变为相同数值的十进制字符串，无对象键、数组长度或其他数值变化。该文件是当前 schema 投影 fixture，不是 sealed 历史见证；本批不改 archived 原始字节。

## 修改文件

- app：local-amount-render、market-chart-projection、market-chart-runtime、portfolio-selector、position-valuation、trading-commands 测试。
- components：AutoOrders、auto-order-protocol、market-grid-row-synchronizer、market-grid-rows、player-orders 测试。
- config：defaults 测试。
- mobile：game-clock-sync、kline-sync、market-model、mobile-component-render、mobile-intraday-projection 测试。
- save：complete-save-schema、day-end-archive、personal-schema、save-schema-contract、schema/personal/beliefs 测试；current-save-fixture.ts、fixtures/current-schema-save.json。
- host（协作子任务）：event-buffer、market-depth-sync、player-working-orders、protocol-coordinator、protocol-effects、protocol-parse、protocol-runtime-store、protocol/runtime-delta、remote-host、runtime-snapshot-policy、serde-normalize、wasm-save-protocol、worker-host 测试以及 protocol-test-fixtures.ts。
- store（协作子任务）：portfolio-selector、snapshot-local-refresh、snapshot-runtime-delta 测试。

新增 save 边界断言保留大于 JavaScript 安全整数和 i64 上界的精确字符串，拒绝 number、非规范编码与 i64 溢出；现金负值仍拒绝。原正负半分偶数舍入、T+1、数量、盘口 rank、权威成交统计等断言未弱化。图表坐标仍为近似 number，kline-sync 和组件 fixture 新增 rawPrices，保证摘要使用原始分值。

## 实际验证

所有 Node 定向批次均使用 runBoundedCommand 外部 10000ms deadline、case timeout=10000、test-concurrency=4；不运行完整 Web 或完整回归。

- 主任务 22 个相关测试文件：162 项通过，最终批次约 1.4 秒。
- host/store 子任务四组：106 项通过；后续 serde-normalize 调整单独 9 项通过，portfolio-selector 参数校准单独 1 项通过。
- Web TypeScript 完整类型检查在外部 timeout 10s 下通过，约 5.9 秒。
- 当前 JSON 结构逐叶对比：367 个纯整数到同值字符串变化，其余内容保持一致。

迁移过程中先观察到旧数值 fixture 被新 Money parser 拒绝，及新字符串输出与旧 number 期望不符；修正 fixture 后复跑通过。新增 save 边界测试属于实现接线后的覆盖补充，没有独立的生产实现前行为红证据，不宣称该项符合先红后绿顺序。价格错误原断言要求“价格”上下文，发现漂移后通知 parser 作者修复生产文案，没有改弱断言。

本记录不是独立复核结论；最终整批 diff 由非作者独立复核，提交由根任务统一处理。
