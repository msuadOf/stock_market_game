# UI 设计分支合并与补缺衔接

## 目标与取舍

用户要求先合并 `feat/ui-design`，再继续修改 UI。Desktop／Mobile 的导航、布局、图表坐标、交互和滚动校准以该分支为准；main 的精确金额、真实公历日期、初始股份设置、日终持久化和受理因果边界不能因视觉合并回退。最新远端目标为 `0110e9db`，其中新增双向交易面板和定时委托；此前基础合并目标为 `6c42c69c`。

用户另已明确：MA 周期可编辑，默认5／10／20／30／60；K 线周期采用自然日／周／月／季／年，以及120／60／30／15／5／1分钟，统一放在折叠选择表单。不保留5／20个交易日聚合。季 K 以公历1–3、4–6、7–9、10–12月分桶，年 K 以公历自然年分桶，只合并桶内已有真实日期的日 K；标签沿用本桶首条日 K 的真实日期，不制造休市日的行情记录。`5日分时` 是此前已批准的跨日分钟展示窗口，与 `5分钟 K` 分开；`120分钟 K` 不与120根视窗容量混用。新增周期与可编辑 MA 在设计分支合入后适配其共用图表，不重新恢复旧布局或第二份设置 owner。

## 工作保护

合并前所有当前改动已完整备份：573个存在的修改／未跟踪文件、binary diff、index diff 和零分隔状态清单保存于 `.tmp/ui-design-merge/`。原并行任务由 `ui-design合并前完整保留checklist并行工作` stash 保留，其对象记录在 `preserved-stash.txt`。仅暂停写入以完成合并，不删除新历史、多人、数据库、经营、股本及指标任务的成果。

设计分支合并提交完成后，再 apply 该精确 stash；恢复时冲突不能整文件覆盖新版 UI。`EngineHost.load` 需要同时保留日终槽身份与恢复提交回调，不能把 callback 当作 archive slot id。

## 基础合并验证

首次编译发现新增测试的 `StepFatal` 引用路径错误，已修正到真实模块；该失败是编译错误，不作为业务红测。基础 Engine／Desktop no-run 多核编译随后成功，使用32个 Cargo jobs 和进程外300000ms编译期限，实际产物清单见 `.tmp/ui-design-merge/base-binaries.json`。no-run 只证明编译，不冒充测试执行。

Web 生命周期32项短测、日历坐标14项短测和 AA 消费者 SSR 1项短测通过。SSR 使用新版真实 `SecurityBrowser` 和 `ChartSettingsFixture`，原可读性断言未弱化。所有普通测试保留10000ms case／进程树期限；不执行复杂回归或浏览器 E2E。TypeScript 检查结果单独记录于 `base-web-tsc-final.log`。

基础设计已在 `2c3b3bb6` 合入 main。最新 `0110e9db` 增量整合保留双向 QuickTrading 及手数输入，并修复可用金额未扣跨证券待受理买单预占的问题：金额与可买手数复用精确计算，界面明确说明预占。当前单玩家整合版16项纯逻辑短测、相关28项调用／SSR短测和最终 TypeScript 检查通过，非作者完整及修复增量复核见 `latest-quick-trading-review.md`。恢复共享市场代码后仍须绑定真实本人账户，不能沿用单玩家的账户0；这项接线门禁不因基础 UI 合并通过而取消。

NPC 日界融合保留设计分支的正常 producer `pending_npc=None` 与下一真实观察时的 deferred preparation，并保持 main 的异步决定完成顺序和 ingress receipts。日终不能生成下一日决定或消耗 RNG／attention。合法同格式的空批输入不是旧存档兼容；本次不扩大公共存档校验或引入迁移。该层实际短测及独立签核见 `npc-boundary-review.md`。

完整限定复核分别见 `base-ui-layout-review.md`、`base-lifecycle-independent-review.md` 和日历坐标工作记录。每份证据只证明其覆盖范围，不把基本合并通过宣称为整个 checklist 或恢复 stash 后全部新功能已完成。
