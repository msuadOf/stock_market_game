# luna79：OOP 发布验收总结 EOF 全文复核

日期：2026-10-03。基线产品 `08e4fc7`，与产品 `b76ece3` / `2247f4f` 同树；工作树合并提交为 `a7c7ce3`。目标 `agents/oop-release-validation/summary.md` 共 **41 行**，从头到尾逐行阅读；遵循根 `AGENTS.md` 与 `docs/principles.md`。本轮只读文件和当前 caller，未运行测试、长验收、网络请求或 Git 写操作；唯一新增本审计记录。旧逐章复核 `sweep77.md`、`sweep78.md`、`sweep81.md` 用于找反证，不照抄其结论，重点以当前树调用链与较新 `agents/main-release-validation/summary.md` 的产品验收决定更新适用范围。

## 逐章矩阵

| 原文行/章节 | 原文主张与旧结论复核 | 当前证据及新判断 |
|---|---|---|
| 1–3 标题、任务范围、基线、排除文件 | 描述原 OOP 发布验收的工作分支和提交边界；历史基线声明不是本轮产品变更清单。 | 产品/工具目标树为指定 `08e4fc7`；summary 的旧基线只解释当时验收，不可拿来推导当前提交差异。本次任务只新增审计账，不触碰产品。 |
| 5–7 `.gitignore` | 报告称根 `.worktree/`、`.worktrees/` 与 agent JSON 规则已加入；旧结论将其作为当时实现事实。 | 该段属于历史实现记录；当前独立审计主题是发布验收与后续决定，不据历史记录重跑 gitignore 行为或声称其为新改动。当前工作目录本身位于 `.worktree/implementation-reaudit`。 |
| 9–17 宿主/存档、Rust cleanup、撮合 fixture、CompanyPanel、发布脚本 | 旧逐章复核已确认 CompanyPanel ready/empty guard 与 G27 发布前 SHA 查询确有生产代码，未停留在测试；这里重新沿 caller 核实，并核对曾被旧 review 指出的 fixture/语义风险。 | `CompanyPanel.tsx` 当前仅在报告状态 `ready` 或 `empty` 时协调选择，loading/error/unavailable 保留 state；ready 才渲染 disclosure 和公开 DTO。故自然日刷新 loading 暂空导致选择丢失的旧问题已修，不列新候选。撮合段陈述的是受理顺序 fixture 修正，不是生产撮合规则变更；不据此重开交易规则缺陷。G27 当前 `publish-release.mjs` 先查冻结 tag SHA、收集/上传为 draft、校验远端 draft 身份与资产，然后再次读 tag SHA；不匹配明确失败，最后才取消 draft。两个 API 请求与公开操作不是原子锁，旧结论“覆盖需求但保留极窄竞态”仍准确。 |
| 19–29 验证与证据 | 当次长回归、scripts 32 文件/370 case、fresh E2E、构建、发布门禁及本地日志均是作者报告的历史执行证据。旧复核指出日志为本机忽略产物、runner 能力不等于本轮实跑。 | 按原文限定为该次运行结果，不将历史 green 改写成本轮执行或远端当前结果。summary:25 的每文件及每 case 10000ms 是 scripts runner 的历史结果，且整批 13.675s 明确属于被 300000ms supervisor 覆盖的长验收；不能移植成 Rust case 监督已具备。summary:27 的 2193 Rust、608 Web 和 fresh 12/12 E2E 也仅适用于所记源码和流程。较新的 main summary 是另一验收批次，不能把两份计数叠加成当前验证。 |
| 31–39 发布与适用边界：首次 Windows CI、用户改为 build-only、Release/Pages | 旧逐章结论认为 build-only 是用户明确决定，CI 留作手动开发诊断，不把发布不测当缺陷；重新核对当前 workflow 和 ADR。 | `release.yml` 当前依次 validate → reusable `distributions` → publish → Pages → cache cleanup；publish 用 `run-long-validation.mjs 300000` 监督制品收集/远端核验。`ADR-0028:15–24,33–35,88–91` 明确发布不调用 CI/test/lint/Clippy/E2E/smoke，生产 TS 编译和制品核验仍属构建要求；手动 CI 是诊断入口。因此原 Windows CI 失败/取消属于旧发布方案的历史，不要求恢复 CI。tag/Actions/35 资产/Pages 数字是文档作者记录；本次不联网验证。Pages 成功仍不等于公网游戏完整验收。G27 双 SHA 实现适用于当前发布链，不能扩张成无并发窗口保证。 |
| 41 末行及全文 EOF：未覆盖项、Rust case 时限、A 股语义声明 | 原文明确未覆盖 ignored scale/stress/cost、完整 K7、原生 GUI 安装、签名/公证，并坦承普通 Rust case 无独立 10 秒 watchdog；旧 sweep78/sweep81 将最后一项作为仍开放的工具缺口。 | 未覆盖项是范围界定，不能从 Release/build 成功推导通过；当前 ADR-0028 进一步明确 unsigned、GUI 安装/签名公证仍不在发布链。A 股规则、金额分/股、T+1、受理顺序和日终存档在本轮为未改变声明；当前改动主题没有引入交易制度主张，无需重新外查规则。Rust case 候选经现行 caller 复核后仍成立，见下节。 |

## 最新 caller 与候选反证

### L79-1：普通 Rust case 没有独立 10000ms watchdog

当前 `scripts/run-full-regression.mjs:328–363` 的 `executeRustTestBinaries` 为每个预构建 binary 计算共享阶段剩余 `timeoutMs`，并只传 harness 并发参数与 Rayon 线程预算；每个 binary 失败会停止并发 siblings。`runFullRegressionPhase` 在 `:626–643` 为 execute 子进程施加 300000ms 长阶段 deadline。没有逐个普通 Rust case 的 10000ms 计时、取消和超时报告。Rust harness 自己按测试串行/并发调度，所以一个 case 可在 binary 共享剩余期限内超过十秒而不触发 case watchdog。

反证已逐项考虑：`docs/testing.md:91` 的单 case 十秒规定适用普通 case；`:99`/`:107–120` 的五分钟 deadline 适用于明确的长批次/完整回归，不能替代单 case 上限；必跑 ignored 长用例由独立 `executeRequiredLongValidations` 分类，亦不能把所有普通 harness case 变成长例豁免。Web runner 的每 case/child 十秒也不影响 Rust harness。故保留为测试工具监督缺口；这不是本轮复现了某 case 超时，也不构成交易逻辑缺陷。与 sweep78 N78-02、sweep81 N81-1 是同一项，不再编号为新发现。

### 已修复/已决事项，不新增候选

- **G27：** 当前生产 `scripts/publish-release.mjs` 在上传前确认来源 SHA，上传并逐项核对 draft 资产后再查 tag SHA，确认一致才公开。原需求已落生产 caller；旧结论“已修复”维持。API 检查和公开命令无法组成原子锁，既有约束诚实，不以此另提无法实现的绝对锁要求。
- **CompanyPanel：** `apps/web/src/components/company/CompanyPanel.tsx` 的选择协调 effect 仅在 `ready`/`empty` 执行；展示内容只在 `ready` 且 report/statement 可用时渲染。旧 loading 暂空清选择问题已解决。测试 fixture 曾修正 scope 与公司身份关系，故不按旧初审 fixture 提出披露范围错误；也没有证据表明该 UI 把游戏虚构公司的公共报告冒充现实公告。
- **短测、发布与长验收：** summary 记录的脚本短测、红绿 TDD 与真实 fresh E2E 分别按当时命令/源码范围理解。发布链路后续 build-only 是更新的明确产品决定；短测通过、workflow 存在或历史 Release 成功均不能代替未执行项目。K7、真实 UI 性能、ignored 扩展验收和 GUI 安装继续保持原文未验证边界，不新增产品实现缺陷。

结论：当前可确认的遗漏仅是普通 Rust harness case 的十秒硬门禁，既有编号下去重跟踪。G27 与 CompanyPanel 旧问题已由现行生产 caller 修复；build-only/不 smoke 是明确决策；summary 的历史测试与发布证据不冒充本轮或更广范围验收。没有新的 A 股语义变更发现。
