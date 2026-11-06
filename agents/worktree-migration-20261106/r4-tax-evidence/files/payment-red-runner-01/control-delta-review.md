# 付款三门 runner 机械差量复核票

日期：2026-10-29。裁决：**APPROVED_MECHANICAL_CONTROL_DELTA**。未发现本次机械差量的阻断问题；本票不授予 launch，不代表 source/config 已冻结或实际首红已取得。

本审仅核本目录相对 integration `joint-green50-r2-2026-10-29` 的名单、计数、marker、target kind、绝对工作路径及 freeze/bind/post-WAIT 位置。未重审原监督基础设施、未扫历史指纹或 source 大 manifest、未读取 target 大产物、未运行任何脚本或 Cargo；仅新写本票。

## 绑定

已核 `mechanical-control.diff` SHA-256：`894045b9fe42bc37518b881c236af6191bf8afd3589ab19988a96c2a2becfe3a`。`mechanical-control-fingerprints.json` 所列 11 个机械文件的 SHA 均匹配。对以下五个文件分别执行 byte comparison，均与 50r2 模板相同：`launch-batch.py`、`absolute-supervisor.mjs`、`subreaper-owner.py`、`build-verified.mjs`、`terminate-owned.mjs`。

## 机械检查结果

- 唯一构建为 `engine --lib`，features 为 `simulation-diagnostics,verification-harness`，`-j8`、`CARGO_INCREMENTAL=0`；删除另外两个 integration target。实际 unit artifact 仍要求 `target.kind == ['rlib']`，未误改为 `['test']`。
- cases 与 contracts 均为三项，完整 exact 名称逐项相等并核实际注册名单恰一次；三个 child 的并发池宽为 3，分别使用 `--exact --test-threads=1 --nocapture`，每条 child 命令受 10000ms 进程树 deadline 限制。
- 每门须同时具有 collector 正常退出、准确 child argv、child exit `101`、唯一 summary `FAILED / 0 passed / 1 failed / 0 ignored`，以及 stderr 中恰一个 panic header。panic 路径固定为 `packages/engine/src/session/ordinary_tax_day_end/payment_runtime_tests.rs`，还须命中对应 marker。实际测试源码中的函数名和三条现金断言 marker 已逐项核对：

| exact 尾名 | stderr marker |
|------------|---------------|
| `r4_cash_01_payable_credits_gross_and_activates_personal_income_once` | `R4 付款日必须真实增加持有人现金，不能只保持 Registered` |
| `r4_cash_02_flat_payment_credits_net_and_records_real_withholding` | `R4 简税付款必须在真实账户留下税后净额` |
| `r4_cash_03_actual_sale_before_payment_collects_personal_tax` | `R4 真实转让后的付款必须按原登记权益收缴个人税` |

- 配置内检查与最终检查均要求三项全部 `businessCaseRed && expectedMatched`；总数必须等于 3。green、提前 fixture panic、编译失败、超时或其他 panic 不通过此 gate。post-WAIT 摘要的 selected 已改为 3，executed、rawBusinessRed、expectedRed、green、expectedMatched 均取本批实际结果。
- work 绝对绑定 `/data1/<USER>/workplace/stock_market_game/.worktree/d3-p1-integration-2026-10-28`；唯一 owner 为 `/tmp/stock-market-game-validation-2026-10-27-1311109/engine-r4-payment-red-2026-10-29-01`。本次清理监督差量仅替换 owner，296/297/298/299/300 秒边界未改。
- `freeze-current.py` 只接受固定 integration cwd，先核接收树付款 tests-only 两份来源，再生成 source/config 与 plan；`bind-preflight.py` 在其后接 root 具名的最新绝对 front、SHA、真实 session/WAIT chunk，检查前槽清场和冻结来源，再写授权 handoff。`publish-actual-front.py` 留在实际 WAIT 返回后，以显式 session/exit/chunk 发布本批摘要；不从旧角色或旧制品继承结果。

## 当前边界

本审现场读取的 prepare/slot 均明确 `PENDING`、`rootAuthorized: false`、`candidateBound: false`；未见 source-freeze、snapshot-plan、plan-template、preflight 或实际 batch/supervisor/front 原件。后续仍须 integration 合入并审过 Host 最小 actor，得到 root 授权后现场 freeze、bind，才可 launch。本票不代替该授权、接收树合成独审或运行后的原件核验。

AGENTS 三问限定：本差量只采集既有付款测试的目标失败，不新增 A 股规则；从 50 门裁剪到单图三门与需求一致；未发现机械计数、marker、target kind 或工作路径漂移。没有实际执行证据，不能报告三门已红或任何付款生产功能已完成。

## 追加：现场 freeze 后的小绑定复核

日期：2026-10-29。裁决：**SOURCE_READY_PENDING_ROOT_LATEST_FRONT_AND_SLOT**。本节仅核一次已完成 freeze 的必要绑定，不重审源码或基础设施，不授予 bind/launch。

- `plan-template.json` SHA-256 已实际核为 `f114545791e10c1e9ed483b24826c837ad27eb56f4d6d4708f988ff207bbc69e`。candidate HEAD `7f970b259e442ee562177cef5219d3a58b5a842f`、tree `5f0edf405cc9631b5bc33a6ca3370f285d1ac046` 与当前 integration 一致，cwd、target、owner/scratch 路径保持前述绑定。
- plan 的 1165 项 Rust 与 910 项 Web 清单分别与 `source-freeze.json`、`source-freeze-web.json` 内容一致；只比较冻结数据及清单数量，没有重新遍历或 hash 1165/910 个源码文件。cases、contract 均为三项，完整名称集合一致，plan 两份 command 与 cases 的唯一 engine lib 双 feature `-j8` command 一致。
- receiver 两份付款来源、source-freeze 对应项及 integration 现件的小 SHA 均一致：`ordinary_tax_day_end.rs` 为 `ecad092789c498c068db42d4beee126b27a54704ec5571d22801c514aa6b45a1`，`payment_runtime_tests.rs` 为 `81a21a6d8a9fbf2dbc6e062fa76f27010c5a60c578a9142f9e4f610deae34bca`。已审 Host actor 的 freeze 条目记录为 `5957dabf2cf22e443571ecacf4053a9468a12cea2da3c3c58c7aa0405ff71b99`；本节未重新审该产品改动。
- `expectedNativeCargoInputs` 的 `cargoCwd` 为固定 integration，`cargoHome` 为 `/home/<USER>/.cargo`，记录 `CARGO_INCREMENTAL=0`、jobs 8 和同一 target。当前两处具名 `.cargo/config.toml` 来源分别为仓库根与 integration 根，实际小 SHA 均为 `2fc6f33513ae303f5a8b6186ff9d4ee2e3deb9be4c52a6d8e942e25d1b70f8b0`，与 snapshot 记录一致；未运行 snapshot/probe。
- `runner-freeze.json` 的 22 份小控制原件（含 plan、清单文件、必要 helper）逐项 SHA 检查通过。未读取 target 或重新计算任何产物大 hash。
- slot 仍为 `pendingSlotHandoff: true`、`rootAuthorized: false`，实际 front/session/WAIT 尚未绑定；未见 preflight、batch 或实际 front 原件。下一步仍须 root 最新前槽与唯一 slot 授权，再执行已审 bind/launch 流程。

未发现此必要绑定范围的阻断问题。本审仅追加本票，未执行 Cargo、snapshot、probe、bind 或 launch；没有实际编译、case 或首红结果。

## 追加：Host WIP 本地保存后的 Git metadata 窄复核

日期：2026-10-29。裁决继续为 **SOURCE_READY_PENDING_ROOT_LATEST_FRONT_AND_SLOT**。本次只处理 integration owner 已授权的 candidate metadata 更新，不重审产品或 runner。

已实际核得新 plan SHA-256 为 `70a6ad61b2872217b48e7627c9778a62310aafb2926bae96ff24b0b1ee810ff3`。`candidate-metadata-update.json` 的 after 三字段与 plan、现场 Git 一致：HEAD `347e84f4ae371defd6b83de25888080bfbe9d311`、tree `d499de72d0e57e2f4781d0a563d4fa547f8f2162`，status 为付款入口修改及现有具名 untracked 目录。仅在内存中将 plan 的 `candidate`、`candidateTree`、`candidateStatus` 逆置为记录的 before，按原 JSON 格式序列化，即精确恢复原 SHA `f114545791e10c1e9ed483b24826c837ad27eb56f4d6d4708f988ff207bbc69e`；因此 source arrays、cfg、command、control 等其余 plan 内容保持原冻结值。

`runner-freeze.json` 仍为 22 项，仅其中 plan 条目指向新 SHA；slot 的 plan/source-freeze 小绑定分别为新 plan SHA 与原 `c5ed08034bb80fa983e559c29a65b075f43ed421f8143b6ccc8ff92f8b1bb351`。slot 仍 `pendingSlotHandoff: true`、`rootAuthorized: false`，front 尚未绑定。当前 root #122 占用唯一 RAM 窗口的调度状态由父级提供；付款须等待其真实新 front 与后续明确授权，本票不授权 launch。

产品源码字节未变由唯一 integration owner 声明；本节没有重新遍历或 hash 1165/910 个源码、没有再次 hash 控制文件或 target，未执行 snapshot、probe、Cargo、bind 或 launch。未发现该 metadata 差量的阻断问题；原源码与运行证据边界保持不变。
