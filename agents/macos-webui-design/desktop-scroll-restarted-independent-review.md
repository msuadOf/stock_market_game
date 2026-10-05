# document 滚动边界重新启动的独立复核

2026-10-05。Reviewer：全新 `desktop_scroll_restarted_review_sol_high`，`gpt-6.1-sol`、`high`。未参与产品实现；按用户要求重新启动审查，自行读取当前源码、完整 diff、原始日志和截图，没有使用已中断 reviewer 的结论替代检查。基线为 `c8dfe6e`。

## 结论与范围

**本批产品、测试及契约通过独立复核，未发现需要修复的有效 finding。** 已确认最后一个实际 document 越界问题有行为红、最小修复、真实浏览器绿及同一 IAB 会话的修复后几何证据。十九项终端需求在当前声明的 Web、浏览器输入和实际窗口范围内有对应 owner 与验证；没有发现仍需补实现的产品缺陷。最后 commit/push 与本地、origin 一致性由主 agent 收尾，本记录不宣称它们已经完成。

读取了 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0007、`docs/trading-rules.md`、`DESIGN.md`、`UX-CONTRACT.md`，以及适用的 frontend-design/premium skill 与 canonical ownership、verification 指引。审查包括五个 tracked 文件的全部差异、原未跟踪的 `desktop-scroll-boundary.spec.ts` 和 `desktop-scroll-audit.md`，并核对最后同步十九项状态与正式门禁结论的纯文档增量。最后再次读取实际 staged diff，确认七个本批文件、115 additions/11 deletions 与已审源码一致，`git diff --cached --check` 通过；本 review 记录随后单独加入。其他历史截图/隔离探针作为证据读取，不把它们误算为本批新增产品实现。

## 三项项目门禁

1. **大 A 语义及依据：符合本批要求。** 产品差异仅为公共 `.sr-only` 增加 `top:0`、`left:0`，没有 engine、host、金额、股份、撮合、交易时段、费用、T+1、存档或 API 的改变。原条件单 accessible name 仍为“触发价（元）”“数量（股）”，测试填写 300/400 股的草稿，没有提交交易。四处 `.sr-only` 使用均为非聚焦 label/描述；原 label 关联、错误描述和搜索帮助仍存在。该批不引入交易制度判断，因而无需为两个 CSS 锚点新取交易所规则；不能将本结论扩展为所有历史游戏简化均重新通过官方制度审查。既有周/月 K、时间映射、竞价参考轴等仍明确标注显示简化，没有借本次样式修复改变其含义。
2. **必要性和最小范围：符合。** IAB 的 body/工作区为 833px，而 html 内容高 996px；两个隐藏标签的 bottom 为 959.3125/996.3125。真实 E2E 在两个宽度得到 999px 对 833px 的失败。修复直接锚定已有公共 utility，保留一像素裁切；没有添加 document `overflow:hidden`、新 scroll owner、设备分支、状态或依赖。`DESIGN.md`、`UX-CONTRACT.md` 的两段契约与工作记录均说明这个长期边界，范围适当。
3. **边界、跨层漂移和复杂度：未发现 blocker。** 两个新 case 在 902/1020×833 检验 document 的精确高度及 scrollY，按 accessible name 填写条件单，断言原 order-panel 的 contentHeight 大于 height 且 scrollTop 大于零，再验证完整交易页的草稿与高度，以及 320×844 手机底页的名称、编辑、焦点、关闭和返回搜索。现有完整回归补充菜单、Escape、焦点恢复、周期、行情范围与跨屏路径。公共 owner 的其他使用已在源码及相关浏览器用例中检查；未发现新增协议漂移或第二份实现。任意缩放/操作系统/实体触屏矩阵不在该证据所证明的范围。

## 原始验证证据核对

- `desktop-scroll-red-e2e.log` 的首次失败属于 Node fixture 的 document/window 类型编译错误，未计作产品行为红；`desktop-scroll-behavior-red-e2e.log` 才是两个实际 999/833 几何失败。
- `desktop-scroll-green-e2e.log` 为 2/2、8.2s；增强跨屏/完整交易页后 `desktop-scroll-crossscreen-e2e.log` 为 2/2、7.4s。最终 `desktop-scroll-final-full-e2e.log` 是一次完整 **67/67、1.2 分钟、3 workers**，包括增强后的两个 case，没有拼接各次结果冒充全绿。新 case 保持 10000ms，完整批次按记录使用外部共享 300000ms；日志本身可确认 workers 与实际 case 完成时间，不能单凭它反推所有环境参数和全程 CPU 状态。
- `desktop-scroll-final-production-build.log` 显示 tsc、Vite production **1.15s** 和 `release WASM verified` 成功。本次没有再跑 build 或完整 E2E，也未操作用户游戏。
- 当前 JavaScript 产品源相对基线未改；已独立读取上一批 `feedback-scope-full-unit.log`，八个 shard 合计 **849** 个测试、155 个文件、wall 2371ms。它是上一 JS 批结果，**不是本次重新执行的 849**。
- 原 `desktop-scroll-changed-lint.log` 无输出；无输出本身不足以证明 exit code，仅按主 agent 的命令结果记录本批定向 lint exit0。`feedback-scope-full-lint.log` 明确有 **五个既有 children-prop 告警且 exit1**，不能说全库 lint 通过。
- 自行执行 `git diff --check`，exit0。另自行在 `run-with-deadline.mjs 10000` 外部监督下执行 premium `audit_project.py --mode strict --no-write`，exit0、0 finding；只读静态结果没有替代交互证据。
- `desktop-scroll-live-cpu.log` 确为空文件。缩小后的 `desktop-scroll-resource-cpu.log` 六条记录均为 `processes:[]`；`desktop-scroll-resource-e2e.log` 为 2/2、27.2s、2 workers。这是记录中共享 300000ms 的长资源观察验证，不能称普通 10 秒短命令，也没有取得本批 live CPU 利用率证据。workers 并发日志不证明满核。该缺口已诚实保留，无需为采样好看重复已绿全量。

实际查看了修复前 `terminal-kline-trade-final-902.png`、修复后 `terminal-kline-trade-after-902.png` 和恢复后的 `terminal-restored-final-current-window.png`。原始 `terminal-trade-scroll-before.json` 为 html996/body833；after 为 html833/client833、scrollY0、order-panel203/497。截图与几何共同支持修复效果，截图本身不替代精确几何。恢复截图确为 600101、分时、自选列表、09:15:51 暂停、1x；服务监听和 markDeliverable 为主 agent 现场记录，本 reviewer 未重新操作 UI 或独立采样端口。

## 十九项需求独立对应

下表由实际 owner、用例源码、最终回归日志及相关原始证据核对得出，不复述旧 reviewer 判定。历史失败保留为历史证据，不当作仍在发生的当前失败。

| 项目 | 自行核对的 owner/证据 | 当前完成边界 |
|---|---|---|
| macOS 编译、Web 可玩、IAB 打开 | production build/release WASM 日志及实际终端截图 | Web 成品与实际 IAB 画面成立；没有把它等同于原生 Tauri 打包 |
| 横屏桌面、竖屏手机 | useOrientation 的 width>=height、WorkspaceGrid、最终 desktop/mobile case | 同一规则与数据，当前浏览器跨屏通过 |
| 同花顺层次与看盘交易布局 | DesktopTerminal/desktop-terminal.css、App 唯一 section-order、看盘栏 case | 原图/盘口继续显示，内部下单滚动与草稿保持；本批 document 越界已关闭 |
| 二三级菜单、进入返回、选择 | ChartDisplayMenu root/averages/indicator、company/desktop-workspace case | 分层、逐级 Escape、外点关闭及 F10/返回的最终用例通过 |
| 尽量复用手机版 | LocalRefreshViews 与 MobileStockDetail 都使用 MarketKlinePanel/CompanyPanel，共用盘口与命令 | K 线等复用成立；分时 renderer 的有意差异仍披露 |
| 当前尺寸完整显示 | 新 902/1020 几何、既有 320/390 报价/坐标/44px 设置 case、实际修复后截图 | 当前代表尺寸成立，不声称任意长数据全覆盖 |
| 累积分时后观察 | 默认局 09:48:49 实际截图、MobileIntradayProjection/desktop-intraday 用例 | 确有动态真实场景证据；早盘证据不等同全天验收 |
| 分时 0% 轴居中 | symmetricIntradayScale、DesktopIntradayChart y50、手机/mini 共用对称域，reference-axis case及两端截图 | 当前纠正明确针对分时；K 线没有被误改 |
| 竞价参考轴/粗点、连续无点 | auctionDisplayPoints 两端复用、DesktopIntradayChart 连续仅 polyline、null/单点/连接测试 | 指示点与成交明确区分，不补新成交数据 |
| 图内 TradingView 标志 | price-chart-runtime attributionLogo=false、SVG K线、UserPanel 归属说明及实际 K线截图 | 图内无标，归属说明保留；未新作法律许可审查 |
| 五条 MA 多选与顶部布局 | KLINE_MOVING_AVERAGES 5/10/20/30/60、MarketKlinePanel、最终多选/shared-chart case | 五色与独立开关/数值成立，周期不足不补值 |
| 滚轮缩放、Shift 平移 | useKlineGestures/kline-gestures、desktop-workspace 的实际断言 | 浏览器输入已验收，未覆盖所有实体滚轮 |
| 双指缩放、单指点按 | 同一 hook 的 pinch/tap、合成 touch/click 抑制用例 | 浏览器合成手势成立，实体触屏未实测 |
| 对齐线/详情初始关、点开关、移动吸附 | MarketKlinePanel selectedTime、klineTapIndex、对应详情/手势 case | 价量指标同步与局部状态边界成立 |
| SVG 拉伸不增粗 | non-scaling-stroke、HTML KlineCoordinatePlot、1020→1440 样式断言 | 已测几何与样式通过，系统缩放组合不无限外推 |
| 排序/搜索/自选、去无关工具 | useSecurityBrowser/security-sort-model、共享范围标签、DEV capability gate；最终 security/mobile case | 当前范围标题、空态、排序与错误路径有测试；DEV 有能力正向入口未额外现场验收 |
| 公司资料层级及阅读保持 | ConnectedCompanyPanel 的 Redux reading owner、真实 query、8个最终 company case | 跨股跨屏与日期成立；独立比较 before/counterfactual 原始 JSON 全字段相等，merged 不等，现行精确 gold/分项/1602 断言保留且通过 |
| 设置与存读档 | UserPanel、save-file 三 adapter 的选择后 beforeRead、useSaveCommands、文件/队列测试及两个 quick-load case | pending/失败/AbortError/取消/新局屏障有明确验证；没有声称所有原生文件系统环境实测 |
| 完成后 commit/push、停编译 agent | 当前基线与主 agent 的收尾分工 | 产品复核通过；本批 Git 与 agent/service 状态最终确认由主 agent负责 |

## 限制

本次 review 是当前 diff 的独立静态/证据审查，额外执行的只有短时只读 premium audit 与 diff 检查；未重复完整验收或用户游戏操作。没有发现要重新实施或扩大验收的产品问题。五项既有全库 lint 告警、当前 CPU 空采样、实体触屏、任意硬件/全天数据组合和原生 Tauri 打包未实测须继续披露；这些限制不能被写成“全库全平台全绿”，也不把已经关闭的历史失败重新归为当前未完成。
