# P2-4a：账户回声稀疏化——红绿证据

日期 2026-10-17。实现 agent：第 14 任。分支 `exp/std-thread-model`。

## 动机（数据先行）

- 前任 dwarf 断栈根因闭合：探针二进制无 debug 段（`readelf -S` 0 个 debug
  section）——按方法学以 `CARGO_PROFILE_RELEASE_DEBUG=1` + `RUSTFLAGS=-C
  force-frame-pointers=yes` 重建专用画像二进制（.tmp/driver_semantics_probe.fp），
  fp 调用树可用（perf-records/p14-fp-600.perf.data，10048 samples）。
- 画像实锤：组线程 `Vec::clone` 19.5% 样本（其中 `Vec<GroupAccount>` 克隆
  ~7% + memmove ~10%）+ 主线程 `step_with_capture` 内
  `drop_in_place<Vec<GroupAccount>>` 9.8%。
- 源头：`drive.rs` step_with_capture 每拍 `snapshot_accounts()` 全名册回声
  （20000 账户 × 8 堆 Vec/账户 = 16 万分配/拍）。P2-1b 只稀疏化了写回，
  读面仍全量。
- 对照：信封投影增量化（协调者原批首刀）stub 全段跳过配对 perf stat 循环数
  6 轮中位仅 −2%（5/6 负）——降级候补（负结果归因：resting 常数段主线程
  占比 ~3%，远低于预估）。协调者已批准换刀。

## 实现（M3 同式，镜像 P2-3e EchoAttentionSparse 模式）

- `GroupIn::EchoAccountsSparse { wanted }` + `UpMsg::GroupEchoSparse`（新消息对；
  全量 `EchoAccounts`/`GroupEcho` 读面不变——调试纵切/日结 force_full 用）。
- 组端：只克隆请求指名账户（请求序回执）；下标越界=RuntimeError；
  `#[cfg(test)] sparse_echo_omit_last` 红证注入位。
- world 收集端 `snapshot_accounts_sparse`：回执集与请求集逐位核对
  （`validate_sparse_echo_reply` 自由函数——同长/同序/同下标；缺项/错位/
  多项=RuntimeError 不静默）。
- 驱动器：dirty 集计算前移，非 force_full 拍按 dirty_union 构建 wanted
  （升序天然），force_full 拍走全量并转 (组,下标) 对形态共用回写循环。
- dirty 位→组映射越界显式 Err（不静默跳过）。

## 守卫（drive_tests.rs）

1. `p2_4a_sparse_account_writeback_matches_force_full_bitwise`（协调者点名的
   全量对照守卫）：同 fixture 同种子，稀疏生产路径 vs 每拍 force_full 全量，
   期末 business_state_hash + 事件摘要（去 seq+排序——股间事件序并发非确定）
   逐位一致。fixture：4 NPC、auction_ticks=1（连续竞价每拍成交使 dirty 非空；
   默认 15 拍全程集合竞价事实仅在 AuctionCompleted 落地）、t==3 股 1 一次性
   挂单卖（100 整手——零股规则静默跳过 50 非整手曾使 fixture 退化）。
   判别面前置：组快照断言 pending_sell_qty>0（挂单真实挂出）。
2. `p2_4a_sparse_echo_missing_item_fails_loud`（铁律二）：组端回执丢最后一项
   （cfg(test) 注入通道 CorruptSparseEchoOmitLast）→ 生产步进必须显式 Err
   （错误文案含「稀疏回声」）。

## 突变红（三突变全部实证）

- M1（请求集恒空 `for w in &mut wanted { w.clear(); }`）：
  `p2_4a_sparse_echo_missing_item_fails_loud` 红（无稀疏回声→无缺项错误→
  expect_err 失败）。**同窗登记**：等价守卫在 M1 下仍绿——写回值与 same_tick
  成交投影值冗余（在手+在途口径下挂单 qty 中性、成交面投影全覆盖），空请求
  不改变期末态。此为系统既有冗余设计（回写=对齐安全网），非守卫缺陷；值级
  判别力由 M2 实证。
- M2（回执值腐蚀：结构合法但 invested_cents 清零）：
  `p2_4a_sparse_account_writeback_matches_force_full_bitwise` 红（期末业务哈希
  不一致）——取回值真实流入 session 状态的判别力实证。
- M3（收集端精确集校验禁用）：`p2_4a_sparse_echo_missing_item_fails_loud` 红
  （缺项步进静默成功）——结构校验接线实证。

## 命令

```
cargo test -p engine --lib p2_4a          # 守卫双绿
# 突变：上述 M1/M2/M3 补丁逐个施加后同命令（红）再还原
```

## 等价判据（外部）

- B 探针 events/fees 逐位：300 tick 3336/0、1000 tick 11119/5705（见
  p2-4a-green.txt）。
- A 探针抽查：runtime_s5_probe 守恒逐位 6438600/647541185/6438600。
- 全量回归双绿：默认 + ENGINE_STEP_RUNTIME=1。

## 性能数字

见 p2-4a-green.txt（B 口径 300×3 + 1000×1；机器负载窗口如实注明）。
