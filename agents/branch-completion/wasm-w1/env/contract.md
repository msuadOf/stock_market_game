# W1-1 env 实施合同

本目录仅记录 W1-1；实现位于独占 worktree `.worktree/wasm-w1-env`。

- 输入：`agents/branch-completion/wasm-w1-task.md` §4.1、§5 与独立设计复核。
- W0 生产输入必须为最终可信已审 commit；候选文件只读取证，不作生产基础。
- 仅改 `engine-clock-env.ts`、新增 `facade-thread-binding.ts` 与对应短测试。
- clock 原函数不改；不改 Worker、runtime、Rust、构建或 supervisor。
- 绑定 memory 先于 WASM 实例化；layout 由 `facade_slot_layout()` 返回后一次装入。
- 五 import 均拒绝未绑定调用；shared memory、地址和角色错误显式失败。
- wait 的 i64 实参必须为 BigInt：`-1n` 表无限，其他值只接受非负安全整数域。
  expected 保持完整 32 位位型，合法 raw i32 负值不会因高位而被拒绝。
- notify 唤醒全部；每次使用当前 memory.buffer，grow 后扩展范围可达。
- H5 在调用内复制并以 fatal UTF-8 校验 name，slot/gen 直接来自入参；不重读槽猜代。
- exit 仅恰一 bootstrap 身份匹配的记录；不 post、不 close、不 destroy。
- terminate 仅发控制消息，主线程真实句柄与幂等回收仍属 W1-2。
- 普通 Node case/整命令均 10000ms，显式文件并发；tiny WASM fixture 只验证 ABI，
  不将 Node 或 stub fixture 称为真实浏览器验收。

本批只涉及线程传输边界，不修改 A 股规则、资金/股份单位、T+1、同股受理序、
Abort 或日终存档。最终仍要求未实施者独立复核完整 diff。
