# LibOS 等待、关停与时钟合同

| 接口 | Native / Browser WASM 公共语义 | 允许不同的底层行为 |
| --- | --- | --- |
| `recv_deadline` | 协调者等待消息、断连或相位 deadline。到达时限后再观察一次队列；只有仍空且连接有效才返回 deadline，迟于该判定点发布的消息留给下一次调用。并发入队不按发送方墙钟排序。 | Native 保留 std 事件驱动的限时等待；WASM 保留既有可观察宿主取消的有界等待片。std mpsc 私有 futex 未暴露给调用方，转换为同样即时的 Browser 数据唤醒需要显式通知协议，不能由 Receiver 私自猜测内部内存布局。 |
| `worker_recv{,_spin}` | 无空闲 deadline。正常断连前按 FIFO 交付缓冲。永久停止在入口已可见则不取件，在取件后观察到则不交付该值。 | 自旋、安静预算、调度及 wake 受底座影响。宿主死亡是 WASM 特有物理事件，上层必须显式报故障，不能冒充普通消息。 |
| `ShutdownWake` | 硬停止先置标记、再向每个仍活跃 worker 发送唤醒哨兵，然后释放发送端并由 owner join。不能假设单写 AtomicBool 或丢弃协调者 Sender 能解除互持 Sender 的裸 recv。 | 消息路径不增加每消息分配，不引入定期空闲计时器。独立发送端只在 World 生命周期登记一次。 |
| `Instant` | 在平台支持的范围内，正序差值正常、逆序差值饱和到零，`checked_duration_since` 对逆序返回 None；可以构建实例原点前的相对坐标。 | 本机原点、分辨率和极端可表示范围由底座决定。WASM 以共享原点表示有效宿主样本，等待中回退至调用起点以上或以下时均报 `ClockRegressed`；非法宿主样本本身仍走时钟的显式致命错误。 |

正常退出仍是引擎的 `Drain → Shutdown → Final`（行政清理按原协议），不得用硬停止旁路。可重试 tick Abort 与硬停止互不共用标记。不涉及交易规则、撮合、资金或沪深市场差异。

同源合同断言在 Native LibOS 单测与真实隔离 Browser Worker 的验证制品入口执行。Browser 另检查裸接收选路后以真实消息唤醒、硬停止过滤及 Worker/槽清理；选路标记不声称证明 OS 或 WASM futex 已 park。性能门禁必须另行比较每个平台同语义的底座直通与门面，不用跨平台绝对值或前后整局结果冒充封装开销。
