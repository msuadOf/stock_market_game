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

## 本次重建实验的冻结源

父任务授权后，已独立创建并核真以下两个 detached worktree；两边 tracked 源码均未修改，只各新增一个薄 example。它们是本次重建实验，不能补造旧历史运行见证。

| producer | 独占路径 | HEAD | probe SHA-256 |
| --- | --- | --- | --- |
| old | `/data1/baiyifan/workplace/stock_market_game/.worktree/step-skeleton-gold-884bd5ad-2026-10-30` | `884bd5ad12d71608ff7db1524292c98b96dc2978` | `16bd371233182727768b54f4cef2c9b57e0ffa3d364bf61bc899dc6e50ee5451` |
| current | `/data1/baiyifan/workplace/stock_market_game/.worktree/step-skeleton-gold-current-2026-10-30` | `8801ef2570f580da06e9332c5c247116402c5758` | `d523051966ff01bfb7ea689b9d1c126a7f2d673758d0b9fb69172d1bc8fac31d` |

两个 probe 均位于各树 `packages/engine/examples/step_skeleton_projection_probe.rs`，且已逐字验证：前缀等于各自 `HEAD:packages/engine/tests/step_skeleton.rs` 全文；后缀 main 原字节相同，SHA-256=`d3b3f2e3d6ef948510f1a8808d2b8e533c56c9f5bbdb2041f782426c0ccea680`。两份绑定原件各在相应树 `agents/branch-completion/last-integration-2026-10-30/gold-probe/source-binding.json`。

实际冻结 main 与上方初步方案的唯一组织差异：每个进程只接收一个 `auction_ticks`，参数为 `<全新输出目录> <0|3|6>`，便于三个 fixture 独立并行，20 拍逐一输出 JSON，并额外写完整原字节拼接 `raw.bin`。`ENGINE_STEP_RUNTIME=1` 显式固定 native 原路径；该 fixture 无 NPC，不以自由调度为旧锚失配理由。capture 正常退出只表示采集完成，不表示 golden 通过。

已交父任务审查冻结源与绑定，再由唯一 integration 在**既有同一 target、既有 profile** 内逐个构建/采集；不新增 target/profile，也不运行新大 seal。本 writer 没有编译或执行这些 probe。

父任务已独立核对完整 probe 前缀、相同后缀与源码绑定，并确认 current 的 Cargo/src/build 输入与 dev 候选相同，已向 integration 发出 READY。排程先完成在途正式补验和 Browser X05，再运行本实验；不以排程等待降低 golden 门槛。

## 表示变化的正式来源（待运行证据核实是否穷尽）

- `company_state` 互斥来源：ADR-0035 的共同入口只持所选模型、严格当前存档决定；`docs/trading-rules.md`“公司模型选源与完整存档（2026-10-27 候选分支登记）”明确 required `LegacyOperations { operations, closing, groups }`。当前 `session/company_state.rs::SessionCompanies::save` 的 Legacy 分支逐项复制原 operations/closing/groups；旧 `session.rs::save_projection` 原字段分别复制 `state.operations/state.closing/state.groups`。fixture 的 `company_system=None`，须实际核对三项完整值不变，不能只核对 tag。
- `runtime_sync_policy` 与 `mirror_origin`：`docs/trading-rules.md`“运行镜像同步与日终存档（2026-10-29）”明确 EveryTick 默认及 origin 表示实际安装来源；ADR-0043 提供 World/Session 安装边界。本 fixture 不做 Admin、成员加入或显式 flush。源级推导为首次 lazy authority epoch=1，初始静点安装一次，每个市场拍再安装一次；World 每市场拍发布 generation+1，日结 retained 在同 tick 额外发布一次。预计 origin 为 `authority_epoch=1`、`install_sequence=tick+1`、`source_tick=tick`、`source_generation=tick + u64::from(tick==20)`、`non_tick_revision=0`，日结阶段为 tick20 `retained_published`、此前为 `tick`；`mirror_phase` 使用 `phase_at_tick(tick)` 的完整分段公式。此为 source 推导候选，仍须和全部 60 份原字节逐项核实，不从实际 hash 倒推。
- 新增更正记录：`SaveSlot.report_correction_operations` 来自 `state.report_corrections.completed.clone()`；fixture 无更正请求，预期完整空 map。仍需独立断言字段存在且空，不能在当前投影中丢弃后冒充覆盖。
- `SessionSetup` 新增公司配置均有明确 `skip_serializing_if`；本 fixture 所有 Optional 为 None、机制开关为 false。实际只应出现新的 `runtime_sync_policy`，需要逐 path 核对，不能假定任意新字段可删除。

旧三锚复现与完整差异清单尚未取得；以上不构成允许重锚的结论。

## 前向表示取证脚本冻结

脚本位于本目录 `gold-probe/forward_transform.py`。最终 SHA-256 为 `a03b31db5f7caa7c8f5acf722cc5751fc13fe4985064dcc4efce63a941bfa52a`；预审 SHA `a3373418e4b3507f89b0a2f1cddbe4de7c1e883f3a104f008767c1d8ba0daf45` 之后只将开盘阶段整数公式由 `2*a//3` 改成与源定义相同的 `a-a//3`，对于登记的 0/3/6 值等价，已通知原审查者。

脚本先验证全部三个旧原字节 FNV，再从旧原字节按上述有限变换生成完整 expected tuple，最后读取 actual 作比较。原有 scalar token、事件数组、对象字段顺序都以原文切片保留；新来源字段由源级预先推导的公式生成。它不依据 actual 增减 path，也不把 JSON 再序列化当作原字节等价；任意未登记差异都产生 mismatch、保存每拍 expected 和完整逐 path 差异并返回非零。

AST 解析与纯构造 sanity 已核对：`1e-7` 的原数字 token、不安全整数以上的 Money 字符串、中文与转义字节均保留。尚未运行实际 engine 证据分析。父任务已完整审读预审脚本，source epoch/安装序/retained generation 公式另查实现复核；最终一行公式校准已交回确认。

## 已发生的取证与失败保留

第一批 `step-skeleton-gold-reconstruction-2026-10-30` 实际运行后，旧 producer 的全部 60 tuple 与三个 raw 拼接已产生。writer 用 3 个进程、10 秒外层门亲自核对，wall=3.806s：每场 20 份 tuple 的原文拼接等于其 raw，全部原 FNV 精确复现。逐拍 SHA-256 保存在 `gold-probe/old-raw-verified.json`。

| auction_ticks | 本次重建 old FNV | raw SHA-256 |
| ---: | ---: | --- |
| 0 | 16039080022122580669 | `e31490c1a4d070f90915265191c3e427b098a5ebb2f0de8cb34985723f3de380` |
| 3 | 7734365835074963189 | `d4b0e0d6a34173d324867f846fa4c0c62b769740d69cbbaab7e5d688e60849da` |
| 6 | 6825886869182669854 | `f78a20aba42457188d3f8452076547e265470e31ef629e053a001e893d2379da` |

该批仍是 FAIL：旧 manifest 原已声明 `num-bigint=0.4.8`，但旧 Cargo.lock 的 engine 依赖列表缺该边，Cargo 构建时补了一行。writer 只读解析原/生成锁文件，629 个 `(name, version, source)` 包记录集合完全相同，唯一变化为 engine.dependencies 增加 num-bigint；旧锁中的 num-bigint 包版本和 checksum 原已存在。runner 保存原/生成锁、完整 diff 后恢复本人产生的改动，未改写 FAIL，也未运行 current。以上只证明本次 old 重建原字节复现，不能称该批完整通过。

经 root/父任务授权，沿用独占 old 路径创建实验分支 `codex/step-skeleton-gold-resolved-lock-20261030`，只将该锁文件一行改动本地提交为 `fbf31a58d014be22972ce609cf0976f8cf1c76f6`。这是**明确的本次重建实验前置**，不是原历史锁；全部生产 src、原测试与 probe SHA 不变。新锁 SHA-256=`558353207e066e3bd9e1fb56b7d7aa56373c0fcccd8de2fef5a17d32b7c43bb9`，后续构建要求 `--locked`。原失败及 60 tuple 不动。

第二批 `step-skeleton-gold-resolved-lock-2026-10-30` 也保留 FAIL：旧 engine artifact 为 fresh=false，current 的 engine 却 fresh=true，且共用同一 rlib/rmeta alias；current source 标签不能证明实际当前字节重编。未将这些输出用于当前 golden。后继由唯一 runner 对每个 producer 的冻结 Cargo/src 输入执行 owned mtime refresh，并 finally 精确 ns 恢复，要求 engine 与 probe 都 fresh=false；不新增 target/profile、不删除缓存。

完整数据约 116MB，不能用短 fixture 替代完整原字节取证。前向脚本已在父任务独审后改为 3 个进程并行：先全部旧三锚核对完成，再按场并行生成 expected 和比较；缓存旧 FNV/SHA 避免重复扫描。完整分析明确归本批共享 300000ms deadline 内的长验收，不能另开一个 300000ms。并行 v2 原 SHA-256=`784f3ff042ac028624021898647cd62b8c2ff0fbc0d72df099e8f374bb2665e1`，原许可路径与公式无变化；v1/v2 原脚本分别完整保留在 `forward_transform_v1_frozen.py` / `forward_transform_v2_frozen.py`，原 diff 保存在 `forward_transform_v2.diff`。

## 当前冻结身份

root 随后接入 c912 的镜像替换成本修正。经 integration 明确确认新 gold 尚未冻结/launch，writer 将独占 current 树机械推进 `c91273a684eb623b9c40043c3d1ff1bf8d5c2b88`；原测试 blob 仍为 `44082fc264d6503a2732707d2a9ff8b21431b2c9`，完整 probe SHA 保持 `d5230519…c31d`，tracked clean。唯一 forward 变化是 `CURRENT_SOURCE_REVISION` 从 8801 改为 c912，最终 SHA-256=`4765217bcce83a2de1569e306b3e3f8c37e897fd24a251d8590d4a7b664ac729`；数学许可及字段公式不变。current 新绑定在其独占树 `gold-probe/source-binding-c912.json`，SHA-256=`d51ffe1cbd9e52908c944d70a10a8cfd3e22d92c48912f16b70215f4b31b7116`；旧 8801 绑定原件不变。

上述冻结阶段，writer 未运行 Cargo、Rust binary 或访问 target；当时三个活动 golden 常量/断言仍原样。后续取证与源码修复如下。

## 完整来源门与追加 PlanBook 表示取证

第三批 `step-skeleton-gold-full-inputs-2026-10-30` 实际保留在主工作区 `agents/branch-completion/validation-scheduler-2026-10-29/`。本轮 old/current engine 与 probe 均实际 fresh=false，逐 producer 刷新冻结输入并在 finally 逐 ns 恢复，`-vv` 原件绑定真实 engine 编译和 probe `--extern engine`；6 个独立 fixture 全部产生完整原字节。原批因前向表示许可尚未包含 PlanBook 而 FAIL，原 `forward/manifest.json`、stdout/stderr、收据和全部原 tuple 不改写。

原 4765 前向差异是每拍恰好 3 个表示 path：`$[2].plans.next_plan_seq` 旧存在新无、`owner_sequences` 旧无新有、`plans` 旧 `{}` 新 `[]`，60 拍均无其它 JSON 值差。writer 实际读取旧值：每拍完整 PlanBook 都是 `policy / next_plan_seq:0 / plans:{}`。追加许可不是忽略业务差异：

- 旧 `884bd5ad:plans/mod.rs` 的 `PlanBookSaveRef` 按 policy、next_plan_seq、plans 顺序序列化；`new()` 初始游标为 0，`PlanRecords` 使用 SerializeMap，未创建记录得到 `{}`。
- 当前 `plans/mod.rs::PlanBook::serialize` 按 policy、owner_sequences、plans 顺序输出；`new()` 使用空 owner map 与默认 PlanRecords，`records.rs::Serialize` 使用 SerializeSeq，空记录得到 `[]`。
- 现行 `docs/trading-rules.md`“计划来源与持久化边界（2026-10-28）”明确 OwnerSequence/PlanId 与 plans 数组，严格拒绝旧结构；ADR-0029 的严格当前格式边界不变。已授权身份迁移在 `16299297`，来源枚举迁移在 `57909d55`，既有核心实施记录 `agents/branch-completion/plan-identity-core-2026-10-25.md` 明示新 wire 结构。
- 父任务作为未实施 Sol 亲读旧新定义、实际旧值与正式契约后，只准本零 NPC fixture：旧 PlanBook 必须 exact 键序 `policy,next_plan_seq,plans`、next_plan_seq 原 token 严格 `0`、plans 原 token 严格 `{}`；保留完整 policy 原文，输出空 owner_sequences 数组与空 plans 数组。任何非空计划或已消费游标立即拒绝，不建立通用迁移器。

v4 脚本仅追加上述 9 行许可，SHA-256=`ce21dd7f65cbcbc8ea27897442f8e2a52f01bacdc32c0c2d51775bdf36cbf0ff`。原 4765 脚本保存在 `gold-probe/forward_transform_v3_frozen.py`，完整追加 diff 在 `gold-probe/forward_transform_v4.diff`；父任务已再次独审。

## 本次独立 expected 与完整原字节结果

writer 仅对已存在、已绑定的原字节作非 Cargo 分析，3 个进程同时工作（观察 CPU 为 100%、99.1%、99.4%），进程外 `timeout --signal=TERM --kill-after=1s 60s` 长取证门，exit=0；没有再申请一个 300000ms 编译预算。本次不可用短 fixture 取代的完整分析不冒充普通 10 秒单测。命令及来源收据在 [`gold-probe/analysis-receipt.json`](gold-probe/analysis-receipt.json)。

全部 expected 先仅由旧原字节与上述预先独审公式生成，再读取当前 actual 比较；60/60 **完整原字节**相同，无事件重排、字段过滤或容忍差。manifest SHA-256=`b3fc5f46d723d3845a7460d35a914ac58854ab1c1f125a93df4920d1b054fa49`，父任务另行逐一复读了全部 60 对原文件与 SHA。

| auction_ticks | 独立前向 expected FNV | expected 与 actual 相同的 raw SHA-256 |
| ---: | ---: | --- |
| 0 | 14515314912140190070 | `21698c9d87dfa48e9592d52b582ba2ac4b7a248fd484d1cac27eb1fc23f9da0b` |
| 3 | 1528640566612864764 | `538e4578e6743410db2caf8df58a6952705d5b0d040d3436b18d3e47dbea7db9` |
| 6 | 571153393436025933 | `16f52d1994ee77449a89ee3f91d4ead740b9bd571e3bcb4815481a3842c1b2fb` |

[`gold-probe/archive/`](gold-probe/archive/) 最小归档包含旧 source、实际 probe、显式锁实验绑定、三个旧 literal、逐拍大小/SHA 与 v4 完整 manifest。116324069 字节旧 raw 采用标准 XZ 无损保存，仅 169108 字节；压缩后已逐字节解压核对，避免把冗余巨大 raw/expected 副本提交到仓库。每场 raw 是 20 个原 tuple 按原顺序直接拼接，archive manifest 中各 tick 的 bytes 长度可原样切回，逐拍 SHA 均保留。压缩读取仅服务取证，不接入产品旧档加载。

## 当前源码小片与剩余门禁

源码提交 `d483869fb3e4f3787bb77b14141f5911011b2d14` 仅改 `packages/engine/tests/step_skeleton.rs`：三个活动完整 tuple 字节锚使用上表独立 expected；旧 literal/source/沿革仍保留。新增 7 字段 RuntimeMirrorOrigin 精确 Eq、tick/phase、EveryTick、LegacyOperations 选源与空 groups、空更正记录，以及 typed/wire 的空 PlanBook/OwnerSequence 精确断言。当前完整 tuple 不剔任何字段，原 seed/tick-order 扰动、重复性、无委托、无成交、价格与跨日守卫均保留。

两份原 `.diff` 取证文本包含 patch 格式所需的空行 context 空格；为完整保留原 SHA 且避免将其误当源码尾空白，提交中以同名 `.diff.xz` 无损保存，读取后即得原冻结 diff，解码 SHA 见 `gold-probe/diff-archives.json`。没有改写原 diff 或放宽源码 whitespace 门。

`rustfmt --edition 2021 --check` 和 `git diff --check` 通过。writer 没有运行 Cargo 或 Rust 测试；源码完整 diff 与后续真实 golden 执行仍由未实施父任务及唯一 integration 接手。此处只报告取证已闭合与源码已提交，不把尚未执行的新测试冒充绿灯。
