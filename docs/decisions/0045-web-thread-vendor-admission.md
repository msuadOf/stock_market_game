# ADR-0045：隔离引入 web-thread 上游快照

- 状态：draft（LibOS 适配进行中；完整生产接入未验收）
- 日期：2026-11-11
- 关联：ADR-0043、ADR-0044

## 背景

用户决定先引入现有 web-thread 源码，再针对发现的问题作必要修改，以便评估作为浏览器线程层的可行性。现有 Native 使用 `std::thread`，Web 使用共享 memory、受控 std、专用 Worker bootstrap 和 panic/death 通知；这些契约不能仅凭 API 名称一致而视为兼容。

## 决策

1. 在 `vendor/web-thread/upstream/` 保存 `daxpedda/wasm-worker` commit `ce376d95dbdd9e7b59ac7de9c6f14090076f7865` 的完整源码快照；Cargo 包名 `web-thread`、版本 `0.0.0`，上游许可为 MIT OR Apache-2.0，保留两份许可及全部上游版权和文件。快照保持原样，本地适配应在快照外单独记录与管理。
2. 原始快照不列入 workspace、不直接作为生产依赖；按用户确认的统一 LibOS 目标，在快照外追溯移植并适配上游线程与通知实现。当前 `libos::mailbox::Signal` 的 token 状态机改编自上游 Parker；上游完整线程创建与 Worker bootstrap 尚未接入，不能据此称为完成。Native 的上游路径重导出 `std::thread`；WASM 适配须验证共享 atomics、异步 Worker 启动和等待机制，编译成功不能证明运行时兼容。
3. 生产接入前须证明与 ADR-0043 活性和 ADR-0044 受控 std、早期 death 通知、child 身份、TLS、tombstone、join 和释放生命周期兼容，并在真实浏览器验证主线程不因阻塞等待造成 UI 背压。任何必要补丁均与上游快照区分并可追溯；缺失证据时保持当前运行时。
4. LibOS 适配不改变 A 股撮合、资金、股份、存档或单位语义；所有权转移和失败处理遵守 ADR-0043。

## 后果

保留原始快照便于审查、对比与回退，但增加源码和许可证维护成本。快照独立于生产依赖，现阶段不承诺在现有产品中可用；编译与浏览器验证结果按实际证据记录。

本轮 Native 上游库源码直接使用固定 nightly 的 rustc 编译通过；原始 Cargo manifest 的 Native 和 WASM 检查在依赖准备阶段超时，未进入编译。证据见 [准入验证记录](../../agents/web-architecture-diagnosis/web-thread-import/validation.md)。

后续隔离 lib-only fixture 的 Native Cargo 检查和 WASM atomics、共享 memory 的 cdylib 链接已通过；上游原 manifest 的完整依赖验证、浏览器 Worker 和受控 tombstone std 验证仍未完成，不能据此宣称生产兼容。详见上述准入验证记录。
