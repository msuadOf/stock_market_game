# 命名重构验证记录与历史边界

当前命名与格式契约见 [命名约定](naming-conventions.md) 和
[ADR-0029](decisions/0029-responsibility-names-and-contract-versions.md)。本轮使用 schema 3，
旧 schema 1/2 和旧字段/tag 显式拒绝。下面的旧提交、通过数、失败、未运行项及当时标签
仅记录 2026-10-01 的事实，不作为本轮验证结果；本轮 Clippy、独立复核与提交由实际执行记录证明。

## 当前实施验证

2026-10-04 的职责改名依据全量调查实施：63 个必改名字的 128 条定位、6 个路径目标、
2385 条文本与契约位置及 48 类可选角色名逐项核销。当前存档使用 `schema_version=3`、
`runtime_state` 和 `a-share-simulation`；真实业务版本、数值、法源和密封历史证据保留。
新增遗漏也纳入收尾清单，不用关键词零命中代替完整 AST 核查。

剩余符号实际使用 rust-analyzer LSP 与 TypeScript LanguageService 的 rename；已改名字重新
查询 semantic references。前期语法位点编辑和文件移动不追认为 LSP rename。Rust 63 个目标
取得 441 条引用，TypeScript 65 个目标取得 195 条引用，最终位置核对零漂移。TypeScript
57 条内存 rename 精确复原，8 条动态属性的反向 rename 受限，未冒称全部证明。

| 验证 | 实际结果 |
|---|---|
| Rust 常规测试 | 78 个二进制；2254 个通过，9 个原有 ignored 长用例未执行 |
| Rust doctests | 5 个通过 |
| Web 短测试 | 124 个文件，612 个 case 通过 |
| 脚本短测试 | 32 个文件，380 个 case 通过 |
| 两套 ts-rs bindings | 各 74 个导出 case 通过，保持 `TS_RS_LARGE_INT=number` |
| TypeScript、格式、diff 检查 | 通过 |
| Web lint | 通过；保留 3 个原有 React 警告 |
| Cargo Clippy | `--workspace --all-targets --all-features --jobs 64 --offline -- -D warnings` 通过 |

Rust 常规批次使用 8 个并发二进制，每个 12 个 harness threads 和 4 个 Rayon threads；
engine 修复旧诊断断言后，1096 个单测以 16 个 harness threads 和 8 个 Rayon threads 全部重跑
通过。完整测试批次与构建受进程外 300000ms deadline 约束；Node 普通测试同时使用
10000ms case timeout 和进程树 deadline。Clippy 配置 64 个 Cargo jobs，实际约 42 秒，
采样峰值 29 个进程、145 条线程；累计 CPU 时间约 130 秒，不将线程容量冒称 CPU 利用率。

初次新增 Rust 测试 import 路径错误、6 个旧英文诊断断言失败、沙箱阻止端口/子进程输出、
临时目录不在工作区，以及 bindings 导出超时均保留真实日志。修正调用或断言、使用工作区
`.tmp` 与正确生成环境后重跑通过，没有删除、跳过或放宽断言。四个隔离旧 producer 契约
反例均实际失败，当前对应 case 通过；这只是事后验证反例效力，不补造实施前 TDD 时序。

新旧真实 producer 的 63 份 capture 只逆替换许可身份 token 后，业务事实和完整字节全部
相等，6 个历史固定摘要全部复现。仅存档 schema、runtime 字段、policy 身份和来源 tag 改变；
事件字节不变。五个当前表示摘要据实际输出更新，旧锚保留历史出处，临时导出接口已删除。

额外 verification harness 完整执行为 16 个通过、2 个失败：
`real_protocol_capture_has_committed_evidence_without_claiming_full_acceptance` 与
`public_payload_probe_uses_real_identities_and_negative_controls_diverge` 仍被原有
`local_event_index` 的 zero-based / contiguous 校验拒绝。基线提交 `c0ab429` 的独立 workspace
分别实际复现相同失败，属于既有验证器问题，未在命名任务中扩大修改；不将该套件记为全绿。
未运行 ignored 长验收、真实 simulation 长矩阵或浏览器 E2E。

完整 diff 由未实施者分模块独立复核，发现逐项修复再复核。活动审计 Python 生成源仅运行
`ast.parse` 与归一 AST 比对，不执行旧生成器覆盖历史记录。准确命令、计数、源码指纹、
rename 响应、旧摘要取证与复核回执统一见
[本轮实施汇总](../.tmp/naming-refactor-implementation/summary.md)；本地提交信息以实际 Git 记录为准。

## 历史格式名称对照

2026-10-01 当时保留 `P0`/`P0Expiry`、`P3Created`、`p0_released`/`p1_live`，
以及 `k7-*` schema、`fresh_current_k7_setup` source 和 `P0`–`P9` 计时 key。
当时 summary schema 从 `escrow-task9-matrix-summary-v1` 改为
`escrow-verification-matrix-summary-v1`，matrix version 从 `task9-runtime-v1-matrix-v1`
改为 `escrow-runtime-matrix-v1`，scenario 从 `task9-runtime-v1` 改为 `escrow-runtime-v1`。
这些是旧改名批次的事实；当前机器身份和独立数值版本遵循 ADR-0029，不沿用此表作为新契约。

## 2026-10-01 历史命名整理验证记录

本批在现有工作区、`fix/synthetic-history-policy` 分支实施，分批独立复核和本地提交，不推送。
源码命名清理依据初始全仓审计按职责进行，真实版本与历史格式契约保留。
具体规则见 [命名约定](naming-conventions.md)。

## 已保存的改动

| 提交 | 内容 |
|---|---|
| `990f711` | 51 个流水线模块文件改名和引用同步；旧账本测试改用现有全账户观察入口，保留断言 |
| `866a66f` | 流水线类型、函数、变量和测试名按职责改名；保留序列化拼写 |
| `51a3a26` | 守恒投影接缝使用权威 `SaveAccountSnap`，修复精简存档后的旧类型接口 |
| `2d2672e` | 验证工具的内存检查点与公共日终读档分开，公共保存限制保持不变 |
| `269ff7e` | 9 个工具、Web 和夹具文件改名，51 个工具标识符及调用同步 |
| `d64a5e6` | 清理局部 `p1`、`v1/v2` 与计划编号测试名，删除四个没有构造调用的撤单原因分支 |

压缩旧档只由 `task29-save.json.gz` 改为 `archived-schema-save.json.gz`，字节和哈希未变。
当前矩阵的 summary schema、matrix version 与 scenario 身份标签随工具改名同步更新，
具体新旧值见 [命名约定](naming-conventions.md)；历史密封证据与 `k7-*` 格式身份保持原样。
撤单诊断中的真实 `Termination` 原因不变；生产到期释放仍使用 `Expired`。
仅供测试调用的提交诊断 helper 与自愿撤单 fixture 使用 `cfg(test)`，不再编入生产路径。

## 通过的定向验证

- 第一、二批流水线分套件并发验证：515 个用例通过；第二批另有存档 32、账户 25、契约 4 个用例通过。
- 守恒投影 19、公开保存/恢复边界 19 个用例通过；验证检查点恢复不产生公开存档候选。
- 模拟采集器 74、产物校验器 15、矩阵/性能测试 24 个用例通过；诊断证据审计短测通过。
- 最后 JS 守恒变量改名后，determinism/perturbation、per-envelope conservation 与矩阵消费者合并 21 个用例通过。
- Web 保存与夹具 20 个定向用例、TypeScript 检查、Web lint 通过；lint 仍有原有警告。
- 最后清理批：日历 10、地产 20、报表 18、披露 22、基本面 22、撤单 4、证据 19 个用例通过。
- `TickFrame` / `EventFact` 的两个类型导出用例各自通过，生成的 TypeScript 仍保留 `P0` 标签。
- `cargo check -p engine --all-targets --all-features -j 32 --offline` 通过；格式与 diff 检查通过。

Node 普通测试同时使用 10000ms case timeout 和进程树 deadline。
Rust 普通验证按套件/目标进程拆分，每个命令受 10000ms 进程树 deadline 约束；流水线批次
最多 12 个并发进程，每个使用 4 个 harness threads 和 4 个 Rayon threads。
构建与类型检查使用 32 个 Cargo jobs，并由进程外 300000ms supervisor 约束。
进程树采样观察到最多 32 个 rustc 进程及 28 条 running/runnable 线程；有限采样不表示持续满核。

## 未通过的扩展检查

本次没有完成完整回归，以下结果不得记为通过，也没有删除、跳过或放宽相关原断言。

- 工作区全目标检查失败：`apps/server/tests/{actor,api_contract,ws}.rs` 中 13 处旧 `handles.save()`
  调用未传现行 `Option<SaveCandidateKey>` 参数。这些服务端测试和接口在本次命名任务中未修改。
- Web 全批失败：未修改的 `wasm-worker-failure.test.ts` 中 7 个用例预期
  `wasm-worker.step`，实际为 `wasm-worker.message`；完整 Web 批次不能称为通过。
- Rust 验证 harness 当前 12/14 通过，两条真实执行用例仍被旧事件身份检查拒绝：
  `real_protocol_capture_has_committed_evidence_without_claiming_full_acceptance`、
  `public_payload_probe_uses_real_identities_and_negative_controls_diverge`。
  错误为局部索引不满足验证器要求的 zero-based / contiguous 条件；相关检查逻辑未修改。
- 最后流水线批次出现 `multi_round_trades_record_first_real_open_and_final_depth_once`
  成交价/成交顺序断言失败；此前两批 515 个用例通过不能代表后续批次也全部通过。
- `company_scenarios` 的两条跨日用例超过 10000ms 上限，进程树被终止：
  `lifecycle::civil_information_chain_keeps_unread_beliefs_stable_and_closed_days_tick_free`、
  `restore::restored_and_uninterrupted_sessions_keep_canonical_events_and_saves_identical`。
  公司夹具的代表性覆盖用例通过；未运行 ignored 年界长验收。
- 全部类型导出合并成单命令超过 10000ms，已终止；改用本次实际变动协议的两个独立短用例，
  不宣称完成所有类型导出检查。

本地日志、映射与各批独立审查结果保存于工作区 `.tmp/naming-refactor/`；这些本地产物未加入 Git。
