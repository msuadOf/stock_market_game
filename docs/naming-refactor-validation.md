# 2026-10-01 命名整理验证记录

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
