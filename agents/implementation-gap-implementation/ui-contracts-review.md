# UI contracts 独立复核

## 范围与时点

复核者为未实施产品改动的 `review_ui_contracts`。收到作者 `implement_ui_contracts` 的 ready 通知后，复核其 [实施记录](ui-contracts.md) 所列完整 UI diff 和新增文件；施工期发现只用于反馈，未当作最终完成证据。依据根 `AGENTS.md`、工程原则、架构、开放问题、错误处理、ADR-0007/0009/0010、`DESIGN.md`、`UX-CONTRACT.md` 及总账完整原行。

完整范围为 `apps/web/index.html`、`App.tsx`、`App.css`、`index.css`；`app/LocalRefreshViews`、`useMobileUiController`、`useTradingCommands`、`useSessionHostLifecycle`、`usePausePreferences`、`session-control-commands`、`mobile-detail-focus`、`trading-field-errors`；`components/MarketGrid`、`market-grid-accessibility`；`mobile/MobileStockDetail.tsx/.css` 及实施记录中的十份定向测试。交叉追踪 chart/protocol→Redux→真实详情 renderer，seed→真实 lifecycle 和 EngineHost Promise→真实 App/hook；不是只审纯 helper。

## 有效发现与再次复核

1. G40 移动详情倍速最初仍直接 dispatch Redux，删除旧 speed effect 后不再调用宿主。作者已让 `App.handleSpeedChange` 经 `ConnectedMobileDetail.onSpeedChange` 透传至真实详情，同全局/桌面共用 `SessionControlCommands`；最终源码及接线短测通过。
2. G40 暂停偏好最初仍先改 checkbox/Redux/storage，再异步通知宿主。作者已让两套 checkbox 调用 `changePreferences`，pending 时禁用；runtime 串行 await actor 确认后才 `apply` 与持久化。宿主失败不写 UI/storage，storage 失败仍显错，最终延迟确认/拒绝短测通过。
3. 新暂停偏好 runtime 最初只守卫旧宿主成功，旧宿主晚到拒绝仍报进新局。作者已在 catch 同样检查 `isCurrent`；补齐旧拒绝隔离与拒绝后队列恢复测试，复核通过。

上述发现均已修复；本 UI 范围未留产品代码阻断。G40 整体仍需宿主 reviewer 对 Worker/Tauri/Remote 的回执与失败锁定独立核销，不能仅凭本报告核销三宿主全链。

## 逐 G 判断

“实现可核销”只表示原总账静态缺口已修且真实消费者已接入，不表示浏览器视觉/辅助技术验收通过。

| G | 判断 | 消费证据与未覆盖边界 |
|---|---|---|
| G22 | 实现可核销 | hook 即时生成字段错误并复用现有元/股解析及证券类别数量上限；App 委托/条件单输入关联 `aria-invalid`、错误节点。买卖方向提交时保留 T+1/冻结股数与零股卖出预检，不把合法零股输入无条件判错。 |
| G23 | 实现可核销；浏览器验收未执行 | App 真实选股走 `openDetail`，hook layout effect 在详情挂载后聚焦返回；关闭恢复连接中的来源元素，失联时恢复可聚焦行情区域，并使用 `preventScroll`。切股不重置来源或抢焦点。 |
| G24 | 实现可核销；浏览器验收未执行 | 信息 tab handler 仅更新状态，删除主动 `scrollIntoView`；没有用 helper 替代真实 hook 接线。 |
| G25 | 实现可核销；像素验收未执行 | 最终 CSS 返回首列 44px、返回及切股高 44px、切股宽 44px；未证明 320/390/430px 实际热区无遮挡。 |
| G30 | 实现可核销；键盘视觉验收未执行 | 详情根定义 `--msd-focus`，真实图表窗口按钮焦点规则继承该 token，不再引用缺失变量。 |
| G32 | 实现可核销；像素验收未执行 | 实际 renderer 将 SVG、昨收虚线与 0% 标注放入同一扣除 23px 时间轴的 `.msd-price-plot`；昨收投影中点及 CSS 中点都为该绘图区 50%。 |
| G33 | 实现可核销；宽度矩阵未执行 | 后置报价/摘要/周期/盘口字号改为 `clamp`+`cqw`，不再覆盖为固定 px；响应式容器现有配置保留。 |
| G34 | 实现可核销；动效验收未执行 | 最终 App CSS 在 reduced-motion 下覆盖详情之外的实际交易底页/遮罩，取消 transition/animation。 |
| G48 | 实现可核销 | HTML 使用 `zh-CN`；真实 AG Grid 消费中文 locale。额外对安装的 AG Grid v36 源码核对实际 `ariaSortableColumn`、`ariaMenuColumn`、`ariaFilterColumn` key，不凭自定义 helper 名字认定支持。 |
| G64 | 实现可核销；浏览器键盘验收未执行 | 真实 Grid 开启 cell focus、注册 `onCellKeyDown`，Enter/Space 选择实际 cell 股票并走同一 `onSelect`；方向键不被拦截。 |
| G65 | 实现可核销 | 实际主导航/行情分类公开 `aria-current`，交易按钮公开 `aria-expanded`/`aria-controls`；未添加与现有组件不符的强制 tab role。 |
| G68 | 实现可核销 | App 股票 options、hook 当前证券选择与数量类别、快捷涨跌停均使用 `activeSetup`；非默认创业板代码下单及真实快捷价 SSR 已覆盖。名称缺失显示代码，不编造证券信息；现有规则算法本身未扩大。 |
| G12 | renderer 可核销；全项见 chart 复核 | SVG 真正消费价格涨跌方向，上涨 `fill=none`/红 stroke、下跌绿 fill，维持槽位与量高度；不将 buy 标为主动买卖方向。投影相邻有效价格及同槽方向完整结论由 [chart 复核](chart-stream-review.md) 给出。 |
| G14 | renderer 可核销；全项见 chart 复核 | protocol effect 携所属 frame tick，Redux→详情每行消费 `formatTradeTime(trade.tick)`，不读取当前时钟；缺字段显示“成交时间缺失”，非法 tick 显错。 |
| G20 | consumer 可核销；全项见 seed 复核 | 真实 lifecycle 新局调用熵端口，E2E 固定 seed、读档无损 BigInt 路径独立；最终 helper/baseline 结论见 [seed 复核](seed-baseline-review.md)。 |
| G40 | UI consumer 可核销；三宿主全项待宿主复核 | 暂停/继续/倍速 await 确认后改 UI，串行控制并隔离旧 host 成功/失败；详情/全局/桌面同一 owner。偏好等待 actor 确认后发布 UI/storage；启动、返回启动页及清理等待 Promise 并显错；App 协议失败走 fatal callback，accept 结果同步回传。宿主内部确认/失败锁定不由本 UI 结论代证。 |

## 大 A 语义、必要性与边界

本批不新增交易制度，无需凭印象引入新规则。委托权威单位仍为股、价格仍为分，UI 元/手只展示；T+1、冻结股份、零股一次卖完、按类别的数量上限及涨跌停沿用现有规则模块。真实行情、成交时间与量均消费权威事实，不造主动买卖方向或逐笔全天统计。activeSetup 修复使非默认局与规则配置保持一致。新增 owner 分别仅管理字段关联、导航焦点和异步控制，未另立交易权威、未引入新依赖或无关功能。

尚未执行浏览器焦点/滚动/热区/中轴像素/字号宽度/动效/桌面键盘矩阵，也未执行屏幕阅读器、完整回归、长验收或生产构建。源码字符串 gate 是接线回归证据，不是浏览器行为证据；这些验证边界必须保留在总报告。

## 独立定向验证

作者 ready 后，复核者独立在 `apps/web` 执行实施记录的十文件命令：`timeout -s KILL 10s node --experimental-strip-types --test --test-timeout=10000 --test-concurrency=4` 后附 `trading-field-errors`、`trading-commands`、`mobile-detail-focus`、`market-grid-accessibility`、`ui-contract-wiring`、`local-amount-render`、`mobile-component-render`、`session-control-commands`、`session-host-lifecycle`、`pause-preferences-runtime` 对应 `.test.ts` 路径。4 个并行隔离进程、case timeout 10000ms、整命令 deadline 10 秒；exit 0，10 个文件全部通过，0 fail/skip，测试框架 wall 1366ms、shell wall 1.50 秒。Node v25.8.2。`git diff --check` exit 0。作者报告的 tsc/lint 未由复核者重复执行，不当作独立重跑结果。

## 冻结后增量再次复核

收到作者最后冻结通知后，再审以下完整增量：

- G22 的 `visibleFieldErrors.price` 在已有提交错误或已触碰时重新读取当前 validation，未触碰空价格提交失败后切换 market/highest 不再显示被禁用字段的旧价格错误。新增真实 hook 测试覆盖此边界；没有放宽价格输入或提交规则。
- `app-startup-wiring.test.ts` 将已移出的 App callback guard 断言迁到真实 `SessionControlCommands`，保留两处失败身份守卫并检查 App fatal 透传；lifecycle 更新断言强化旧身份返回 false。该变更是跟随职责移动的等价断言，不是删除保护或弱化成功要求。额外完整阅读既有 `app-persistence-wiring.test.ts`、`app-workspace-wiring.test.ts` 相关消费门禁。
- 最终 `App.css` 移除没有消费者的 `.market-select-button`，字段错误使用明确红色 `#a62020`，没有引入交易语义变更。

独立再次短测：同一 `timeout -s KILL 10s node --experimental-strip-types --test --test-timeout=10000 --test-concurrency=4` 命令，附 `src/app/trading-commands.test.ts`、`src/app/trading-field-errors.test.ts`、`src/app/app-startup-wiring.test.ts`、`src/app/app-persistence-wiring.test.ts`、`src/app/app-workspace-wiring.test.ts`；5 文件全部通过，exit 0，0 fail/skip，框架 wall 396ms、shell wall 0.55 秒。初轮十文件加三份接线文件组成最终 13 文件复核范围；本次只重跑受增量影响的五文件，没有把分批短测伪称完整回归。最终 UI consumer 无遗留阻断；浏览器矩阵与三宿主内部全链的既述验证边界不变。
