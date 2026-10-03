# Hosts 实施协调记录

本批基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。范围为 assigned-actions 的 34 动作及必要 caller 迁移；无 Git 写操作。

- actors：4 动作；desktop/server actor 和桌面 actor fixture。
- server：6 动作；WASM registry、静态资源 root、CLI/API/WS fixture 及 WS publisher。
- fixtures1：6 动作；披露、周末、存档、曝光、竞价与行为 fixture。
- fixtures2：6 动作；civil、belief、更正、计划、披露失败和订单存档 fixture。
- build：6 动作；build/matrix/runtime/baseline/audit 生命周期。
- deadline：2 动作；bounded command 和 sealed inventory。
- performance：4 动作；process sampler、UI report resources、paired run 和 conservation。
- account_callers：Account/Position 封装引起的 action 文件之外 integration caller 迁移。

Rust 编译与简单短测由 root 统一运行。本组只运行明确的 Node 短测，case timeout 10000ms，进程树 deadline 10000ms；不运行完整回归、E2E 或性能矩阵。

## 协调者改动

- `apps/server/tests/actor.rs` 和 `protocol_updates.rs`：六处 broadcast 订阅从 `handles.event_tx.subscribe()` 迁为 `handles.subscribe_events()`；原断言和接收时序保持。未运行 Rust 测试。

## 复核

各 worker 完成后由未实施者全文 diff 独立审查三门；有效发现修复并复审后才记完成。

## 2026-10-03 收口

34 个动作已实施并分别经未实施者完整 diff 独立静态审查三门通过。统一结构化台账为 `status.json`，含 198 条 requirement_checks；源码绑定见 `source-review-manifest.json`。动作的实际 owner、方法、caller、文件、短测证据与运行限制逐条保留。明示允许省略的可选薄 wrapper 按 root 确认保持等价直接 caller，不虚称新增方法；所有可选状态 owner 均已实施。

复核有效发现已修复并再次复核：ArtifactInventory 深层 extra JSON 接受范围；PerformanceComparisonRun 未校验 append 与样本引用泄漏；runProcessSample async facade；WS flush 策略断言；civil_clock 变更注释中文及记录路径限定。build 的 negative-control witness 漏检判断经完整调用链和重算 receipt 的零 child 短测证明为误报，保留纠错历史，不新增重复 guard；E01 Writer 补真实短文件系统 fixture 测试源码。

额外 Account/Position 四文件 caller 完整独立静态审查通过。Market 的三个故意非法内部状态用例完整迁到 Domain 拥有的 unit test，原十个用例仍各一次；映射见 account-callers.md，Domain 的独立 reviewer 已确认搬移闭环通过，报告为 ../domain/orderbook-review.md；正式短测为 lib filter market::price_limit_state_tests。

本组没有运行 Cargo、完整回归、E2E 或真实性能矩阵。Root 正在统一 Rust 编译与代表性短测；桌面 CLI 精准五用例已由 root 权限重跑全通过；Writer example 编译成功，五个短用例按注册 workspace-local 环境重跑全通过，首次环境缺失失败保留。不同修订版本的 Node 通过记录没有合并冒称最终整套重跑。

all-targets 编译发现的 production_entry_performance 三处 TradingPlan 读取已迁 getter，未执行性能；未实施者完整文件及两行 diff 复核通过，报告 review-production-entry-caller.md，纳入 source-review-manifest.json 与 caller-migrations.json。

Root 最终验证已引用至 final-validation.json 与全部34条 status.json：build07、check08通过；232个lib短case全过；追加41项39过及两个WS bind沙箱失败原记录保留、沙箱外2/2通过核销。Writer5、桌面CLI5通过。本次只更新台账，没有源码/断言变更；default-features check09由root继续，未提前记通过。
