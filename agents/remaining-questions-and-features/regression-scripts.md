# scripts 回归修复

## 根因与范围

`test:scripts` 的独立入口原样继承宿主 `TMPDIR`，本机该值在工作区之外。性能验证测试因此正确触发源码与输出 containment 拒绝；完整回归入口已规范化临时环境，独立入口却没有。现统一复用 `prepareWorkspacePaths`，将 child 的 `TMPDIR`、`TMP`、`TEMP` 指向 workspace `.tmp/process-tmp/script-tests`，保留其他环境，继续禁止 symbolic link 与工作区外输出。规范化在已受外部 300000ms supervisor 监督的 internal worker 内完成，目录准备也计入共享批次上限；外层仅派 child，不在监督前准备目录。未修改交易、资金、存档、兼容或 deadline 语义。

## 证据

- 新增独立入口环境短测，真实红为环境未传递／未规范化；修复后 runner 8 个 case 通过，约 0.54 秒。测试按 `resolveWorkspaceRoot` 核对路径，兼顾 linked worktree 的共同工作区。
- 主 agent 复核发现初版目录准备位于外部监督前，已移至 internal worker；新增断言保留完整规范化检查，同时验证外层先派监督 child、不修改原始环境。最终增量 runner 8 个 case 通过，约 0.54 秒；下述全 scripts 结果来自该监督位置增量之前，最终全量结果由主 agent 后续验收。
- `wasm-build-dependencies.test.mjs` 的早先 5000ms Cargo tree 超时经精确提升复验 3 个 case 全通过，整文件 6.30 秒；沙箱另可返回 `spawnSync EPERM`，不是 Cargo 构建错误。未提高任何 timeout 或替换真实依赖检查。
- 首次全 scripts 重试在 verifier 的两个 resume 用例遇到当前源码指纹漂移，后续精确 verifier 18 个 case 全通过（2.90 秒）；未弱化 receipt／来源检查。源码仍被其他任务编辑时不应把 source-bound 证据拒绝误报为业务失败。
- 最终 `node scripts/run-script-tests.mjs` 成功退出，完整递归 34 个文件均完成，4 个 file worker 并行，共 8397ms；每文件和 case 均保留 10000ms 门禁，聚合外部监督仍为 300000ms。完整日志：`.tmp/main-regression-2026-10-05/script-tests-final-second.log`。
- 仅运行 scripts 聚合与精确短测，未运行 Cargo 构建或完整回归，未 commit；最终完整 diff 的非作者独立复核见下文。

## 非作者独立复核

`fix_regression_civil` 未实施本项，已完整读取两个脚本的 diff、完整 runner/test 源码及 `workspace-paths.mjs`。复核通过：

- 改动不触及 A 股领域语义；将独立脚本入口的临时环境与既有 workspace containment 对齐，范围必要。
- `prepareWorkspacePaths` 根据主仓库或 linked worktree 的 Git 元数据解析共同工作区，逐层拒绝 symbolic link、非目录、路径越界及不可写目录。没有解除性能工具的输出 containment，也没有在工作区外创建产物。
- `env` 明确向 supervisor 和 internal worker 传递，只有 `TMPDIR`、`TMP`、`TEMP` 及受控内部标记被覆盖；其他环境保留。普通文件与 case 仍为 10000ms，最多四个 file worker；独立聚合仍由进程外 300000ms supervisor 监督，没有删除测试或放宽断言。
- 首轮独立运行环境 case：1/1 通过，case timeout 与外部 deadline 均 10000ms，命令约 0.18 秒；`git diff --check` 通过。已核对全 scripts 日志中的34文件全部完成、8397ms与deadline参数，不冒称本人重跑了整批或 Cargo 完整回归。
- 最终增量完整 diff 复核通过：`runScriptTests` 不再在监督前准备路径，只启动带 300000ms 外部监督的 internal worker；`main` 在监督范围内调用 `prepareScriptTestEnvironment`，将规范环境传给批次。新增断言覆盖外层环境未提前改写、原 `env` 未被修改及 worker 规范化后的全部环境字段；原期限、发现与失败清理断言保留。独立复跑该增量 case 1/1 通过，case 与外部 deadline 均 10000ms，命令约 0.18 秒；未重跑 scripts 整批或 Cargo 完整回归。
