---
name: wasm-thread-abstraction
description: 用户决策（2026-10-20）：废弃 wasm 串行回退，建 std::thread 兼容抽象层（第三方 crate 或自研），性能优先，一份纯净 std-thread 代码不改跑全平台
metadata:
  node_type: memory
  type: project
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-19T19:39:08.554Z
---

用户拍板（2026-10-20，两条原话要旨）：
- "在 wasm 里面对 std thread 创建兼容抽象层，不管是基于第三方 crate 还是自己做，遵循性能优先，代码能够不改就能在 wasm 跑"
- "这样就没有串行版本了，不用维护串行 bug，不需要考虑串行，std thread 代码只用一份纯净代码就能运行在多个平台"

**Why:** 串行回退（ADR-0043 的 wasm 简化条款）导致双路径维护成本：B2 数据面刚为串行补过一轮等价安装；单一代码库消灭整类 bug。

**How to apply:**
0. **Native 零损耗（用户追加硬约束 2026-10-20："不要搞其他中间层拖慢速度"）**：抽象只对 wasm 生效——编译期替换（cargo target-specific dependencies 或 cfg 条件 use），native 编译产物与现状逐指令等价；任何在 native 路径引入运行期间接层（trait/虚分派/包装）的方案直接否决
1. 抽象层目标：engine 的 std::thread/std::sync::mpsc 使用面（spawn/join/channel）在 wasm 映射到 Web Worker + SharedArrayBuffer/Atomics（或第三方 crate），**业务代码零改动**
2. 性能优先：真并行（SAB+Atomics，需 COOP/COEP——批 9 A10 删的门禁将回归）优于 postMessage 串行化
3. 翻案 ADR-0043 串行条款（修订 ADR），解锁批 9 C 桶（旧管线/ComputeContext 核心）最终删除——串行 B2 补面成待退役技术债
4. 设计先行走 Sol 门（架构级变更），关联：[[architecture-not-sacrificed]]、[[std-thread-migration]]
