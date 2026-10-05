# 手机范围标题与玩家反馈范围的独立审查

2026-10-05。审查者为本轮新建的 `feedback_scope_independent_review_sol_high`，未参与实现，不复用前批 reviewer。基线为 `d22de3201fab5c4f21d4757ac5b228b130cdbd34`。按委派只读产品实现、既有测试和日志；未运行全量构建或测试、未操作用户游戏、未提交或推送。

## 结论与范围

本批手机标题和 NPC 普通拒单通知的改动通过独立门禁：没有发现阻断本批提交的有效缺陷。整体终端 goal 尚未完成，当前存在一项有具体现场数据支持的布局问题：902×833 看盘交易栏展开时，条件单的 `.sr-only` 标签令 document 高度扩展至 996px。此问题不由本批引入，也不应混入本批修复，但必须在整体收尾前单独修复和复核。

初始完整 diff 为八个 tracked 文件、92 行新增/9 行删除：`DESIGN.md`、`UX-CONTRACT.md`、`App.tsx`、`mobile-ui-state.ts` 及其测试、`protocol/effects.ts`、`protocol-effects.test.ts`、`security-browser.spec.ts`。随后再次读取两份工作文档的增量：`requirements-completion-audit.md`、`terminal-fidelity-plan.md`；它们明确保留旧失败、记录本轮真实结果，并将新布局边界列为未关闭。产品 diff 在审查期间未改变。下一批独立新增的 `desktop-scroll-boundary.spec.ts` 不属于本批实现或本批测试结论。

已读 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、`docs/trading-rules.md`、ADR-0010、完整 `DESIGN.md`/`UX-CONTRACT.md` 及 frontend-design/frontend-design-premium 的审查规范。对照了共享 owner、相关源码上下文、测试断言和实际截图，没有用旧 reviewer 的结论代替本次检查。

## 门禁一：大 A 语义及依据

本批没有修改交易时段、撮合、T+1、涨跌停、价格笼子、申报单位、资金冻结或存档协议。`effects.ts:13` 仍先调用原 `rejectionMessage`；`effects.ts:15` 只限制玩家 notice 的投影，`effects.ts:18` 的全部 `SettlementError` 和 `effects.ts:21` 的 `Trade` 投影保留。业务拒绝与系统故障的区分符合 `docs/architecture.md:138` 的既有 `IntentRejected`/`StepFatal` 契约。

玩家身份并非此次新设默认值：`packages/engine/src/session.rs:1358` 明确 `AccountId(0)` 为玩家，`:1391` 实际构造 `AccountKind::Player`；`App.tsx:88` 和 `:305` 起的玩家委托事件处理早已使用同一身份。protocol parser 的 `parse.ts:105` 对拒单账户/原因仍严格校验，未知原因也不会因非玩家过滤而静默绕过。此处不需要重新选择交易制度，本次未查新的交易所条款；现行规则的正式来源和核对日期继续由 `docs/trading-rules.md` 记录，不能把本次 UI 投影复核冒称为重新核验全部制度。

范围标签继续使用证券集合 `all`/`watchlist`/`holdings`，没有将账户持仓页变为行情筛选页。`mobile-ui-state.ts:35` 对行情/自选列表读实际 `view`，另外三页按自己的身份返回；`App.tsx:481` 从同一 `securityBrowser.view` 传参。价格、数量及时间单位均未变化。门禁一通过。

## 门禁二：需求必要性与最小范围

实际列表数据已切为全部但标题仍显示自选，会误导用户；标题从现有范围状态派生是必要修正，不另建标题状态、筛选状态、重置规则或保存机制。`SECURITY_LIST_VIEW_LABELS` 为范围按钮/桌面标题的现有 owner，本次手机直接复用，没有新建等价 owner。

NPC 普通业务拒单显示为玩家“委托被拒”会误指用户操作；过滤位置在所有宿主共用的 effect 投影层，未删除协议事实，没有设备/宿主分支。保留系统结算故障可见性满足显式错误要求。两处改动都围绕反馈范围一致性，文档仅补充对应 owner 契约，无新增依赖、引擎策略或顺手重构。门禁二通过。

## 门禁三：测试、跨层与复杂度

新增标题纯测遍历行情/自选两个入口、三种范围；另对交易/持仓账户/我的三页与三种范围做交叉断言。旧测试只是补入明确参数，没有删断言。真实浏览器新增 case `security-browser.spec.ts:5` 检查 320px 自选入口→全部五行→持仓空态、1440→320 保持、我的/账户持仓身份以及行情入口恢复全部。已有桌面范围/搜索/空态用例继续通过。

新增协议测试 `protocol-effects.test.ts:10` 对非玩家账户 1、7、60003 各构造 NPC 拒单、玩家拒单、NPC 结算故障、成交，经过完整 `reduceEngineUpdate` 后精确比较 notices、原始 events、规范排序后完整 facts、游标和重试无 effects，并检查成交/automatic-order 投影仍存在。事件序列和事实没有经玩家过滤再进入 state；过滤只影响 notice。规范排序使用未改的 `factIdentity` 与 normalize 契约，修正 fixture 的顺序预期没有弱化事实逐字段断言。`CivilUpdate` 继续走同一 `effectsFromFacts`，没有另设过滤逻辑。门禁三通过，没有发现需要扩展本批的遗漏边界或复杂度。

直接读取的验证证据：

- `feedback-scope-red-unit.log`：有效行为红，11 通过/2 失败，分别是标题错误与多出的 NPC notice。
- `feedback-scope-green-unit.log`：保留首次实现后 fixture 错把 canonical facts 顺序当输入顺序的失败；不能称该轮绿色。
- `feedback-scope-final-unit.log`：13/13，116.299916ms，无取消/跳过。
- `feedback-scope-full-unit.log`：逐 shard 重算 tests/pass 合计均为 849、fail/cancelled/skipped 均为 0；155 文件、8 分片、可用 CPU 10、wall 2371ms。`scripts/run-web-tests.mjs` 仍同时使用 10000ms case 与共享进程树 deadline。
- `feedback-scope-full-e2e.log`：完整 65/65，44.0s，实际 3 workers；对应公司、图表、交易、存档、范围、排序用例全部列为通过。共享外部 300000ms 与 RAYON10 来自本轮执行记录；日志本身未记录 live CPU 样本，不宣称测得饱和利用率。
- `feedback-scope-production-build.log`：执行 tsc/Vite/release-WASM 校验，Vite 362ms，release WASM verified。
- changed lint 日志无告警输出；full lint 明确有五个既有 `children-prop` 警告，bounded command exit 1。`feedback-scope-premium-audit.json` strict 为 0 findings。审查者另执行 `git diff --check` 成功。

没有用静态 premium audit 代替交互证据，没有把定向 lint 冒充全库 lint 通过。

## 19 项终端需求的独立完成审计

下表对照需求审计原十九行，检查当前 owner、实际测试断言和可用截图。通过代表已存在针对性的实现与验证证据；不会扩展为所有硬件或全天无限组合的承诺。截图存在历史状态时只用于它当时已观察到的事实，不覆盖后续修复状态。

| 原需求 | 本次独立核对的实现与证据 | 当前判定 |
|---|---|---|
| macOS 编译、网页可玩、内置浏览器 | production 日志包含 tsc/Vite/release WASM；真实 902 及 320 新局截图显示应用和暂停时钟 | 网页路径有直接证据；本次不另验 Tauri 打包。HMR 后是新 09:15:51 局，不能说旧局无状态恢复 |
| 横屏电脑、竖屏手机 | `App.tsx` 依 orientation 接同一数据/命令；65 回归包含 mobile-layout、desktop-workspace、范围往返 | 有证据 |
| 同花顺层次与模拟器看盘下单 | `DesktopTerminal.tsx:42` 起区分行情/个股/公司/交易/游戏；`:68` 复用唯一 order panel 到底部交易栏；desktop-workspace:299/:333 验草稿和同屏 | 层次和操作有证据；窗口布局仍受下文明确问题阻塞 |
| 二三级菜单、进入返回、当前选择 | `ChartDisplayMenu.tsx:35`/:76 分层 root/averages/indicator；Escape 返回及触发焦点；desktop-workspace:87/:129 验菜单与 F10/返回选择 | 有证据 |
| 尽可能以手机为准复用 | `LocalRefreshViews.tsx:148` 和 `MobileStockDetail.tsx:194` 共用 MarketKlinePanel；FiveLevelBook、CompanyPanel、UserPanel 和命令 owner 共用；shared-chart-state 验双向设置 | 有证据；分时两种 renderer 共用投影，不能称所有绘图只有单 renderer |
| 当前分辨率完整显示 | desktop-workspace:349、kline-coordinates:49 验内部按钮/指标边界；报价320/390和暂停触控有独立用例 | **未关闭**：902×833 实际 document996px，现有断言未查 document.scrollHeight |
| 积累分时数据后核对 | 当前 MobileIntradayProjection 读取权威 minute/auction；09:48:49 两端 `intraday-session-join-*` 截图显示连续段、量能和接缝 | 动态数据证据已有；历史截图内截断报价/旧占位/诊断按钮不代表当前仍未修复 |
| 分时0%轴永远居中 | DesktopIntradayChart:33 对称域/:55 固定50轴；mobile 模型 symmetricIntradayScale；intraday-reference-axis 实查两端几何；09:27:19 两端 final-intraday 截图与单边涨幅一致 | 有证据；当前用户新要求替代旧贴边要求，K线无此变化 |
| 竞价左侧无单0轴、更新粗点，连续无点 | 共用 auctionDisplayPoints/auctionContinuousJoin；DesktopIntradayChart 仅 auction.updated 绘 circle，continuous 仅 polyline；真实09:48:49可见左段/接缝 | 有证据；指示价和更新点未冒充成交 |
| 去掉图内 TradingView 标 | price-chart-runtime 的 attributionLogo=false；实际 SVG K线截图无图内标；原归属说明保留 | 有证据 |
| 均线多选、颜色及 MA5/10/20/30/60 | kline-moving-averages:3 五种统一配置；MarketKlinePanel:77 多选/数值；desktop-workspace:218 和 shared-chart-state；最终902K线截图五条均线清晰 | 有证据，没有另设桌面 MA 计算 |
| 滚轮缩放、Shift平移 | useKlineGestures 共用；desktop-workspace:239 断言 capacity48 和 offset12 | 有浏览器输入证据；未扩大为每一种实体滚轮实测 |
| 双指缩放、单指点按 | 同一 useKlineGestures；desktop-workspace:255 起 pinch capacity30、tap、合成click抑制、拖动不关闭及再次tap关闭 | 有合成触屏输入证据；实体触屏未测为诚实限制，不作为无限追加任务 |
| 首次无对齐线、点开再点关、移动吸附与详情 | MarketKlinePanel 局部 selectedTime，KlineDetails 使用同一显示索引；desktop-workspace:275 首次0、开启详情、移动更新、关闭后不跟随 | 有证据，切股/切屏的局部关闭与共享设置区分清楚 |
| SVG 拉伸边框不变粗 | non-scaling-stroke 来自统一 renderer/CSS；desktop-workspace:293 查 computed vectorEffect/strokeWidth，并在1440重复 | 有自动样式证据 |
| 排序/搜索/自选真实可用、去无关工具 | useSecurityBrowser/shared labels/sort model；security-browser 与 security-sort 含 Enter/键盘/跨屏/损坏恢复；MobileStockDetail:200 仅财务/盘口/资金；App:667 严格 host capability gate | 有证据，手机标题本批也关闭；DEV 无能力入口真实DEV历史两case证据与源码符合，production无入口不冒充DEV验证 |
| 公司资料层级和跨屏阅读 | LocalRefreshViews:194 的 ConnectedCompanyPanel 保持受控阅读；company-information:228/:258 双向阅读与公司选择；:193 日终自然日；:39 四表/附注/精确分项 | 本轮65全部通过。旧净利gold失败不再列未关闭；当前精确断言非只查总计。历史因果探针为既有证据，本次没有重编历史引擎 |
| 游戏设置与存读档细节 | UserPanel 共用；save-file:176 先选择再beforeRead再getFile；save-commands/file-target/day-end-persistence 的待写/失败/取消/新局测试；quick-load-barrier 本轮两项通过；save-control-labels 两尺寸通过 | 有证据，前批待写边界不再列未关闭；原生文件API依 adapter mock，不冒称全部原生环境现场验证 |
| 好后 commit/push、停编译agent | 本轮任务要求编译agents已停，用户授权提交推送；审查末手机主题已本地提交22a1c68，NPC主题与推送仍待主agent收尾；本次审查无Git mutation | 尚待主agent收尾；整体布局问题修复/新独立门禁前不得标全goal完成 |

本轮直接查看了 `mobile-scope-header-all-after.png`、`mobile-scope-header-holdings-after.png`、`terminal-kline-trade-final-902.png`、`final-intraday-centered-desktop.png`/`mobile.png`、`intraday-session-join-desktop-current.jpg`/`mobile-320.jpg`、`company-reading-final-current-window.png`、`dev-diagnostics-hidden-desktop-current.png` 与 `save-control-labels-mobile-320.jpg`。后几份历史截图含后来已修复的边界，已经结合当前源码与最新用例判定，没有从单张旧图重新打开旧问题。

## 整体 goal 唯一已证实的当前布局问题

**[P2] 约束隐藏标签位置，避免 document 被交易栏内容撑高。** 公共 `.sr-only` owner 在 `apps/web/src/index.css:125` 设置 absolute/clip-path 但没有定位锚点；`App.tsx:605`/:606 的条件单隐藏标签仍按所在内容位置计算。`terminal-trade-scroll-before.json` 记录902×833下 body height/scrollHeight 均833，html clientHeight833/scrollHeight996，两个标签 bottom959.3125/996.3125。实际 `terminal-kline-trade-final-902.png` 同时可见额外 document scrollbar。该数据由主agent只读现场采集，本 reviewer 独立读取原始JSON、截图和源码后确认，未自行操作游戏。

现有 `desktop-workspace.spec.ts:349` 只比较按钮、指标与各内部面板的 bounding box；`:333` 只查 `.app-root` 水平宽度。因此65通过不能证明 document 垂直边界已正确。下一批应先增加这一实际布局的失败几何断言，再修公共 owner，并验证看盘栏、完整交易页及移动标签的可访问性/内部滚动不受影响。问题范围明确，无需追加任意硬件或全天所有行情组合。

审查末主agent已开始下一批独立红测/CSS修复；这不改变本批八个产品/规范文件和两份工作文档的已审内容。下一批的 `.sr-only` CSS、新几何用例和结果不属于本记录，也不能在本记录中预先判定修复完成。

本批可按已审边界提交；整体 goal 必须保留上述具体未关闭项和全库五条旧 lint 告警。审查结束，不接续实施下一轮。
