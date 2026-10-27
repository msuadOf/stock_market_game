# Exempt 新局印花税校验闭合（2026-10-25）

## 范围与因果

基线 `b172c1ab`。`SessionSetup::validate` 先调用 `validate_company_fields`，按 ADR-0040 接受 `Exempt + stamp_tax_rate=0`，但随后旧的无条件 `fault1013` 要求税率 `0.0005`，使合法 Exempt 新局被拒绝。若 Exempt 携带 `0.0005`，又会被前面的 `fault1055` 拒绝。

修复仅使旧的 `fault1013` 适用于无公司系统的局；公司局保留既有 `fault1055` 单入口模式配对。不会免除佣金、过户费，也不会支持新的真实税籍、修改费用管线或兼容旧档。

## 依据

已读 `ROLE.md`、`AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`、`docs/git/AGENTS.md`、`docs/git/daily-workflow.md`、ADR-0039 与 ADR-0040。

ADR-0040（2026-10-08 批准）明确：`FlatWithholding` / `AShareIndividual` 要求 `0.0005`；`Exempt` 要求 `0`，佣金与过户费照付。`Exempt` 是游戏模式，不是真实 A 股普遍免税制度。

2026-10-25 实际读取国家税务总局官方公告：<https://fgk.chinatax.gov.cn/zcfgk/c102416/c5211343/content.html>。公告成文日期 2023-08-27，自 2023-08-28 起证券交易印花税减半征收。原始响应保存为 `official-stamp-tax-2026-10-25.html`。本次不变更现行基线与其他税费。

## 验证安排

机器 `nproc=128`；独立 target 使用仓库根 `.tmp/exempt-setup-target`。构建与测试分离；构建显式 `--jobs 16`，`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`，由进程外 `run-long-validation.mjs 300000` 监督。编译日志原件保存，不把编译时间计作普通测试耗时。

普通测试直接运行预构建二进制，整命令受 `run-with-deadline.mjs 10000` 监督；新测试组用 `--test-threads=8`。公司 foundation 套件与既有 A 股基线校验 case 逐 case 由进程外十秒监督、最多 8 个进程并发，避免长 case 藏在整组宽限中。

## 实际结果与交接

- 红：新增 5 个 case，4 绿、Exempt 合法零印花税 case 以 `InvalidSetup(code=1013, context=[0])` 失败；harness 0.63s，进程外十秒门禁。见 `red-test.log`。
- 绿：同一组 5 个 case 全绿，harness 1.24s。见 `green-test.log`。
- 回归：完整 `session_company_foundation` 21 个 case 与既有 `formal_session_setup_enforces_a_share_baseline_and_category_limits` 1 个 case，合计 22/22 通过。8 个独立进程并发，每 case 进程外 10000ms deadline，批次 3.162s，最慢 case wall 2.564s。见 `foundation-list.log`、`green-summary.json` 与逐 case 原始日志。
- 冷构建 16 jobs，实际编译日志见 `red-build.log`；实现后构建 22.06s，最终只调整新测试排版后重编 1.73s，见 `green-build.log` / `final-build.log`。已有警告保留，未顺手修复。运行中核对 rustc 多线程（资源快照 `build-resource-snapshot.txt`）。
- `git diff --check` 通过。未运行 workspace 全量验收、未 merge、未 push；本地提交后交 Leader 安排未实施者完整 diff 独立复核，不据本记录宣称完成门禁。
- 为避免重复编译警告淹没改动，仓库忽略 `.log` 与 `.json` 工作产物；提交只包含代码、正式边界文档及本 README。完整原始构建日志、红绿组日志、case 清单与汇总、逐 case 日志、源码与二进制 SHA-256 绑定及官方网页响应保留于同一独占 worktree 的本目录，供只读复核。

## 独立复核后的精确零修订

首轮冻结 `669b2948` 未通过独立复核：`sol61_review_exempt` 在真实 rlib 上发现 `Exempt + 1e-16` 与 `Exempt + f64::EPSILON` 均可构造并保存恢复。合法 `10^18` 分金额通过同一 `GameConfig::stamp_tax` 会产生 100 分或 222 分费用，违反 ADR-0040 的免税承诺。原因是既有公司模式配对使用浮点绝对误差，而首轮 guard 修复首次放行了该接受域。独立复现原件保留在主工作区 `agents/branch-completion/exempt-setup-review-2026-10-25/`，首轮日志未覆盖或回填。

修订只令 `Exempt` 在原 `fault1055` 单入口精确比较税率与零；`FlatWithholding` / `AShareIndividual` 保留原有 `0.0005` 与 `EPSILON` 比较，不修改其他费用。新增两 case 分别验证开局校验/构造拒绝与真实 save 经 JSON 篡改后 restore 拒绝；覆盖最小正 subnormal、`1e-16`、`f64::EPSILON`。开局拒绝仍要求 `1055` 和 `[期望率位模式, 实际率位模式]`；恢复沿用现有 `2002` 包装，未扩大错误协议范围。合法零费率恢复另加合法大额金额费用为零断言。

实际第二轮证据在独占树本目录 `epsilon-revision/`：

- `red-test.log`：同一税率模块 7 个 case 中 5 绿、2 红，分别真实观察到非法极小率校验通过、被篡改存档恢复成功，harness 1.46s；`red-binding.json` 是红测运行结束后、任何实现修改前保存的源码和产物哈希，不声称是运行前 manifest。
- 构建显式 16 jobs、进程外共享 300000ms deadline；红测试构建 4.20s、实现后构建 19.85s。日志分别 `red-build.log` / `green-build.log`。构建后才进行的资源查询已无活跃 rustc，未用这次查询声称观测到编译 CPU 占用。
- `green-before-run-binding.json` 于最终测试前记录源码与两个 test binaries 的 SHA-256；`green-after-run-binding.json` 于测试结束后再次核对，全部一致；两个 manifest 使用新文件名，不覆盖首轮绑定。
- `green-test.log`：7/7 通过，harness 1.27s，整组进程外十秒门禁，8 harness threads。
- `green-summary.json` 及逐 case 原件：foundation 完整 23 case + 既有 A 股基线 case 共 24/24 通过；8 进程并发，单 case 进程外 10000ms deadline，批次 3.207s、最慢 case wall 2.597s。
- `git diff --check` 通过；新冻结提交继续交同一未实施 reviewer 复核完整最终 diff，不将首轮绿测当作批准。
