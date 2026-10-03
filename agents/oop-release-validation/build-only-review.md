# 发布仅构建的独立复核

- 复核日期：2026-10-03。
- 基线：`0e64ae7458ae1ba0fffa05de58f02faf443e5536`。
- 复核人：未参与实施的 `build_release_review` subagent。
- 用户最新范围：取消发布链路的 CI 检查，仅保留 build release；原 run 第二次 Windows 定向重跑已经取消。

## 范围与方法

已读取 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md` 和 ADR-0027/0028。相对基线逐项审查全部 tracked diff：三个 workflow、三个契约测试、五份正式文档、root 的发布 summary，以及用户预先添加的中文注释/文档规范。`agents/oop-refactor-audit/` 是既有另一主题的 untracked 工作目录，不属于本轮实现。

同时沿调用链检查未修改的 `release-policy.mjs`、`publish-release.mjs`、`package-distributions.mjs`、`package-static-web.mjs`、`frontend-build.mjs`、`build-targets.mjs` 和各手动产品入口。未修改产品文件，未提交，未运行完整回归，也未触发远端发布。

## 独立结论

1. **大 A 语义符合现有基线。** diff 没有涉及 engine、宿主协议、UI、存档或领域测试，不改变金额分/股单位、T+1、实际受理顺序、策略和日终存档语义；本轮不实施交易制度，无需新增交易所规则依据。正式文档继续明确这些语义不变。
2. **改动为用户新需求所必需，范围保持最小。** 删除 release 的 CI job 和依赖；分发移除契约测试、Pages/native smoke 与 Playwright 浏览器安装；CI 改为仅 `workflow_dispatch`。工具版本、生产 TypeScript/WASM 编译与制品完整性检查保留。契约断言针对明确变化更新，同时新增发布 job 列表、依赖、SHA 传递和无 CI/测试的断言；未删除或弱化游戏产品断言。
3. **未发现本批新增的边界遗漏、跨层语义漂移或不必要复杂度。** DAG 为 `validate → distributions(all) → publish → pages`；缓存清理等待 distributions/publish/pages，成功公开 Release 才退休标签缓存，失败保留编译进度。独立 Server 矩阵仍不依赖 frontend；其余产品复用同 run 的前端。十组产品/平台、只读构建权限、发布 `contents: write`、Pages OIDC 权限以及独立清理权限均完整保留。

Release 继续下载默认当前 run 的 `stock-market-game-*` artifacts，不合并目录；`github.sha` 固定来源、静态 manifest commit 校验、发布前两次 tag SHA 查询、各文件数量/大小/SHA-256、符号链接/重名拒绝、draft 上传后远端 asset inventory/digest 核验均未修改。Pages 复用同轮上传的站点，并继续在发布成功后部署。未发现阻断性或需修复的有效发现。

## 证据与适用边界

- 已核实实施者日志 `build-only-green.log`：三份修改契约测试合计 20/20 case 通过，约 54ms；该结果不表示游戏回归或三平台构建通过。
- `build-only-red.log` 保存实施前的三个文件级失败摘要，但没有具体断言详情；仅凭该日志不能独立确认每项预期失败原因，已经通知实施者补充说明。
- root 的 `build-only-yaml.log` 使用系统 Python/PyYAML `BaseLoader` 解析全部 workflow，并检查每项 `needs` 引用存在，exit 0。未使用 actionlint；该解析不等于 Actions 全部语义验证。
- 本复核执行 `git diff --check`，exit 0；DAG、reusable workflow 输入、权限和 artifact 来源通过人工独立检查。
- 既有 Release collector 要求十组产品/平台以及 manifest 所列文件完整性，但不独立固定每平台安装器扩展名组合；对应格式要求仍由未修改的打包入口验证。本轮没有扩大或削弱这一既有边界，也不宣称 collector 独立强制固定总资产数。
- 新 build-only 流水线的三平台真实编译、公开 Release、Pages 部署与线上可玩情况须分别报告真实 Actions 结果；本记录不将 YAML、短测或旧 CI 结果替代该证据。
