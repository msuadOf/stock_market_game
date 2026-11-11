# ADR-0044：WASM panic 的早期死亡通知与诊断记录

- 状态：draft（实现和验收进行中）
- 日期：2026-11-07
- 关联：ADR-0027、ADR-0043

## 背景

Web 本地局使用共享 memory 的 WASM Workers。Worker 在 Rust panic 中死亡时，等待该 Worker 的执行方需要先获知死亡，诊断消息是尽力提供的附加信息。固定版本的标准库在普通 panic hook 之前可能读取锁或格式化动态 payload；仅在自定义 hook 内避免分配和加锁，不能保证这些操作之前发出通知。

## 决策

1. Web WASM 构建固定 `nightly-2026-09-05`、`rust-src` 和 `build-std`。受控的 std 副本在 `panic_with_hook` 进入 panic 计数与标准 hook 之前调用早期入口，随后 abort；该路径不执行标准 hook 或动态 `Display`。这是 WASM 专用构建约定，不改变 native panic 处理。
2. 早期入口先发布共享死亡字并唤醒等待者；已绑定 child 的 join 状态随后竞争式标记死亡并唤醒，最后才尽力发布该 Worker 的 tombstone。通知先于记录，消费者不得等待处于 `WRITING` 的记录，也不得假定通知到达时消息已发布。每条记录绑定 Worker 身份，地址不复用，直到整个实例废弃才回收。engine 的身份为 `0`，ingress 为 `u32::MAX`，child 使用 `1..u32::MAX-1`。实例化后分配 primary 记录，`registerDeath` 须获得主侧确认；ingress 验证预登记的地址和身份后绑定。child 的 TLS 在 trampoline 的 SLOT OK 和 Rust `SpawnJob` 双重检查之后绑定；JS 地址范围校验不能证明 Rust 对象的 owner。
3. 单条记录固定 256 字节，其中 payload 最多 240 字节，超出时标记截断。仅使用无需格式化即可取得的字符串；动态格式或 `panic_any` 无此字符串时明确报告“panic 消息不可用”。字节按上限截取，读取端须区分无效 UTF-8、截断、缺席、写入中和非法记录，不得虚构消息。`phase=0` 表示未知。
4. hard trap 不经过 Rust panic 入口；未绑定或记录未完成也可能没有 tombstone。宿主仍须显式报告死亡及已有上下文，不能把缺席解释为 panic payload，也不能等待已死亡 Worker 完成记录。相关错误不得当作成功的 tick 或存档发布；此诊断机制不改变 A 股撮合、资金、股份、单位或存档语义。
   宿主无法可靠获知故障瞬间的权威 `last_committed_tick`，错误 context 明确标为 `unknown`（未知），UI 反馈保留该原始值。此前已观察到的 tick 只能作为下界，不能冒充故障时的权威值。日终存档是独立的恢复点，也不能用于推断故障瞬间的提交位置。
5. 构建脚本校验固定 rustc commit、完整 `rust-src` 和 panic 源文件的哈希，校验补丁锚点后在临时 sysroot 副本应用补丁；版本、源码或锚点不匹配即失败。原始 std 不含受控构建的 marker，强制链接及检查 marker，避免误用原始 std。构建结束清理副本，不修改安装的工具链。固定输入和校验使修改可重建，但不承诺跨环境产物字节一致。已有 `RUSTC_WRAPPER` 或 `RUSTC_WORKSPACE_WRAPPER` 时显式拒绝，避免绕过受控链。
6. 受控 sysroot 构建入口编译并使用原生 rustc wrapper，Windows 选择 `.exe`；该路径尚未在 Windows 实机验证，不能把已有的跨平台分发目标视为此 WASM 构建链已在 Windows 可用。其他平台也须分别验证。缺少固定工具链、源码或受控构建能力时构建失败，不静默改用未修改的 std。

## 后果与验证边界

- 早期通知不保证诊断记录完整或可见，也不覆盖绕过 Rust panic 入口的 hard trap。child 绑定前发生 panic 时没有该 child 的 tombstone，只能依赖 global DEAD 或宿主故障回退；实例化与 primary 登记前同样存在记录缺席边界。engine、ingress 和 child 的实际覆盖范围须验证，不能由局部结果推断。
- 受控 std 的 release WASM 浏览器 fixture 验证了 child、engine、ingress 的 panic 记录、死亡唤醒与 hard trap 缺席，以及真实 Worker error 经 owner 的故障清理。该 fixture 不覆盖 `createWorkerHost` 到 UI 的完整旅程，也不代表全仓验收。最终实现和平台可用性仍须按实际检查与独立 diff 复核报告。
- 修改标准库会增加工具链升级维护成本。升级时须重新审查入口、源哈希、构建和真实浏览器行为，不能仅更新版本号。

## 备选方案

只安装标准 panic hook 的方案不能保证在 std 内部锁与动态格式化之前发出死亡通知，因此不足以满足本决策。仅依赖宿主观察 trap 则无法在 Rust panic 早期提供同样的通知和可用的静态消息；hard trap 仍由宿主处理。
