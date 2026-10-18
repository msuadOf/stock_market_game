# due lifecycle 索引化排查（假设否决，无源码改动）

## 背景与待验证假设

read-only start 并行准备（382c1871）在 22-span 诊断中命中 2098/3002。交接摘要推断约 904 个未命中 tick 因“到期报价或空 ledger”回退旧串行路径，候选实验为按到期时间索引化 `npc_order_lifecycles`，消除 `iter().any(expires <= current)` 的 O(n×到期数) 线性扫描并提高命中率。

## 方法：零侵入 example 诊断（已回滚）

- 临时在 `GameSession` 增加 `#[doc(hidden)] pub fn lifecycle_diag_snapshot` 访问器（返回当前市场分钟、lifecycles 长度、到期数、ledger 非空），并新增一次性 `packages/engine/examples/lifecycle_diag.rs`：engine 层、release、Rayon16、默认 setup（5 股 / 20,007 NPC / seed 42）、3000 tick，逐 tick 分类 eligibility 并统计规模。诊断结束即按 HEAD 原字节回滚访问器、删除 example；生产源码无 diff，本文件即全部产物。
- 首选的 `cargo test --release --lib` 方案被弃用并记录在案：test profile 默认开启 debug-assertions，引擎热路径断言使单 tick 放慢 >100 倍，3000 tick 两次在 299s 超时（exit 124，只打印到 tick=0）。后续性能类诊断一律走 release example，不用 lib test binary。
- 诊断脚本与输出：`/tmp/lifecycle-diag-example.log`（engine 确定性 seed 42，可与 server 侧对照）。

## 结果

| 指标 | 数值 |
|---|---|
| 帧构成 | PreOpen 300 + CallAuction 600 + Continuous 2100 = 3000 |
| Continuous 命中 | 2096/2100（99.81%） |
| miss_due_quote | 4 tick（每 tick 约 26 个到期，合计 104） |
| miss_empty_ledger | 0 |
| lifecycles 峰值 / 末值 | 3428 / 3266 |
| 净 placements / removals | 3701 / 435 |
| Continuous 期 Σlen（扫描量） | 4,606,827 |

- “904 未命中” ≈ 900 个 PreOpen/CallAuction 阶段帧（eligibility 要求 Continuous，设计上不可命中）+ 4 个真实到期 miss；与 server 侧 22-span 的 2098/3002 相符（批量边界差 +2 帧）。
- NPC 报价 TTL 为 8–36 市场分钟（≈474–2133 tick），到期高度成批；3000 tick 窗口只覆盖首批到期，更长窗口 miss 数会上升，但每个 miss 的成本只是该 tick 走旧路径一次，与扫描成本无关。

## 量化判定

- `can_prepare` 的 `any` 扫描 = Σlen ≈ 460 万次比较 / 3000 tick ≈ 数毫秒，<0.1% 总 wall（13.7s 量级）；`due_at`、`ensure_order_absent`、`remove` 同量级。
- 索引化（BTreeMap / 堆 / 最小到期游标）预期收益 <0.1%，远低于 2% 保留门槛；命中率本身在 Continuous 期已达 99.8%，无提升空间。
- **结论：不做实现实验，假设关闭，不改语义也不留代码。** 1000x 主杠杆仍指向 22-span 的四大 CPU 段：`continuous_shadow_initialize` 1.825s、`borrowed_existing_projection` 1.777s、`continuous_input_prepare` 1.520s、`seal_total` 1.227s，以及 tick 间串行依赖链。
