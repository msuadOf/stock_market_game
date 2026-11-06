# escrow harness 现行合同修复

## 范围与合同依据

- 独占 worktree：`.worktree/escrow-harness-contract`，detached `5bbcab37`；从集成树仅复制 4 个必要脚本共 76547 bytes，不复制 Cargo/node cache，不写集成树或正式 cache。
- 需求是修复 full12 首个 scripts 失败；只改 `scripts/simulation/escrow-performance-harness.test.mjs`。以 `fix.patch` 为相对集成树的完整交付 diff，不以此独占树相对旧 HEAD 的 Git diff 作为交付包。
- `f86684ad615132d89afb8a29496ec2698ce878c5` 明确删除 environment 的 `rayon_threads`、样本报告的 `rayon_registry_capacity_samples` 以及基于 pool size 的比较；原始提交 diff 见 `retirement-commit.txt`。
- 当前 `validateEnvironmentContract` 只接受 cargo/rustc/target/rustflags/cargo_jobs 五键；`parseSampleStdout` 仍要求当前 sample 的 `runnable_samples` 为 null 或非空非负整数数组，after 必须非 null。当前 `REQUIRED_PHASES` 为原 fixture 的 10 项，无需修改。
- ADR-0043 记录 std::thread 方向、CPU 限制属于 OS 而非 pool width；ADR-0029 要求生产者、exact-key validator 与 fixture 同步。没有改变交易制度、游戏实现或 A 股领域语义。
- 生产性能端点的准备链当前显式标为退役；本修复不声称恢复正式 escrow 性能测量能力。

## 旧断言承接

1. config fixture 删除退役 rayon_threads；新增带该旧键应被 exactKeys 拒收、现行 config 正向通过。
2. 原 wrongCapacity=[1,1] 的拒收要求已被上述提交明确删除，替换为现行 []/负数/非整数拒收，以及 [1,1] 正向接受。
3. 旧 report capacity 数组输出改为 own-field 不存在断言；额外携带 retired capacity 的 stdout 仍被 sample exactKeys 拒收。
4. 增补真实 OS thread summary 的 runnable>total 拒收。原 histogram、phase、ticks、source、toolchain、setup、repetitions、配对成功、报告复用、防污染、idempotent 写入与 child 清理断言全部保留，regex 未放宽。
5. fixture 报告目录由本 workspace 的独占 mkdtempSync 根派生；不继承可能指向其他 worktree 的 TMPDIR，生产 workspace 边界保持不变。每分片 fixture 随机根互不干扰，after 清理。

## 红绿与失败保留

- `red.stdout`/`red.stderr`/`red-result.json`：原文件完整 20 case，10 pass / 10 fail，exit 1，866.391ms。错误包括 environment exactKeys 和 retired wrongCapacity 的 Missing expected exception；没有将前置失败宣称成具体业务断言已覆盖。
- `attempt1-*`：初次并行试验把两个 shard 的 cwd 放到不同深层目录，19/20 pass；原 outsideCheckout 因 cwd 改变实际仍在 workspace 内，故因 does not exist 而非 outside 拒收。这是试验配置错误，完整保留；没有改该断言或 regex。
- 前次 `previous-gnu-green-harness.stdout` + `previous-gnu-green-lifecycle.stdout`：互斥 name pattern 覆盖全文件 20 case，12+8 pass，0 fail/skip。两个独立 Node 进程从 worktree 根同时启动，fixture 随机根隔离；共 606.926ms，两 shard exit 0。
- 各 Node case 10000ms；各 child 使用 run-with-deadline.mjs 10000ms 进程树 supervisor；前次总验证使用外层 GNU timeout 10s（kill-after=0.1s），因新 session 不足以证明共享进程树总时限；该口径由下面补跑证据更正。驱动器自身在 9s 后开始终止/清理，均未触限。前次命令：`timeout --signal=TERM --kill-after=0.1s 10s python3 agents/escrow-harness-contract/verify.py`。
- 可用 CPU=128，真实并行 shard=2；previous-gnu-green-result.json 保留每 10ms 的 /proc 进程树、线程数与 CPU tick，两个分片重叠采样数为 43。只报告真实进程重叠，不把 --test-concurrency 当作多核证据。
- 最终 TMPDIR 故意指向主树 `.tmp/escrow-harness-contract-external`，与测试 worktree 不同；helper 不访问/创建该外部目录。最终 fixture_residue=[]；原 child 清理 case 均通过。

## source identity 与交付

- `source-lock-before.json` 记录从集成树复制的精确 SHA；`source-lock-after.json` 显示集成树 4 文件均未变、3 个生产依赖与复制源逐字节相同。
- 最终 test SHA-256：`5b2794905c3a8746720344532291d6607916faa708d75102d293bfedacf7eabe`。
- 未运行 full 或其他 scripts 全集，未提交、push；本包交 Sol 对完整 fix.patch 与原始证据独立终审后才可宣称完成。

## 共享进程树监督补跑

- 前次 GNU timeout 的所有 green 产物和 receipt 已原样改名 `previous-gnu-*`，不追认为共享进程树 10000ms 证明。
- 最终正式补跑命令：`node scripts/run-with-deadline.mjs 10000 -- python3 agents/escrow-harness-contract/verify.py`；外层项目进程树 supervisor、每 shard 的项目 supervisor 与 case timeout 都为 10000ms。未修改 verify.py、测试源码或 patch。
- 补跑外层 exit 0；driver 整批 570.775ms；两分片分别 exit 0，12+8 case 全绿、无 skip；最新原件为 green-*.stdout/stderr、green-result.json、final-wrapper.stdout/stderr、verification-receipt.json。
- 补跑采到的全部进程 PID 已消失，fixture_residue=[]。工具原始 wall 与 driver wall 分别记录，未混称。
