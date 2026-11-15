# LibOS 平台边界与验证口径

## 边界

- `packages/engine`：单一生产 World、撮合、账务、协议与会话。不得选择 Native / WASM 实现，也不得按目标选择 Cargo 依赖。测试运行器的目标条件和显式 `verification-harness` 故障注入不属于产品。
- `packages/libos`：分配器选型、线程、等待、时钟、故障、后台析构及 Browser Worker/Atomics/槽与 TLS 生命周期底座。Rust 和 Browser 子层均不依赖 engine 或产品宿主。
- 宿主保留本来就不同的外部边界，例如 Axum 的 HTTP/WS、Browser 的 postMessage 接线和 IndexedDB。业务数据校验、市场设置、可见错误及按产品配置内存预算不塞进基础库。Rust/WASM ABI 输出序列化与 TS 外层装配不是第二套撮合引擎。

## 性能判定

- Native 与 Browser 各自用同一底座做直连/LibOS 配对。结果窗、实际退役、提交返回和 FIFO 真正析构完成分别报告。不得比较不同机器、不同宿主的绝对值以推断门面开销。
- 使用 31 对 AB/BA 样本和计时分辨率修正后的区间。当前用户未给绝对延迟 SLO，暂以 **额外开销不超过直连中位数的 5%** 作为本轮工程验收线；区间跨线为未定，不报通过。采样环境必须没有并行编译；真实产品的 tick / RSS/线性内存另报观察值，不把这些值换算成封装税。
- 普通测试按 case 10 秒；真实浏览器矩阵、构建、性能采样分为独立的 ≤300 秒长验收。记录制品、准确运行范围以及未通过项目；不把绿色 mock 或旧制品代作当前产品。
