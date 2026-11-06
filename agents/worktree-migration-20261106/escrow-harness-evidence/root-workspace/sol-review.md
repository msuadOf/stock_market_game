# escrow performance harness 合同独立终审

结论：**APPROVED，仅限下述测试文件补丁**。未发现未修阻断项；不代表正式性能测量恢复、full13 或整树验收通过。

复核者为 `/root/sol61_proc_exit_review`，未参与实现。独立读取完整冻结 patch、现行生产 validator/report 逻辑、原始退役提交、测试与运行原件，并核对实际 SHA、集成树 `git apply --check`、观测 PID 和临时目录；没有运行测试/build，没有修改集成源码。

## 精确批准范围

| 对象 | SHA-256 |
| --- | --- |
| `fix.patch` | `571d243d563b6106dc795bb4abe81b38e9d4ecce7a6cd4e60f09fbb0c4f0e7b0` |
| `scripts/simulation/escrow-performance-harness.test.mjs` 基线 | `c8a2ddcd4bb2a34ddede0ada914e647af0b8cedcd249fdb8cd31b5dd0e8385db` |
| 同文件批准结果 | `5b2794905c3a8746720344532291d6607916faa708d75102d293bfedacf7eabe` |

精确 patch 只含一个测试文件。实现 worktree 相对旧 HEAD 可见的 `run-with-deadline.mjs` overlay 属于集成源，不是本包交付，禁止顺带带入。集成前独立 `git apply --check` 成功，源/patch 最后 SHA 与表中一致。

## 合同归因与断言承接

原提交 `f86684ad615132d89afb8a29496ec2698ce878c5` 的真实 diff 明确退役 environment 的 `rayon_threads`、报告样本的 `rayon_registry_capacity_samples` 和基于 pool size 的容量比较。现行 `validateEnvironmentContract` 的 exactKeys 仅接受 cargo/rustc/target/rustflags/cargo_jobs。旧 fixture 仍带退役键，使多个有效断言在更早的 environment 校验处失败。

测试修复没有恢复生产旧 schema：fixture 删除 `rayon_threads`，同时新增现行 config 可接受、额外旧键明确拒收的断言。原 registry-capacity 数组产出断言改为 own-field 不存在；额外携带该旧字段的 sample stdout 仍由现行 exactKeys 拒收。

原 `[1,1]` 应因 pool capacity 不一致而拒绝的断言已无现行合同依据。修复将其迁为 `[1,1]` 可接受，并验证空数组、负数、非整数拒收。另补 OS thread summary 的 runnable 大于 sampled total 明确拒收。原 histogram 不一致、缺 runnable、缺/不完整 phase、tick/source/toolchain/setup/repetitions 漂移、配对过程、报告复用、防污染、幂等写入和真实 child 清理断言均保留，未放宽原 regex。

现行 harness 的 REQUIRED_PHASES、原测试 fixture 与 Rust `PhaseTimingPhase::ALL` 均为同样 10 项，本包没有修改 phase 列表。ADR-0043 记录 std::thread 方向及 OS CPU 约束；本报告不把其 draft 状态改写成最终验收通过。

## 路径隔离与最小范围

CLI `main` 会以模块所在 workspace 为根校验 config/output 路径。原 helper 从外层 TMPDIR 创建目录；TMPDIR 指向主工作区时，独占 worktree 的真实边界校验会正确拒收该目录。修复只在当前 workspace 的 `.tmp` 父目录下 `mkdtempSync` 创建随机独占 fixture 根，再由它派生临时报告目录，保持生产 workspace/symlink/outside 校验不变。

随机根使同文件两独立进程的 fixture 与 after 清理互不干扰。首次并行方案曾改深层 cwd，导致原 `process.cwd()/../outside` 实际仍在 workspace 内；该配置失败原件保留。最终从 workspace 根启动两个分片，原 outside 断言及 regex 没有改动。

## 原始验证与纠正记录

以下为员工实际运行的原件，复核者已读取并核对，没有重新执行：

| 记录 | 实际结果 |
| --- | --- |
| `red.stdout` / `red-result.json` | 原 20 case：10 pass、10 fail，exit 1，批次记录 866.391ms |
| `attempt1-*` | 初次深层 cwd 方案：19 pass、1 fail；完整保留，不冒称绿 |
| `previous-gnu-*` | 先前两分片 12+8 绿、606.926ms；外层 GNU timeout 的监督口径存在缺口，未追认为最终共享进程树证据 |
| 最终 `green-*` / `green-result.json` / `verification-receipt.json` | 两个真实 Node 分片 12+8 全绿，0 fail/skip，两 shard exit 0，共享批次 570.775ms |

最终实际命令为：

```text
node scripts/run-with-deadline.mjs 10000 -- python3 agents/escrow-harness-contract/verify.py
```

各 shard 另受项目 `run-with-deadline.mjs 10000` 监督，Node case timeout 为 10000ms。两个互斥 suite 名称分片覆盖原全部 20 case；128 CPU 可用，`/proc` 原始采样记录 40 次双分片重叠及实际 CPU tick/线程数，未以 `--test-concurrency` 参数冒充真实并行。

最初外层为 GNU `timeout 10s --kill-after=0.1s`，而 Python 启动独立 session，不能凭该 wrapper 宣称整个分离进程树受共享 10 秒总截止约束。复核发现后，员工保持源码/patch 不变，只用上述项目 supervisor 补跑同一批次。旧原件保留，README 明确纠正。最终 receipt 分别记录工具 wall 与 driver wall，不混用两种计时。

复核者独立确认最终记录的 8 个 PID 均已不存在，fixture 随机根无残留。最终 TMPDIR 故意指向主工作区的外部路径，helper 未创建该路径；真实 workspace 边界仍有效。真实 child 清理 case 也全部通过。

## 三项门禁与范围限制

1. **A 股语义**：只改性能证据工具的测试合同与临时路径。市场规则、撮合、现金/股份单位、存档/API/UI 均未改，生产脚本未改，无新增交易制度实现需要补查官方依据。
2. **必要性与最小范围**：以原始退役 diff 和现行 validator 为依据迁移过时断言，没有为测试通过恢复退役线程配置，没有新增生产抽象、依赖或宽限。单文件范围足够。
3. **边界与一致性**：有效防篡改和所有权断言保留；新增退役字段拒收、现行 runnable 边界与 OS 线程摘要校验；路径修复验证了跨 worktree TMPDIR 场景。没有发现未修的边界遗漏、跨层语义漂移或不必要复杂度。

`source-lock-before.json` / `source-lock-after.json` 与复核者现场 SHA 一致：生产 harness 为 `9a7b769811169fd83dd668f322803b509a0d06fa136091966103e79f356090b6`，source-manifest 为 `d610094dab9014d53927f000a389af3724a9b1c3ba5c9076fa6c5f1416cb3939`，deadline 为 `3f391c87ab5032c3c9acfea70a131a8d1694c0f94a27d1e3f69c1b43412f3233`，均与集成源相同。

本专项没有运行全 scripts、完整回归或正式性能 benchmark。正式性能端点准备链已显式退役，局部测试绿不能证明恢复该能力。精确单文件批准已同步 Leader 与实施员工；后续完整验收仍由主任务进行。

## 证据位置

冻结原件位于 `.worktree/escrow-harness-contract/agents/escrow-harness-contract/`，要求保留归档；主工作区同主题目录保存本独立报告。

- `README.md`：合同、断言映射和口径纠正。
- `fix.patch`、`source-lock-before.json`、`source-lock-after.json`：交付与身份。
- `retirement-commit.txt`：真实退役依据。
- `red-*`、`attempt1-*`、`previous-gnu-*`：未覆盖写的失败及旧验证。
- `verify.py`、`green-*`、`final-wrapper.*`、`verification-receipt.json`：最终真实并行与监督证据。
