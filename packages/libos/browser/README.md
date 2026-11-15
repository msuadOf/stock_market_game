# LibOS 浏览器执行后端

这里保存 Rust `libos` WASM 后端所需的 TypeScript 执行机制。产品选择 Worker 入口、加载哪个 WASM 模块、分配多少 Memory，LibOS 管理共享槽位、启动/关闭、等待、时钟与故障观测。

| 接口 | 职责 |
| --- | --- |
| `engine-clock-env.ts` | 每 Worker 唯一的 `env` import 绑定，提供时钟、futex、spawn 与 exit。 |
| `facade-wasm-module.ts` | 由调用者提供内存页数与隔离状态，创建共享 Memory；验证用同一份 Memory 实例化与提供清理导出。 |
| `engine-supervisor.ts` | 由调用者提供 Worker 工厂与结构化错误解析，管理同一实例的 Worker、槽代际、退出回执和排空。 |
| `facade-child-bootstrap.ts` | 装载同一 Module/Memory，验证槽位，调用 trampoline，清理 TLS/stack 后发布退出。 |
| `wasm-wait-abort.ts`、`wasm-wait-death.ts`、`wasm-tombstone.ts` | 验证与读取共享内存协议，不解释业务消息。 |

Web 宿主的旧模块路径保留为重导出或很薄的装配适配器；尤其不能为裸 `env` import 构造第二份绑定。宿主的前端状态、业务协议错误、存档、内存配额及 WASM 文件路径不进入 LibOS。Native 不使用本目录。

`scripts/libos-browser-boundary.test.mjs` 隔离校验导入闭包、同一 `env` 绑定、共享 Memory 配置与完整回归源码指纹。槽状态编码、Worker 消息及回收仍由原单测与真实浏览器 fixture 验证。
