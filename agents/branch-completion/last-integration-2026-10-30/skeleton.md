# step_skeleton 收口记录

## 工作区与边界

- 独占 worktree：`/data1/baiyifan/workplace/stock_market_game/.worktree/last-integration38-skeleton-2026-10-30`。
- 分支：`codex/last-integration38-skeleton-20261030`；起点 `7057a495435d9edd9f1032d94f365d22dab79ffe`。
- 首条命令的 `pwd`、Git toplevel、branch 一致；父任务核对后授写。仅修改 `packages/engine/tests/step_skeleton.rs` 与本记录。
- 已读根 `AGENTS.md`、`ROLE.md`、`docs/principles.md`、`docs/testing.md`、`docs/open-questions.md`、ADR-0017/0029/0043、相关架构/交易规则与 Git 日常流程。

## 已知红灯与本次边界修正

原红灯位于主工作区的 `agents/branch-completion/validation-scheduler-2026-10-29/integration-unreached-e1af5aaa-2026-10-30/failed-cases.json`，保留原件。

`characterization_ticks_preserve_price_and_day_boundaries` 在第二天首次市场拍前报 `GameSession::prepare_market_calendar_tick`：要求 `Trading`，实际 `TradingCompleted`。原 fixture 连续调用 60 次 `step`，没有完成自然日日结。此次保留 0/3/6 开盘窗口的全部 60 次市场拍、价格恒定、3 个交易日界、最终 day=3 断言，每 20 拍后显式调用 `end_civil_day`。另核对 settled/next 日期、权威日期、tick/day 不被 civil 日结再次推进，以及恰好一个 `CivilDateAdvanced`、没有额外 `Trade` 或 `DayBoundary`。

本次不改变生产交易规则。依据沿用 ADR-0017 的自然日屏障、`GameSession::end_civil_day` 的现行双时钟契约与 `docs/trading-rules.md` 已登记日历来源；2030 日期为已登记模拟日历，不冒称重新核验全部现行交易所规则。

三个活动 golden 的常量、断言、驱动路径均原样保留，没有新增 ignore：

| auction_ticks | 原期望 FNV | 原红 actual FNV |
| ---: | ---: | ---: |
| 0 | 16039080022122580669 | 14515314912140190070 |
| 3 | 7734365835074963189 | 1528640566612864764 |
| 6 | 6825886869182669854 | 571153393436025933 |

## 旧锚源级定位与尚未闭合的见证

`git blame` 定位三个现有期望值首次写入 `884bd5ad1`。该 revision 的 `tests/step_skeleton.rs::complete_projection` 使用 `game.step()`；`runtime/mod.rs::step_runtime_enabled` 的 native 缺省是 true。因此不能将这些锚描述成已核实的纯 serial producer，改走 `step_with_commit_evidence` 也不能冒充原 producer。

`884bd5ad1..7057a495` 的源级差异已确认至少包括：

1. live `Snapshot` 新增必填 `mirror_origin`，含 7 个字段；旧 source 没有该字段。
2. `SessionSetup` 新增首字段 `runtime_sync_policy`，本 fixture 为 `EveryTick`；其他新增公司配置在本 fixture 下由 `skip_serializing_if` 保持缺席。
3. `SaveSlot` 根 `groups`、`company_operations`、`closing_registry` 改为 `company_state::LegacyOperations { operations, closing, groups }`；当前源码 `SessionCompanies::save` 显式复制相应事实。
4. 新增 `report_correction_operations`；本 fixture 无更正命令。

这些仅是已核实的结构变化清单，尚未证明完整输出仅有这些变化；不能据此删除字段、改 hash 或宣称等价。`/tmp/forensic-baseline` 与 `/tmp/forensic-final` 已不存在，既有历史审查记录只能作历史线索，不能冒充本次原 producer 见证。

`7057a495` 的 extraction diff 实为从私有 serial wrapper 改回生产 `step`；没有 `step_serial_evidence` API，也没有本提交新增的 ignore。此事实已通知父任务。

## 交唯一 integration 的最小取证方案

禁止实现 writer 自行运行 Cargo、metadata、rustc、target/binary。请唯一 integration 分别绑定：旧 producer `884bd5ad1`、当前实际接入候选；各使用该 revision 自己的 `packages/engine/tests/step_skeleton.rs` 原文构造 probe，不手抄 setup，不修改原文件。

probe 构造方式：将原测试文件全文复制到临时 `.rs`，末尾追加下面的 `main`。普通 binary 编译不会运行 `#[test]`，而原 `complete_projection` 和 `projection_digest` 可在同一 module 中直接调用。依赖使用该 revision 实际构建的 engine/serde_json，旧新不能混链接。

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).expect("必须显式提供证据目录");
    std::fs::create_dir_all(&output)?;
    for auction_ticks in [0, 3, 6] {
        let projection = complete_projection(auction_ticks, 42, 20);
        println!("auction_ticks={auction_ticks} fnv={}", projection_digest(&projection));
        for (index, bytes) in projection.iter().enumerate() {
            std::fs::write(
                format!("{output}/auction-{auction_ticks}-tick-{:02}.json", index + 1),
                bytes,
            )?;
        }
    }
    Ok(())
}
```

构建明确多核并受进程外 300000ms deadline；两个 probe 分别受 10000ms 进程树 deadline，可独立并行运行。保留构建命令、source SHA、engine/serde_json 实际链接身份、stdout/stderr、60 份原 tuple JSON 的 FNV/SHA-256 与完整原字节；先要求旧 producer 复现三个原锚。若不能复现，应报告旧来源/构建/运行环境差异，不继续修改当前锚。

旧锚复现后逐 tick 比较 `[events, live_snapshot, save]`；事件数组、tick、账户/资金/股份、价格、业务事实与顺序不能静默过滤。若拟做旧格式表示投影，须对每一个许可 token/path 给出旧新源定义、构造/保存来源与逐路径原值证据，并独立断言现行 7 字段 origin 和所有新增权威字段。没有源级与运行证据时不实施投影，不倒推 hash。

## 检查与状态

- `rustfmt --edition 2021 --check packages/engine/tests/step_skeleton.rs`：通过。
- `git diff --check`：通过。
- 本 writer 未运行 Cargo、测试 binary 或构建；边界修正尚待统一实测与未实施者完整 diff 复核。
- 3 个 golden 红灯仍待旧新 producer 取证，不宣称本任务完成。
