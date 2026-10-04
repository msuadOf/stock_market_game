# UI contracts 实施记录

## 范围与语义

- 工作基线：`aaaa9f9`，独立 worktree `implementation-audit-final`。本模块不改总账、README、Git index 或提交。
- 阅读依据：根 `AGENTS.md`、`docs/principles.md`、`UX-CONTRACT.md`、`DESIGN.md`、`docs/architecture.md`、`docs/error-handling.md`、`docs/open-questions.md`、ADR-0007/0009 与总账对应原证据（`reaudit-ui.md`、`exhaustive-review/luna02.md`、`sweep41.md`）。
- 不新增交易制度：沿用现有 `parseYuanPrice`、`parseShareQuantity`、`validateAShareQuantity`、`aSharePriceLimits` 与 `activeSetup` 的证券类别。金额权威单位仍为分、数量仍为股，行情手数只用于显示；不造盘口、成交或主动买卖方向。未决 Q04/Q07/Q08 不在本批裁决。

## 实施映射

| G | 真实消费与实现 | 定向验证 |
|---|---|---|
| G22 | `useTradingCommands` 根据输入即时生成价格/数量字段错误，提交方向预检关联数量；`App` 委托与条件单字段使用 `aria-invalid`/`aria-describedby` 和字段错误节点。证券类别数量上限参与即时校验；买/卖方向尚未选择时不把合法零股卖出错误判为非法输入。 | `trading-field-errors.test.ts`、`trading-commands.test.ts`、`ui-contract-wiring.test.ts` |
| G23 | `openDetail` 记住原股票行，`useLayoutEffect` 在实际详情挂载后聚焦返回，返回优先恢复原行、失联时恢复可聚焦列表区域，`preventScroll` 保持滚动。切股不抢焦点。 | `mobile-detail-focus.test.ts` 与 App/hook 接线检查；未做浏览器焦点验收。 |
| G24 | 信息 tab 只更新状态，不再请求 `scrollIntoView`。 | `ui-contract-wiring.test.ts`；未做浏览器滚动验收。 |
| G25 | 返回所在首列固定 44px，切股按钮 44px × 44px，可见图形仍用容器字号响应。 | 最终 CSS 热区规则检查；未做浏览器像素测量。 |
| G30 | 定义实际消费的 `--msd-focus`，修复图表工具按钮 outline 无效问题。 | CSS token 与实际消费静态复核；未做浏览器焦点可见性验收。 |
| G32 | 新增共享 `.msd-price-plot`，SVG、昨收中轴和 0% 标注共用扣除时间轴后的绘图区。 | 真实 SSR 绘图区结构测试；未做浏览器像素测量。 |
| G33 | 后置报价、涨跌、摘要、周期与盘口规则改为 `clamp(... cqw ...)`，不再以固定 px 覆盖容器字号。 | 最终 CSS 规则检查；未跑 320/390/430px 浏览器矩阵。 |
| G34 | 在最终 `App.css` 增加覆盖详情外交易底页/遮罩的 `prefers-reduced-motion`，取消 transition/animation。 | 最终 CSS 规则检查；未做浏览器动效观察。 |
| G48 | HTML `lang=zh-CN`；`MarketGrid` 把中文 `localeText` 配置交给实际 AG Grid；使用安装版本的真实排序/菜单辅助文本 key。 | `market-grid-accessibility.test.ts` 与真实 JSX 接线检查。 |
| G64 | 不再禁止 AG Grid 单元格焦点，`onCellKeyDown` 把 Enter/Space 映射到同一 `onSelect`，保留方向键导航并提供中文说明。 | 键盘 owner 测试与真实 Grid 接线检查；未做浏览器键盘验收。 |
| G65 | 主导航与行情分类通过 `aria-current` 公开当前状态；交易按钮额外公开底页展开状态/目标。 | 行情分类真实 SSR 与主导航 JSX 接线检查。 |
| G68 | 股票选项与默认选择仅取 `activeSetup.stocks`，快捷涨跌停也取当前 setup 的类别规则，不再查 `DEFAULT_SETUP`。非默认代码无证券名称契约时显示代码而不编造名称。 | 非默认创业板代码 hook 下单与实际快捷价格 SSR。 |
| G12/G14 renderer | 分时量改为真实 SVG rect，半像素宽不变；上涨红色空心（`fill=none`）、下跌绿色实心；使用 chart 模块提供的价格方向。逐笔每行消费 `formatTradeTime(trade.tick)`，缺时间明确显示，不借用当前时钟。 | `mobile-component-render.test.ts` 保留量柱原高度/槽位断言，增加双色空心与每笔时间/缺时间测试。 |
| G20 consumer | 生命周期无存档非 E2E 新局调用 `createNewSessionSeed`；E2E 维持固定 seed，读档无损消费存档 seed。 | `session-host-lifecycle.test.ts` 固定三条路径、0 seed 与熵失败不创建/不 ready/显错。 |
| G40 consumer | `SessionControlCommands` 串行 await start/stop/setSpeed 后才更新 UI；详情/全局/桌面都接同一控制 owner；后台控制也等待确认。生命周期 start/dispose/返回启动等待 Promise，释放失败显式报告。暂停偏好 await actor 确认后才发布 Redux/storage，pending 禁用两套 checkbox，旧宿主成功/失败不能污染新局。协议失败 callback 同步回传 false，致命失败使 running=false。 | `session-control-commands.test.ts`、`session-host-lifecycle.test.ts`、`pause-preferences-runtime.test.ts` 与真实 consumer 接线检查。 |

## 验证记录

- 红绿证据：新增字段/focus/control owner 首次是模块缺失的准备失败，不计作行为红灯。G32/G14 实际 SSR 首次断言失败，新增绘图区/逐笔时间消费后通过；SVG G12 的真实 renderer/原高度断言首次失败，替换 renderer 后通过；生命周期/暂停偏好延迟确认断言首次失败，await 接线后通过；字段类别数量上限及未触碰价格错误在改为禁用字段时撤下的后续用例也均取得真实行为红→绿。不能把准备失败冒充完整 TDD 行为证据。
- 核心定向命令（工作目录 `apps/web`）：`timeout -s KILL 10s node --experimental-strip-types --test --test-timeout=10000 --test-concurrency=4 src/app/trading-field-errors.test.ts src/app/trading-commands.test.ts src/app/mobile-detail-focus.test.ts src/components/market-grid-accessibility.test.ts src/app/ui-contract-wiring.test.ts src/app/local-amount-render.test.ts src/mobile/mobile-component-render.test.ts src/app/session-control-commands.test.ts src/app/session-host-lifecycle.test.ts src/app/pause-preferences-runtime.test.ts`。10 个隔离测试文件通过，最后记录 wall 1.95 秒；普通 case timeout 10000ms，整命令 deadline 10 秒，4 个并行测试进程。
- 追加相关既有 `app-startup-wiring.test.ts`、`app-persistence-wiring.test.ts`、`app-workspace-wiring.test.ts`，3 进程并行，case/deadline 同为 10 秒，3 文件通过。startup 旧同步调用源码断言迁至真实控制 owner，保留跨宿主晚到更新/失败 guard 并加强失败同步返回 false；并未删掉保护断言。
- 上述全部 13 个相关测试文件另合为同一短批次，4 进程并行，wall 1.54 秒，13/13 文件通过；新增未触碰价格错误撤下的定向两文件再次通过，wall 0.51 秒。
- 编译与测试分开：外部 10 秒 deadline 内并行执行 app/node 两个 `tsc --noEmit --incremental false`，最后 wall 5.84 秒，exit 0。没有启动完整回归、长验收、浏览器矩阵或生产构建。
- 针对拥有的生产 TS/TSX 文件运行 `oxlint --threads=4 --deny-warnings`，exit 0；`git diff --check` 无输出。
- 上述 CSS/SSR/纯 owner 测试只证明实现与消费接线，不伪称实际浏览器像素、屏幕阅读器或交互矩阵通过。[UI 复核](ui-contracts-review.md) 初轮与最终增量均通过；三个有效发现已修复并复验。最后价格字段反馈、既有接线断言迁移与未使用样式清理已由同一非作者再次复核，独立 5 个受影响短文件四进程并行通过，wall 0.55 秒。G12/G14 全链见 chart 复核、G20 全链见 seed 复核、G40 全链见 host/remote 复核，不用本模块的 consumer 结论代证其他模块。
