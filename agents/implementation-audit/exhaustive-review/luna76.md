# luna76：三篇发布/公司报告记录全文复核

## 范围、全文与基线

- 产品基线：`08e4fc7`（合并等价）；当前 worktree HEAD `a7c7ce3`。目标记录均非该产品提交改动；仅核验已有工作记录对照当前政策和调用，不修改产品、Git，不跑测试、构建或长验收。
- 连续读取到 EOF：`agents/oop-release-validation/build-only-review.md` 29 行、`company-report-selection-fix.md` 48 行、`publish-tag-sha-tdd.md` 35 行，共 112 行。下表逐行段覆盖每份记录全部内容，标题/空行/代码块及证据段也包含在行区间中。
- 另读当前根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、ADR-0027、ADR-0028、交易规则基线；当前发布规则以 ADR-0028 2026-10-03 accepted 决定为准。A股规则没有被本次历史记录或当前修复改变。
- 本核验只承接相关发布入口、`CompanyPanel` 选择行为及 G27；不把结果外推到 Release 全模块、公司信息全模块或交易语义全量复核。

## 行数与章节矩阵

### `build-only-review.md`（29 行）

| 原文行 | 原文主张/原始证据 | 当前政策、调用和复核 |
|---|---|---|
| 1–6 | 标题、日期/基线、复核人、用户取消发布链 CI、只保留 build release。 | ADR-0028:3、15–18 明确发布仅构建；旧 CI 重跑取消是历史事实，不是现行待办。`.github/workflows/release.yml` 是标签发布编排入口，`ci.yml` 为手动开发诊断。 |
| 8–12 | 写明方法：审全 diff、列 workflow/契约/文档；检查未改发布及打包 caller；未改产品、未发布、未跑完整回归。 | `scripts/release-policy.mjs` → `release.yml` → `distributions.yml` → `scripts/publish-release.mjs` → Pages 是本记录所涉链。历史“未发布”不等于当前发布授权；此处产品入口范围没有 engine 或财务计算。 |
| 14–19 | 独立结论三项：A股语义未触及；按用户新要求移除 CI/test/smoke、保留生产构建/制品检查；保留矩阵 DAG、权限、cache。 | ADR-0028:15–24 是现行约束：生产 TS、工具版本和制品完整性仍需；不运行套件、lint、Clippy、浏览器/原生 smoke。旧结论与政策一致。没有交易规则需要新查证。 |
| 20 | Release collector、固定 `github.sha`、两次 tag SHA、asset inventory/digest、draft 等保护未改。 | 当前 `publish-release.mjs:79–113` 可直接确认执行顺序；`release.yml` 固定 validate 输出 SHA 供后续构建/发布。G27 的特定 tag 守卫在本文后部单独核销，不据此替整个 collector 背书。 |
| 22–27 | 列短测 20/20、red 日志只有摘要、PyYAML BaseLoader / needs 检查、无 actionlint，以及人工检查的 collector 格式边界。 | 这些是历史证据，不是本轮实跑。历史记录自身诚实指出 red 根因详情不足、YAML 解析非 Actions 语义证明、collector 不独立固定每平台安装器扩展名。当前无证据允许扩大为“真实三平台发布已验证”。 |
| 29 | 真实构建、Release、Pages 与线上可玩须看实际 Actions 结果；本记录不替代。 | 仍成立；本轮未查 Actions、GitHub Release 或 Pages。 |

### `company-report-selection-fix.md`（48 行）

| 原文行 | 原文主张/原始证据 | 当前生产调用、复核 |
|---|---|---|
| 1–3 | 记录注明日期和非正式规范性质。 | 本记录是当批排查证据，不创建报告披露/会计规则。公开报告可见性仍由现有 DTO、host 与 engine 决定。 |
| 5–9 | 原 E2E 12 场景 11 通过；trace 证明按钮实际被选中后回到另一份报告；拒绝将现象误判为点击未触发。 | `CompanyPanel.tsx:47–51` 将用户选择作为组件状态，并由 `DisclosureList` 的 `onSelect` 驱动；旧 trace 属历史事实。当前不重跑浏览器，不能将其写成当前 fresh E2E 结果。 |
| 11–15 | `acceptCivil` 刷新公司查询，root page loading 令 `visibleReports` 暂时为空，旧 effect 因临时空列表清空选择。 | 当前调用仍允许自然日变更引发刷新，`CompanyPanel.tsx:43–46` 读取当前 page/view；不得靠停掉刷新“修复”。当前 effect 在 59–62 行显式根据 view state 限制协调。 |
| 17–20 | 修复只在 ready/empty 协调；loading/error/unavailable/idle 保留选择；真实 empty、失效编号回退；E2E 改真实按钮/pressed/键盘。 | 当前 `CompanyPanel.tsx:59–62` 只有 `ready`/`empty` 会调用 `setSelectedReportId`；50 行 `selectVisibleReportId` 仍让已失效 ID 回到可见报告；86–89 行仅 ready 时显示报告列表。选择暂留期间，展示 ID 由可见报告集合重新派生，不会展示不可见报告。 |
| 22–24 | `memoryHook` 模拟 state 和 commit 后 effect；覆盖加载/错误保留、真 empty 回退、切公司失效 ID；不声称 DOM 测试，不改跨公司同字符串 ID 的规则。 | `company-report-selection.test.ts:26–64,67–93` 仍对应这三个组件逻辑边界；90–91 行新公司财务 scope 与公司 ID 对齐。它是组件逻辑测试，不是 DOM/E2E。 |
| 26–48 | 验证证据：red 的预期/实际 ID；13 个定向 case；TSC 首次失败后显式泛型修复；复用 preview 的 12 passed 被排除；fresh build 及 E2E 通过；无 A股/财务/披露语义改变；复核发现 fixture scope 错并已修。 | 以上均为历史运行/修复证据，不能声称本轮重跑。scope 反例已在当前 fixture 修正。公开报告仍仅为截至模拟日已公开事实；本修复没有改变公布时点、期间、内容或权限。 |

### `publish-tag-sha-tdd.md`（35 行）

| 原文行 | 原文主张/原始证据 | 当前生产调用、复核 |
|---|---|---|
| 1–3 | 日期、范围仅 `publish-release.mjs` 与测试；未提交或真实发布。 | 当前生产发布入口仍为 `scripts/publish-release.mjs::main`；ADR-0028 要求发布器核验标签移动。记录不授予远端发布权限。 |
| 5–9 | 在 draft 资产上传/核验完成后、公开前再查询 tag commit；变化则报错保留 draft；查询失败传播；不是原子 tag 锁。 | 当前源码 109 行第二次查询，110–112 行比较/抛错，113 行才 `release edit --draft=false`。API 错误未被 catch，调用栈至入口错误处理并以非零退出。两请求之间仍非原子操作。 |
| 11–21 | Red：先加入上传期间 tag 移动测试；首次返回固定 SHA、第二次返回新 SHA；旧实现未拒绝且实际走公开，`Missing expected rejection`。 | `publish-release.test.mjs:144–168` 仍固定上传/远端资产校验完成后二次 query 变化时无 `edit`，但本轮不执行。日志证明历史 red 的具体失败路径，不作为当前运行结果。 |
| 23–32 | Green：4 行生产核验；成功路径精确断言第二次 query 在 edit 前；15/15 三文件测试、10s deadlines、多 worker、无 GitHub 调用。 | 当前源码保留必要顺序和错误路径。历史测试证据不替代当前 test；当前没有真实 GitHub 调用。 |
| 33–35 | diff check 通过；独立复核另由 root 安排；记录不替代该门禁。 | 独立检查材料可见 `sweep77.md` 对生产行 109–113 及 G27 的历史复核；本记录也不等于本轮完整 Release diff 审查。 |

## G27 核销与边界

G27 的问题是 draft 上传及远端资产核验期间 tag 可移动，而发布器只有编译后/进入流程时的一次 SHA 校验。当前 `publish-release.mjs` 的相关代码原文为：

```js
const publicationSha = await invoke(["api", `repos/${env.GITHUB_REPOSITORY}/commits/${tag}`, "--jq", ".sha"]);
if (publicationSha !== env.RELEASE_SHA) {
  throw new Error(`公开 Release 前发现 tag ${tag} 的提交发生变化（期望 ${env.RELEASE_SHA}，实际 ${publicationSha}）；保留 draft，请核对 tag 后重新发布。`);
}
await invoke(["release", "edit", tag, "--repo", env.GITHUB_REPOSITORY, "--draft=false", ...(prerelease ? ["--latest=false"] : [])]);
```

前置核 SHA 在 83–84 行；draft 创建与远端身份/资产核验在 86–107 行；上引第二次查询确实位于核验之后、公开命令之前。匹配才执行公开；SHA 不匹配会抛错，查询拒绝同样因 await 拒绝传播，不会到达 `edit`。对应测试固定 tag 移动路径并断言 `calls.some(args => args[1] === "edit") === false`（测试 155–168 行）。因此 **G27 已修复并核销**；旧结论维持。

核销严格限定于“上传期间已发生且第二次读取时可见的 tag 变化”。第二次 API 查询与后续公开 API 调用仍是两个请求，窗口不是原子锁。既不能以这个窗口重开原 G27，也不能声称 tag 在最后查询之后永远不可移动。

## CompanyPanel 核销与反证

当前核心逻辑原文为 `CompanyPanel.tsx:59–62`：

```tsx
useEffect(() => {
  if (state.kind !== "ready" && state.kind !== "empty") return;
  setSelectedReportId((current) => selectVisibleReportId(current, reports.map((item) => item.id)));
}, [reports, state.kind]);
```

对照旧问题链：`loading` 时 effect 提前返回，用户选择状态不被空列表清掉；恢复 `ready` 后选择仍在可见 ID 集合中则保留；若此时确实已不在集合，`selectVisibleReportId` 回退；真实 `empty` 仍运行协调，可清除选择。当前组件代码及三个逻辑边界测试支持旧故障已由产品代码修复，不只是测试变绿。报告 ID、公司查询归属、披露 DTO 和金额计算不在本次选择修复核销范围。

## 候选、反证与终结论

- “build-only 遗漏必需 CI/lint/smoke”：ADR-0028 明确取消发布及手动产品构建的这些步骤，生产编译/制品核验保留；不是当前缺陷。
- “G27 只有测试没有真实生产接线”：当前 `publish-release.mjs:109–113` 在真实入口执行二次核验；发布 workflow 固定并传递 `RELEASE_SHA`。排除。
- “二次 SHA 查询提供原子锁”：调用是两个独立 API 操作；排除过度主张，保留竞态边界。
- “CompanyPanel 仍会因刷新 loading 无条件丢选”：effect 的 ready/empty guard 构成直接反证；真实 empty/失效 ID 仍回退是既定行为，不视作回归。
- “旧 preview 的 12 passed 证明线上/当前全通过”：原记录明确排除复用 preview 并单列 fresh 构建结果；本轮无重新运行或线上证据，不作此主张。
- “collector 不独立固定全部安装器组合即发布链未实现”：原复核已限定该边界；本任务未发现足够依据扩大成新的必需修复，亦不替 collector/完整发行链作全面验收。

结论：三篇记录 EOF 内容与当前有效政策没有冲突；G27 和 CompanyPanel 刷新选择问题均有现行生产守卫，可维持已修/核销状态。未发现本限定范围内需要新增的有效候选。未运行测试、构建或线上检查；不据此宣称三平台实际发行、Pages 状态、整个公司信息模块或整批改动门禁完成。
