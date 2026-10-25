# W1-1 待实现 API

绑定工厂 `createFacadeThreadBinding({ isWorker? })` 为短 fixture 提供隔离状态；默认
`isWorker` 检查真实 `WorkerGlobalScope`。只供测试注入环境判断，不拦截或包装 std。
工厂返回 `bindFacadeThreadEnvironment`、`installFacadeSlotLayout`、`getArenaView`、
`requireExitIntent` 与五个 import 同名方法。env 模块导出一个单例的对应函数。

绑定参数 `{ memory, role, postControl, bootstrap? }`。role 为 engine/child/ingress；
bootstrap 仅 child 必须提供，含 `{ workerId, slot, gen }` 不可变值快照。
三种真实 Worker 均允许 futex wait/notify；engine/child 允许 H5 spawn/terminate；
仅 child 可记录 exit。main wait 明确拒绝。

layout 是实际 Rust 导出 `{ baseAddr, headerWords, slotStride, joinStride, capacity,
genBits }`；导出后只装入一次，交叉验证 TS 镜像、共享内存 header 和当前边界。
`getArenaView()` 每次以当前 buffer 重新创建 W0 `FacadeArenaView`。

H5 控制消息 `{ type: "facadeSpawn", slot, gen, name }`，terminate 控制消息
`{ type: "facadeTerminateChild", workerId }`；不在这里创建新 supervisor 或回收协议。

`requireExitIntent()` 返回 readonly、冻结的 `{ workerId, slot, gen }`，缺失显式错误；
重复 import 调用显式错误。外层拿到返回码后校验并 destroy 属 W1-2。
