# 文件读取屏障与财务 gold 的全新独立复核

2026-10-05。本记录来自本轮新建、未实施任何产品改动的 gpt-6.1-sol / high reviewer；结论依据本轮自行读取的完整未提交 diff、源码上下文、历史原始证据和最终日志，不采用上一 reviewer 的结论。未启动测试、构建或浏览器，未改产品及主 engine；仅写本审计记录。

## 范围与规范

审查 tracked diff 的全部七个文件：DESIGN.md、UX-CONTRACT.md、company-information.spec.ts、save-commands.test.ts、useSaveCommands.ts、file-target.test.ts、save-file.ts；同时核对本主题 company-causal-audit.md、probe、setup 与三个报告 JSON。已读 AGENTS.md、docs/principles.md、architecture、open-questions、testing、error-handling、ADR-0025/0031，以及 company-accounting §2.6 和 trading-rules 的简化登记。两个改动主题应分别提交；隔离审计副本不应整体提交。

## 门禁一：大 A 语义与证据

文件读取只协调既有自然日日终持久化，不改 T+1、撮合、单位、费用、档格式或日内禁止保存。loadFile/recoverFromFile 传入同一 DayEndPersistence.beforeRead；用户选择后等待调用前已提交的写入，读取成功且校验后才 invalidate 与等待退出。取消不进入屏障，不使在途日终失效。newGame 仍先 invalidate，再等旧写入退出；本批只补回归测试。现有 host/generation 与 SessionReplacementGate 的晚到结果检查保持。

FS Access 的 AbortError 取消判断只包围 showOpenFilePicker；beforeRead/getFile/text 的 AbortError 均显式失败。Tauri 在路径选择后、readTextFile 前等待；upload 在选到 File 后、text 前等待，并向调用方明确拒绝错误。上传 File 已在选择时取得，不承诺其具有 FS handle 的重新获取快照能力；该环境本来不支持日终文件持续覆盖。三个 adapter 复用一个可选回调，无新增持久化 owner。

公司精确值更新的合法范围是已批准游戏经营模型，不是新宣称现实会计制度合规。docs/company-accounting §2.6 与 trading-rules 已登记月末直线折旧、年末计税与合同到期收付等为游戏简化，并保留法源限制；本批未改 engine。

自行解析 company-causal-before-report.json、merged-report.json、counterfactual-report.json：before 与 counterfactual 整个 JSON 完全相等，merged 有真实金额变化。git show 确认 36b6f83 加入 OperatingDayRun::run 的 settle_period_end 调用。对 git archive 20e160b 与隔离副本逐字节比较600个共有文件，差异只有 Cargo.toml、Cargo.lock、day.rs；workspace 仅缩减到 engine，lock 从622项缩到136项，未新增 package name/version 或改变共有 checksum/source。业务源码唯一差异是移除这一调用并加中文审计注释，因此不是用多处公式调整拼出旧值。

probe 使用 GameSession::new(seed42) 与 query_public_reports，未推进市场、未读私人账套。setup 对照 DEFAULT_SETUP 与 tradingE2E 的零 NPC/Random/30、9、3 tick 参数一致；历史 Money number 与当前分字符串表示差异、历史 policy identity 差异均已登记。原36提交 books_mut 编译失败保留，未修补冒充原样复现；20合并原样与单调用反事实是有效因果对照。有效 before/merged/counterfactual 构建日志确实出现 Compiling engine，不采用0.21秒误复用产物为有效 after。

独立精确复算：12928574075.43 - 109278690.49 + 2871190.48 = 12822166575.42。管理费用1949999961.00→2059278651.49；减值414414280.89→411543090.41；新增1602累计折旧净借方变动-109278690.49。新增 UI 断言分别固定这些分项与净利精确值，原期间、编号、四表与精确模式断言保留。证据足以允许此 fixture 的 gold 更新，不推广为全部财务模型证明。

## 门禁二：最小必要范围

文件屏障直接修复读到尚未提交文件的问题，只扩展 adapter 回调并缩小取消 catch；未重构 DayEndPersistence 或改保存流程。测试覆盖实际 adapter 与命令边界，newGame 沿用实现。财务部分仅对齐已有经营提交后的公开报告精确 UI fixture，新增分项断言增强定位能力，无主 engine/生产 WASM 修改、无依赖增加。工作文件位于 agents/macos-webui-design，正式规则仍在 docs。未发现无关产品扩展或不必要复杂度。

## 门禁三：边界与验证

FS Access 的真实等待 fixture 区分旧seed41和提交后seed42，提交前不调用 getFile；覆盖选择器先打开、取消不等、Error/AbortError屏障失败不读。Tauri 覆盖先选后等及失败不读；upload 新增成功等待、失败不读、取消不进入屏障，每次 createElement 创建独立 EventTarget，click 的 this:EventTarget 仅补类型。两个命令入口覆盖不提前取消当前写入，新局覆盖失效并等待退出。原断言未删除、降低精度或放宽 timeout。未发现阻断性遗漏或跨层语义漂移。

独立读取 TDD 日志：file-read-barrier-red-unit 的提前读取实际1/期望0与缺少 rejection 是有效行为红；abort-red 显示普通屏障已正确拒绝后 AbortError 仍被误当取消；最终31/31短测通过。Tauri mock 字符串被 SDK 解析成 NUL JSON、upload 重用同一 input 的重复 listener、多次 tsc startup 类型错误均是 fixture/编译失败，不能当产品有效红；均保留日志且最终类型修正没有弱化断言。

最终完整 Web 日志 final-boundaries-final-unit.log 汇总847/847、无失败/取消/跳过，155文件、8分片、wall2173ms；后续仅测试 this 类型标注后的定向33/33、410.49ms。最终 final-boundaries-final-e2e.log 显示64/64、50.6秒、3 workers，包含真实 WASM 新报告断言。此前两次 webServer exit2 均失败，不能与最后成功拼接。reviewer 未独立重跑，上述为已读取的主 agent 原始运行证据。

最终 production 独立构建日志 final-boundaries-production-build.log 证实 tsc -b、Vite 343ms、release WASM verified 成功。五个变更文件定向 lint 日志无 finding；premium-audit.json strict 为0 finding；本 reviewer 的 git diff --check 通过。全库 final-boundaries-full-lint.log 仍五个原 children-prop 告警、exit1，不能称全局 lint 通过。主 agent 报告 E2E 进程树 CPU 采到多个 headless/renderer，该样本表示实际并行，不表示所有核心满载。本 reviewer 未自行监测 CPU。此审查只关闭本批产品静态、TDD与财务因果门禁，不把整体终端所有需求自动宣告完成。

当前结论：本批文件读取屏障和公司 gold 两项的三条独立复核门禁通过，无未解决阻断性 finding；最终验证证据已核对。整体终端需求结论仍须以最后的需求审计与其明确范围为准，不能由七文件 diff 推断全部完成。本次最终 requirements-completion-audit.md 与 terminal-fidelity-plan.md 的全部增量已核对：当前表关闭两个已有门禁，历史失败段落保持，真实最终64/64证据与此前失败分开，新发现手机列表范围与顶栏不一致明确留待下一批。两个最终截图自行视看显示第1日09:27:19、000812涨10.18%、两端0%中心及留白差异；desktop 截图含已有委托拒绝 notice，曾建议澄清操作范围；最终两个工作记录均已明确本批未新增玩家委托并保留已有拒绝 notice，需求审计另明确未改名单，该文案项关闭，未隐去截图证据。恢复与本批操作历史属于主 agent 陈述，本 reviewer 未操作页面。本批可以按文件屏障/财务证据两个主题分别提交，整体 goal 继续在途。不在下一改动批复用本 reviewer。
