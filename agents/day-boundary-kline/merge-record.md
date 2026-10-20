# 日界批合并执行记录（main ← feat/day-boundary-kline-fix）

> 合并执行 agent 产出台账。分支 tip `17c9d0ad`（5 提交，rebase 自 main
> `1554b89a`），并入 main `1554b89a` ⇒ merge commit
> `07f77281943a011d3dfa8afeddee3a5409e8ce6d`（--no-ff，ort），已推送
> （`1554b89a..07f77281 main -> main`）。实现批台账见同目录
> [`ledger.md`](ledger.md)。

## 合并与冲突

- **git 冲突 0 个**：main 自 `1554b89a` 后无前进，分支即基于该 tip
  （merge-base=main），纯领先 5 提交（`f6553831`..`17c9d0ad`）并集合并。
- **双向校验双 PASS**：merge vs main 差异文件集 ≡ 分支自基点改动集
  （9 文件：desk/drive/protocol/world/session.rs/session candles/
  day_boundary_kline_tests/failure/ledger.md）；merge vs 分支 tip 差异
  文件集 ≡ main 侧改动集（∅）。**merge tree-id `3d5e0761` ≡ 分支 tip
  tree-id**——逐字节并集，物理上无任何一侧回退。

## 回归对照（合并树 07f77281，2026-10-20）

| 面 | main 基线 | 合并树 | 判定 |
|---|---|---|---|
| engine lib | 2206/0 | **2210/0**（+4 日界新测试） | 绿 |
| engine tests 面（--no-fail-fast 语义，73 binaries 含 4 examples，xargs -P 32 × --test-threads 4，face wall 214s） | 3408/1 | **3412/1** | 红集不增：唯一红 `tests/session.rs::full_day_accounting_rejects_a_missing_day_boundary` = main 双亲预存（分支台账已实测对照），实现批验收时的另一 flake 未复现 |
| save_contract | 38/0 | **38/0** | 预存债收口零回退 |
| web tsc（apps/web 直跑 tsc -b） | 0 | **0** | ≡main（分支 diff 零 .ts 文件） |
| wasm32 check（engine / web-wasm，RUSTFLAGS=""） | 绿 | **双 crate Finished** | 绿 |
| B 探针（详见下） | ±2% 门 | **中位 +0.11%** | 门内 |

## B 探针对照（driver_semantics_probe，3000 tick 主口径）

- 臂：P=pristine main `1554b89a`（worktree `.tmp/dayb-probe-main`，binary
  sha256 前 16 `e254e7ab37ee89ea`——与 B4b 合并臂同值，docs-only 提交
  树一致旁证）；M=merge `07f77281`（`44791eb908ea28bc`）。
- 跑法：`taskset -c 96-111` 钉核，warm 各 1 次后 5 对交错、**对内奇偶换序**
  （pair 1/3/5 P 先、2/4 M 先，吸收位置冷启偏差）、对间 sleep 5；
  `probe .tmp/driver-semantics-setup.json 3000 16`（5 股/16 runtime_groups，
  setup 同 B4b 批）。
- wall_s：P 中位 9.84403 / M 中位 9.85450 ⇒ **中位 +0.11%**；均值
  P 9.85614 / M 9.85317 ⇒ **均值 −0.03%**——±2% 门内。对内差
  −1.2%/+0.4%/+0.2%/+0.6%/−0.03%，无位置翻转形态。
- 守恒与事件数：两臂 cash_ok=true、events=21917 十次运行逐位一致
  （与实现批"该 fixture 单日无日结、执行面逐指令零差异"结论互证）。
- **污染窗登记**：本窗外部负载 load 48-58/128 核（htop/kindnetd/
  containerd 等常驻），两臂绝对 wall（~9.85s）较 B4b 净窗（~2.7s）
  慢约 3.6×——配对同窗对照下比值仍有效；绝对速度不构成结论。

## 独立复核（fork，未实施合并方）

**复核通过，零 P0/P1**（只读复核 `1554b89a..17c9d0ad` 合并树，未跑测试
未写文件）。三门禁结论：

1. **大 A 语义 PASS**：收盘成交经 unroll 落当日 K/900 桶、reroll 归档终 K
   并置 `last_close=收盘竞价价`（无成交=最后价）、`traded_days_total`
   恰一次、receipt `civil_date=当日` 即时入账；撮合/涨跌停/清算选价
   零触碰；失败路径 desk Err ⇒ worker Failed 退出（半 unroll 态不可
   存活），world 侧 abort_day_end 与 run_day_end 失败双处对称清
   pending 缓冲（含 S1 修复）。
2. **最小性 PASS**：合并 stat 恰 9 文件 ≡ 分支净 diff，无夹带；B2 豁免
   退役为收紧（删放行分支）。
3. **遗漏/复杂度 PASS**：closing=0/未跨日=None 边界、多日独立、清算价
   ≠锚定均有测试或注释锚定；manual 面/abort 重跑/VWAP 近似均已登记
   ledger 遗留节。

- **P3×3 登记（非阻断，留后续批）**：① drive.rs:1417
  `turnover_cents.max(0) as u64` 静默钳负（建议显式失败）；② manual
  day_end 不调 `take_drained_day_candles`（当前时序不可达，随 S5/S6
  留意）；③ `first_trade_seq = next_seq+1-count` 反推依赖
  `project_trade_events` 连续编号隐式契约（可加断言）。

## 推送

- 合并 commit 推送：`1554b89a..07f77281 main -> main`（绿后授权流）。
- 本台账随 docs commit 随后推送。
