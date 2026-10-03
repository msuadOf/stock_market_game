# sweep77：仅构建发布、公开报告选择修复与tag SHA TDD记录

目标源码 `b76ece3`，worktree HEAD `4ad5a2e`；只读diff确认本组三文档、publish脚本/测试、workflows、公司组件与query coordinator对目标无差异。根AGENTS/principles已读（承接sweep06）。仅新增本工作记录，无产品/Git写操作、无测试、构建、网络/发布或长任务。

全文连续读完 `agents/oop-release-validation/build-only-review.md` **29行**、`company-report-selection-fix.md` **48行**、`publish-tag-sha-tdd.md` **35行**，共**112行**。均一次读到EOF无截断；生产搜索输出超限不当作正文完成证据。另核现行ADR-0028正式发布边界及当前release DAG。

## 结论

无新增独立遗漏。**G27维持已核销**：当前publish入口确实在上传和远端资产核验后第二次查tag SHA，才执行公开；没有把两个请求说成原子tag锁。公司报告刷新清空选择的缺陷已有当前组件effect保护，不重开。最新build-only决定优先，发布和手动产品构建不要求测试/lint/smoke；手动开发CI仍保留测试，二者不矛盾。

三篇记录的历史20/20、13/13、12个fresh E2E、15/15等结果不作为本轮重测；本轮未查询Actions/线上状态，也不据旧结果宣称三平台当前构建、Pages线上可玩、全部游戏回归已通过。

## build-only-review 逐章条款

| 原文条款/行号 | 当前caller/正式决定 | 状态与反证 |
|---|---|---|
| 首部日期/基线/最新用户范围（3–6） | ADR-0028 accepted明确2026-10-03仅构建；release.yml唯一自动tag入口 | 旧“发布必须CI/lint/E2E”不再是现行必做，不重复找删掉的门禁当漏实现。取消旧Windows重跑是历史执行决定。 |
| 范围方法（8–12） | 沿release-policy→distributions→publish-release→Pages；build-targets/frontend-build/package入口已有 | 仅审工作流与构建/打包，不推断engine/域规则改变。未修改产品、不真实发布是历史记录，不是本轮获得发布权限。 |
| 独立结论1 A股语义（16） | workflow改动没有进入engine/账户/存档业务，正式ADR继续保持分/股/T+1/日终 | 无新增交易所规则实现，不能以发布重构拓宽业务范围。 |
| 独立结论2取消CI/测试/smoke，保留构建检查（17） | `.github/workflows/ci.yml:7`仅workflow_dispatch；distributions.yml:75工具版本、77frontend-build仍执行生产TS/WASM | `test "$(wasm-pack --version)" = ...`是工具版本shell检查，不是测试套件；生产TS编译/manifest校验被允许，不能误判违反build-only。 |
| 独立结论3 DAG/缓存/权限/十平台组（18） | `release.yml` jobs validate→distributions(all)→publish→pages；prune-caches needs三个结束、always且未cancelled；publish contents:write、Pages OIDC、独立清理actions:write | 独立纯Server矩阵不依赖frontend，native/webui用同run frontend；不合并目录下载。没有新增缺caller。 |
| 上传/来源/两次SHA/完整性（20） | publish-release.mjs:13收集、16十目录、30静态commit、39实际/manifest文件集合、45大小/符号链接、49SHA、51重名；83和109两次tag查询、113公开 | **G27实修**，不重开上传窗口旧缺陷。collector格式组合边界仍由package入口，不宣称独立固定安装器总数。 |
| 证据20短case（24） | 工作记录保留历史build-only-green.log，契约测试release-policy.test.mjs:52/73覆盖DAG与禁CI/tests | 源码断言存在不是本轮通过；不替代三平台真编译。 |
| red仅文件级摘要（25） | 记录明确无断言详情 | 不冒充逐项TDD red；未编造日志。 |
| YAML BaseLoader/needs检查无actionlint（26） | 人工DAG与输入/权限当前可核 | Python解析不等于GitHub Actions语义穷尽；不升级必须新建actionlint或发布测试。 |
| diff人工审查与collector边界（27–28） | publisher明确10组身份与manifest实物核验；未独立列每平台安装器扩展名 | 既有边界诚实保留，未找到被build-only删除的制品完整性caller。 |
| 真实编译/Release/Pages/线上需分报（29） | 现行台账证据章节也分列历史结果/未重测 | 验收证据范围仍成立，不在本轮联网重查。 |

## company-report-selection-fix 逐章条款

| 原文条款/行号 | 当前生产caller/代码 | 状态与反证 |
|---|---|---|
| 首部工作记录非正式规范（1–3） | 修复仅选择状态，不改变公开报告事实/时点 | 正式财务/交易规则优先，不新增领域定义。 |
| 原浏览器12中11、trace真实点击后回退（5–9） | `CompanyPanel.tsx:47`选中ID为组件state；88行真实DisclosureList按钮onSelect | 原缺陷是刷新状态清空，不能因“点击已成功”排除失败，也不能把旧失败当当前仍错。 |
| coordinator acceptCivil强刷→loading→visibleReports空（11–15） | `company-query-coordinator.ts:96`startCompanyQuery，acceptCivil接自然日元数据；`company-view-model.ts:16`非ready返回当前可见页集合（root loading时为空） | 刷新仍真实发生，不能通过停止自然日更新掩盖问题；修复在选择协调边界。 |
| ready/empty才协调，loading/error/unavailable/idle保留，真实失效/empty回退（17–20） | `CompanyPanel.tsx:59/60/61/62`effect只ready/empty才setSelectedReportId；50行selectVisibleReportId与reports重新求当前显示 | **已生产接线**，旧“loading清选择”不再成立；真实empty/失效ID仍回退，不永久锁旧ID。 |
| E2E真实button/pressed/键盘/期间（19–20） | `e2e/company-information.spec.ts:67/69/70/71/73/75/76`实际半年度button与aria-pressed、编号/期间及ArrowRight后确认 | 断言没有简化成随便点元素；源代码存在不代表本轮浏览器执行。 |
| memoryHook提交effect模拟，三边界，跨公司同字符串ID规则不变（22–24） | `company-report-selection.test.ts:33/48`memoryHook与effect；67/79/86行三个行为；90行fixture financials.scope更新新公司 | 模拟组件state不是DOM验收；不擅自新增跨公司报告ID迁移政策，旧失效ID回退已有。 |
| red新增回归失败（28–29） | 当前测试仍期望loading/error后保留选择 | 日志属于原TDD，未重跑、不称当前red。 |
| green13/13与TSC修正（30–35） | test33行显式memoryHook<Props,…> | 泛型修复存在，不把旧fresh编译报错当当前产品漏实现；未运行tsc。 |
| 复用旧preview的12pass被排除（36–37） | 工作记录明确不能作修复后fresh证据并清端口 | 不把它混成最终fresh证明。 |
| 首次fresh失败与最终fresh12pass（38–44） | 记录CI=1/独立端口4189/workers2/retries0/共享300000ms、清理含在deadline | 历史真实构建+验收支持当批修复，但不替代本轮或线上Pages矩阵；不要求进入最新build-only发布。 |
| 域规则不改/独立复核fixture修正（46–48） | 选择effect不改DTO/公开事实；test90行scope为新公司，正式公开查询未暴露私有状态 | 独立有效发现已修，未发现新生产缺口。 |

## publish-tag-sha-tdd 逐章条款

| 原文条款/行号 | 当前caller/代码 | 状态与反证 |
|---|---|---|
| 首部修改范围与未真实发布（1–3） | scripts/publish-release.mjs主入口79行；release.yml真实调用带冻结RELEASE_SHA | 工作记录不是GitHub公开授权，本轮不调用发布。 |
| 需求：核验后、公开前再查tag，不符/查询失败显式保draft（5–7） | 109行await invoke commits/tag，110/111行不符抛中文错误，113行才edit --draft=false；没有catch fallback | **G27满足**；调用失败自然传播，未产生公开命令。 |
| 语义与非原子锁边界（9） | 两次查SHA与edit是分离请求 | 消除上传期间可见变化，不声称最后查询与edit间不可能再变。不能以没有不可用原子锁重开原G27。 |
| 红灯测试/mock与真正Missing rejection（11–21） | publish-release.test.mjs新增case模拟首次旧SHA、二次另一SHA且draft/digest成功 | 记录是断言级红灯，区别import失败；日志旧实现Published事实保留，不写成本轮复現。 |
| 绿灯四行实现/正常路径精确顺序（23–32） | publish-release.test.mjs:84第二次query、85/86后edit；移动tag用例断言没有edit | 原成功发布断言仍存在；历史15/15只支持当批变更，未用其证明游戏回归或线上发布。 |
| diff与独立门禁交接（33–35） | build-only独立复核20行已核当前双查询未被改回；总账G27核销有生产依据 | TDD记录本身不替代独立门禁，后续独立核对存在；本轮不运行测试。 |

## 候选与反证

- “发布仍缺CI/lint/smoke”：排除。ADR0028最新用户决定显式取消，当前release没有ci job/needs；手动开发CI执行它们仍正确。
- “G27只写测试没改生产”：排除。publish-release.mjs109–113有真实caller，且release.yml传递validate.outputs.sha，检查在上传及远端核验之后。
- “第二次tag查询等于原子锁”：排除此过度核销主张。两个HTTP请求边界仍在，原G27要求的实际复核已兑现，不承诺更强锁。
- “CompanyPanel刷新仍无条件清空选择”：排除。60行ready/empty明确守卫；loading/error不改selected state，真实失效集合回退仍在。
- “12个旧preview通过等于修复fresh/线上全通过”：排除。记录自身否认旧preview证据，另有fresh构建历史结果；本轮/线上不冒报。
- “collector不固定安装器总资产数所以发行未实现”：排除整体缺口。当前collector真实要求10组与manifest文件一致，格式组成由打包入口验证；本分片未找到正式要求被遗漏的新增caller。

本轮未执行测试、构建、Actions查询、GitHub发布或Pages部署；提交父agent汇总并纳入完整diff独立复核。
