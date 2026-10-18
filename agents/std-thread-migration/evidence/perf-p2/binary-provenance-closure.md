# 性能 binary 构建绑定闭合补证（Sol 终审整改 3，2026-10-18）

## 判决指认的缺口

正式账本（perf-phase1-final.txt）绑定 HEAD `1f5863cd`，但该提交
**单独不可编译**（后继 `6dcc25b4` 补 P2-4b 漏提交的 protocol 差分
类型 re-export，承认当时工作树掩盖）——binary 不能直接归为纯 HEAD
构建。

## 三样恢复判定（判决 §131 口径）

1. **完整补丁**：可恢复——`6dcc25b4` 即该补丁（1 行
   `packages/engine/src/runtime/mod.rs` re-export），diff 可审计。
2. **工具链**：`rust-toolchain.toml`（channel 1.96.1）随仓库版本化，
   构建期与本补证期一致。
3. **binary 来源**：磁盘 binary（Oct 17 20:50-51 构建）SHA256 与账本
   登记逐位一致；时间线：binary 构建于 `6dcc25b4`（20:56:39）之前，
   即构建源自 `1f5863cd` 检出 + 工作树内 re-export 行——与判决推断
   吻合。

## 复现实验（本日执行）

在**纯提交树** `6dcc25b4`（= 1f5863cd + 补丁，无工作树差异）重建：

```
git checkout 6dcc25b4
cargo build --release --example driver_semantics_probe --example runtime_s5_probe -p engine
sha256sum target/release/examples/{driver_semantics_probe,runtime_s5_probe}
```

结果（与账本登记**逐位一致**）：

```
driver_semantics_probe 6123db29b892f2e42632eae557de6e6ecfed95ead4cc60842ce29bef8848745a
runtime_s5_probe       ced474fac39be2a1c613b921553b8db481cc0b40997255798ed1b398154ee92d
```

## 结论

binary 来源闭合：正式五轮测量所用的两个 binary 确定性来自
`1f5863cd + 6dcc25b4 补丁` 的源树（rust 1.96.1）。账本 HEAD 标注应
修正为"1f5863cd+re-export 补丁（即 6dcc25b4 内容树）"——数值
（A 1409.2 / B300 302.7 / B1000 501.5）维持**历史测量报告值**地位
（判决 §95 已裁定），不重测。

**口径勘正**（判决 §95）：B 测的是 `GameSession::step()`（经
step_via_runtime）；step_frame 接轨后生产入口 `step_frame()` 增加
TickFrame 投影/稳定键/PublicRuntimeState 组装面，其完整成本未在
上述数字内——终验收后续基准应按 step_frame 口径重测（本轮不宣称）。
