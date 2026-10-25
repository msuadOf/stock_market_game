# B4d D0：日结账户立即交付记录

日期：2026-10-25 至 2026-10-26。基线：`f5820291`。工作分支：`fix/b4d-day-end-install-d0`。源码候选已本地保存为 `d3c331a4`，R3 独立终裁待回传，不据本记录宣称完成。

## 因果与范围

- `GroupDayDone.accounts` 已携带组端 Drain、T+1 解冻后的完整 `GroupAccount`；`world.last_day_end_candidate.groups` 保留这份已校验回执。
- 原 `RuntimeDriver` 三条日结入口只将组端状态保留，手动/生产面仅置 `force_full_accounts`，账户到下一真实交易拍才同步；收盘竞价成交后立即 `save` 因而保存旧现金/持仓/成本。
- 修改只落在 `runtime/drive.rs`、`session/failure.rs` 、`runtime/drive_tests.rs` 和 `session/failure_tests.rs`。提取现有 tick 账户安装循环，与日结 candidate 共用，保留公开现金加回在途买单 purse、持仓加回在途卖余量、T+1、成本、清仓删项与首买经历初始化的原有口径。
- 手动顺序为 World 日结成功 → 立即账户安装 → 收盘确认 → civil；生产 `world_day_end_for_step` 在同一日结拍安装，复用原有 `RuntimeInstallBaseline`/undo，不改变 World 发布或 Abort 协议。`day_end_with_market` 转调用 `day_end`，已承接安装。
- 保留 `force_full_accounts`。日结首买经历价格使用 Drain 买腿实际成交 VWAP，名册按真实 `(group, local)` 映射，非满尾组空槽沿用现有 installer 的忽略语义。

## 失败边界

- 手动入口仅新增镜像基线：账户 cash/positions、retail 经历页、个人确认共享历史/receipt watermark、三类 undo 日志及去重集合；driver 同步恢复 `pending_hold_inits` 和 `force_full_accounts`。`runtime_hold_experience_init` 实际只写 retail 页/对应 undo；已逐行核过，不靠字段名称推断。
- 新 installer 可在账户写入后因首买经历冲突失败；测试真实触发该路径，验证镜像恢复，已发布 tick 保持 6，组端恢复收盘挂单在途，未回退最后交易拍。
- civil 拒绝时恢复上述 D0 新增镜像副作用。此处没有宣称全部 civil/保留快照事务原子性：原 `freeze_retained_day` 的发布边界、`day_end_with_market` civil 后行情安装失败等既有跨域事务债留待 D1。
- `day_end_with_runtime_market_day` 原本只推进市场 day，不执行完整 session 日 K 迁移；其原生 save/restore 会报 `fault2064`。测试没有伪造 K 或日期使该 API 看似可原生保存；save/restore 由真实 production 路径验证。
- 本批不闭合 B4c 的 spine→group 公司权益回灌，不承诺公司行为跨下一拍持久；不涉及 PlanId/#122 或新增 AdminRight。

## A 股语义

本批修复已结算市场事实的公开交付时点，不变更撮合、手续费、申报数量、T+1 及公司权益规则。现行规则基线沿用 `docs/trading-rules.md`（记录核对日 2026-09-22，2026 版自 2026-07-06 生效），官方原件为该文件所链沪深交易所 2026 年交易规则。新增测试使用深圳 MainBoard 100 股限价、真实收盘竞价成交、实际费用与 T+1 解冻，不把公司权益或在途资源当作额外现金。未新增两市/板块统一默认值。

## TDD 与验证

- 先添加测试、编译原实现，再运行红证。最初 fixture 卖方选择错误已更正，真实红证见 `red.log`：production AccountId(2) cash `1347902 != 1447351`，manual AccountId(4) cash `232852 != 132351`。两个失败均来自账户交付断言；civil 失败对照当时已绿。
- 最终六个新测试均通过（`d0_` 过滤另含四个既有 D0 测试，共 10 绿，2.99 秒），见 `green.log`：三组含非满尾组立即 candidate 对账和下一实际 step 存留；production 日结静点 cash/positions/T+1/invested/recovered 与 candidate 一致，save/restore 保真；civil 失败恢复新增镜像；部分安装失败恢复镜像且已发布 tick 不退；真实名册的非连续 `AccountId(99)` 也能恢复。
- R3 最终 binary 另有 10 个既有短测通过：`day_end_public_views_tests::failing_` 四例（#121 守卫）、账户 dirty mask、账户 undo、成本清仓、Continue 早轮恰一次结算、主动/被动/加仓投影矩阵、日结 execution leg Abort。
- 每条测试命令均由 `scripts/run-with-deadline.mjs 10000` 外部监督；最终新测试同一 binary `--test-threads=5`，其余七个独立进程并行，最长普通命令约 7.77 秒。构建采用独立 target、`CARGO_BUILD_JOBS=16`/`-j16` 与 `runBoundedCommand(timeoutMs:300000)` 外监督，最后一次增量构建 44.66 秒。运行中观察到 rustc 多线程及多个并发测试进程，无完整 all 回归。
- `git diff --check` 通过。仓库既有全量格式化差异与数百条既有 Rust 警告未顺手修改；只格式化本批新增/提取片段。

原始本地日志目录：`/data1/baiyifan/workplace/stock_market_game/.tmp/sol61-b4d-d0/`。日志哈希：

| 原件 | SHA-256 |
|---|---|
| `build-final2.log` | `238c174c990aed183daf35c350260ecdb0ece14d303c807d4f616e34dfeaa4c3` |
| `red2.log` | `1f35b337b7bc7c46805d8e503abdab0482ab1ad37939fec0352357347652588a` |
| `d0-final.log` | `619f17bb65ee889941b76149c452cde626fd7d63398ae9cdb9b5512d403113e6` |
| `test-1.log` | `f7ff01e033026ab143f663d1038f1caa561f79cdb0e5d61ce09e49466a282746` |
| `test-2.log` | `f28b06b4211b59610f785a2bb759d26d53b9ae3b09f777f9c1fcf9984121952c` |
| `test-3.log` | `8e4acfd97f6398e8616d46b99e70f47d6f5ae1b56bd032224fac2d810d03934b` |
| `test-4.log` | `0c2fb96835626d30c69c7e75fd53044e7e98e02f7b3e28d93fcd82466b1132e0` |
| `test-5.log` | `1a06d380703493458cbffc031724296d0022424f0be27a3d277640ebaa34d7cc` |
| `test-6.log` | `8cc98354fd435293ea7dff03ba818539b2651daa7a5b4514641fcd0c9563af8a` |
| `test-7.log` | `a0b246b1bf4298195cb33681cae8653b8775b8798be102554a8e936f3b6ab41e` |

## R2 / R3 复核收口

- 自查发现新镜像基线若沿用 `runtime_accounts_snapshot` 的 `0..account_count`，会漏非连续经济成员账户。先补 `session::failure_tests::d0_manual_day_end_mirror_rollback_covers_sparse_account_ids`，真实红证 `42 != 123456` 见 `sparse-red.log`；R2 先局部修新 helper。
- 独立复核确认 production `accounts_full=Some` 分支同样依赖该 snapshot，并清空 undo；因此 R3 收口共用 `runtime_accounts_snapshot` 为真实名册遍历，手动 helper 重新复用它。生产测试在真实日结成功并返回 `RuntimeInstallBaseline` 后，明确模拟尾部资产安装写入并调用生产回滚，覆盖 cash/qty/T+1/invested/recovered，且 World 已发布 tick 保持 4。`production-baseline-red.log` 真实红证为 cash `42 != 123456`；这是生产 baseline 契约测试，没有伪称它是实际 closing 成交或 frame 尾故障。
- 复核确认新增 closing 首买经历不得随安装顺序记到下一日或分钟零点。两条真实红证见 `closing-moment-red.log`：manual minute `0 != 240`，production day `1 != 0`。R3 日结 helper 使用 `candidate.day`、当日 civil_date 与该交易日末绝对市场分钟；`runtime_hold_experience_init_at` 共用原 undo/guard，普通 tick 继续使用原时刻。未改组端经历规则。
- 收窄 `day_end` 原有“一个原子序列”注释及手动分支“全量回滚”错误消息，只承诺 World 日结及本批镜像的可确认回滚域，不暗示 civil 全域原子。
- 最终 R3 代码冻结 diff SHA-256：`b70c5e137a95d8d9d951ef3d54ca792cfcba1af3a048911784e3022f18233fc1`。正式 `collectFullRegressionSourceFingerprint` 源码指纹：`54c370fd9205d9c8dc560998cb8f8d374e6b25004ad7211eb5b58e46f8135a9e`；测试 binary SHA-256：`f94c8c5ec190d4a02c0ad22eba476a5bfa7637c8c35ed3a3f6dfc4319009c84a`。详见 `artifact-binding.json`，原始完整 manifest 在其中所记位置。
- `green.log` 现为最终 R3 binary 的 10 绿日志。最终 binary 的额外十例回归日志为主树 `.tmp/sol61-b4d-d0/r3-test-1.log` 至 `r3-test-7.log`，全部通过，最长 7.77 秒；源码未在这些运行之间变化。

## 独立复核

待未实施者 `sol61_b4d_readiness` 检查冻结完整 diff，结论由复核者单独回传。

R3 原始日志 SHA-256：

| 原件 | SHA-256 |
|---|---|
| `build-r3-green.log` | `b18e172edef732521601635f14c6eef00c9d6d7c53bad0d1e6ae7cbb3bdf8bee` |
| `r3-green.log` | `c8b213dd7635dae14460554d667365a14a72fb9f63b61fd585739b21a0984afc` |
| `r3-red.log` | `e0a8c5c1aa4d8c02cae52dbf6a083bb55e503966f276cca6ad7069f4deb7480d` |
| `baseline-sparse-red.log` | `65a8d0750f4aea6183ee6eac3e7457d66ce0ab06a6edce8085a70ea469adecad` |
| `r3-test-1.log` | `e4782541cd32feeb0cfa1c8d75dd2bbe651c7086be7255b705144ed19a05b1e6` |
| `r3-test-2.log` | `78ad1de57110af734e13dc1b9b3d453262850ac6df201ce8d0d601dfa6b00773` |
| `r3-test-3.log` | `235e6e54a0fe9ca69e3d37248c16448a1a063b0cf384fadeb26f644b42febd97` |
| `r3-test-4.log` | `e170f4f155755a96294306f42c244c79bb94dac186b5e44404637497056cca7b` |
| `r3-test-5.log` | `e2e60ca2939f2412c97ef8d182ae240de3ca026bc78bf8d18d36a30eb8a77d61` |
| `r3-test-6.log` | `89d99025cb2378995904ba36a7a91bec9e6bc087af8a3685363fa10f7944f284` |
| `r3-test-7.log` | `c23a9501e1a5b2ed36abc2d2976eabe9593046d493df04185d30bc3021b33eb0` |
