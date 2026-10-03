# 发布仅构建改动记录（2026-10-03）

用户要求去掉 CI 检查，只保留 build release。本批只修改工作流、对应契约测试和
直接相关正式文档，不改产品逻辑，不删除测试源码，不提交或推送。

`release.yml` 在标签与源码 SHA 校验成功后直接调用分发构建，移除 CI job 及其
依赖。`distributions.yml` 移除构建契约测试、Pages 浏览器 smoke、Playwright 安装、
原生服务 smoke 与清理前的契约测试。生产 TypeScript 编译、固定工具版本检查、
三平台九组原生制品与静态 Web、打包、十组 manifest/大小/SHA-256 核验、公开前
资产核验、Release、Pages 和安全缓存清理继续保留。

`ci.yml` 移除 `workflow_call`，保留 `workflow_dispatch` 及原有开发诊断测试。
`build-web.yml` 原来仅包含构建复用和 Pages 部署，无需修改。

正式文档同步修改 `docs/build-and-deployment.md`、ADR-0027/0028、
`docs/actions-cache.md`、`docs/testing.md` 的对应段落，区分当前发布行为和既有
历史 CI 验收证据。大 A 交易规则、单位、存档与运行时语义未改变。

## 验证

- 先更新三个 workflow 契约测试再修改工作流，初次旧实现运行为三文件失败，日志
  `build-only-red.log` 仅保存文件级结果，不以其摘要声称具体 case 失败原因。
- 再以临时目录复制 HEAD 原始 workflow 和更新后的需求契约，用代表性筛选独立
  复核旧实现：五项需求差异明确失败、一项未改变的手动 CI 契约通过，详情见
  `build-only-baseline-red.log`；整个命令受 10000ms 外部期限约束，临时目录已清理。
- 当前实现初次契约验证 20/20，通过详情见 `build-only-green.log`。
- 最终相关短测使用四个真实进程并行，各自设置 `--test-timeout=10000` 和
  `run-with-deadline.mjs 10000`。本机 Node v25.8.2，128 CPU；每个进程使用
  `--test-isolation=none --test-concurrency=1`，文件组之间进程隔离，不改变 workflow
  内手动 CI 的测试隔离配置。四组共 59/59，通过详情分别见
  `build-only-policy.log`（5）、`build-only-distribution.log`（9）、
  `build-only-cache.log`（26）、`build-only-publish-build.log`（19）。
- root 用 `/usr/bin/python3` 的 PyYAML BaseLoader 解析全部 workflow 并校验 job
  `needs` 引用，exit 0，见 `build-only-yaml.log`。这是 YAML 语法和引用检查，
  不称为 actionlint 全绿；本轮没有运行 actionlint。
- `git diff --check` 通过；未运行完整回归，也未执行三平台实际构建、发布或线上 smoke。
- 未参与实现的 subagent 已独立复核完整 diff，无阻断发现，详见
  [build-only-review.md](build-only-review.md)。

初次用于补充旧实现详情的 Node `execFileSync git` 在 sandbox 中报告 EPERM，
不作为测试证据；改用受相同外部期限约束的 Python 子进程完成上述旧实现复核。
