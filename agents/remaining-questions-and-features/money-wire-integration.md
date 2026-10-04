# Q01 Money 跨边界整合验证

用户决定金额以规范十进制整数分字符串传输，实施契约见 ADR-0031。
本批只完成 Q01，不宣称多人账号、数据库或其余待讨论 Q 项完成。

## 最终短验证

Cargo 使用 `-j32`，编译命令经进程外 `run-long-validation.mjs 300000` 监督；
最终 engine、server、web-wasm 的相关 lib/test 目标构建耗时约62秒。这里只构建和
运行代表性目标，不运行 workspace 全回归。实际 artifact 路径从 Cargo JSON 读取，
记录位于 `.tmp/q01-money/host-build.jsonl`，不按旧二进制文件名或时间戳猜测结果。

最终同一构建的独立短命令均用进程外 `run-with-deadline.mjs 10000`，普通 suite
使用32测试线程；表示摘要用互不冲突 case 并发，每个 case 自有十秒进程树 deadline。
单 worker 只固定每个受控 fixture 的受理顺序，多个独立 fixture 仍并行，不是单核回归。

- Money 29项、config 34项、collector 金额 map 1项通过。
- WASM registry 更新代表性1项、Server 私有 baseline 1项、REST 创建1项通过。
- WebSocket 非法数字拒绝与字符串受理1项：沙箱首次禁止 loopback 绑定；精确批准后
  同一最终二进制通过，约0.21秒，不能把沙箱失败冒充首次绿。
- Cargo 最终 `extraction_replay` 三个 case、`step_skeleton` 三个表示摘要 case 六路
  并发均通过，最长约5.20秒，日志为 `.tmp/q01-money/final-<case>.log`。
- `price_volume_baseline` 的当前金额 fixture 对应5项通过；没有改变原性能数据 JSON，
  其历史字节与指纹保留，历史计划已明确只能在对应旧源码上复测。
- Web TypeScript build 类型检查在外部十秒内通过；本批核心金额与代表性 UI/schema
  文件的 Oxlint 检查通过，`cargo fmt --all --check` 与 `git diff --check` 通过。

各作者和非作者的 Web 代表性短测、实际 ts-rs 导出、当前 fixture 367叶同值变换及
独立旧/新表示取证详见同主题记录。不同批次有重复 case，不把重复计数相加冒充覆盖。

## 历史失败与验证边界

因果诊断原有两项测试因缺少 impact/Filled 失败，已用 `675ac4c` 原完整源码、相同
features 和旧 rlib 独立复现；日志与 SHA 见 `money-wire-rust.md`。本批新增金额诊断
case 单独通过，Q01提交时没有弱化原断言或修复不相关 fixture，因此当时不能宣称
整个因果套件全绿。用户随后要求修复这两项失败，已通过真实双边成交 fixture
补足前置条件，保留原断言，14项因果短测通过；见
[后续修复](causal-test-fixture-fix.md)，不以此替代完整回归。

没有执行完整回归、真实发行、三平台重新打包或全部 Desktop 测试，也没有推送。
完整回归仍遵从用户要求，在剩余讨论与实施全部结束后执行。
