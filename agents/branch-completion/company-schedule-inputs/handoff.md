# F1 公司新局排期与机制输入叶子交接

本片 base 为 `3984e14167202dc645ec340fbd2a73e661c71c8a`（已批准的 I-D 依赖），branch 为 `feat/web-company-schedule-inputs`。仅提交三个叶子及三个对应测试；工作证据归本目录。精确 tip 由实际本地提交后报告，不能在提交前伪造。

## 已实现的边界

- `ReportFrequencyInput` 导出受控 `value` / `onChange` 接口；`ReportFrequencyDraft` 包含不可提交的字符串 `Monthly`。调用层必须先消除该未完成草稿，才可作为生成类型 `ReportFrequency` 交给新局创建。本片未挂 App，未实现新局按钮门禁。
- `Quarterly` / `Monthly` 切换、两个 Preset、Custom 次月 1–28 日与 0–86399 秒、None 或 Uniform 1–31 日延迟。复用 I-D 的 `save/schema/monthly-report-schedule.ts`；没有恢复旧 `report-frequency.ts`，没有新增另一份 schedule parser。非法值保留原文、发出不可创建草稿并显示错误；外部 value 替换完整同步，组件自身回写未完成草稿时保留正在编辑的文本。
- `MechanismSwitchesInput` 的两个 bool 与回调完全独立；这是新局输入叶子，默认关闭与同局不可修改的语义可见。FullByDefault 使用本人真实现金，不足部分放弃并记录弃配；未实现策略没有提交入口。
- `reportAvailabilityReason` 穷尽当前 Rust / generated 六种原因；未知运行时值抛错，生成类型新增变体也会触发编译期穷尽检查。`isIsoMonthEnd` 验证真实月末、闰年与 Rust CivilDate 的 1900–2199 算法年份窗。

## 本轮 TDD 原始证据

初始实现只从 `18725d07` 提取三个旧叶子，唯一准备适配是 parser import 改为已存在的 I-D 路径。随后先写本片新增测试，再运行红灯，没有用 stub 制造失败，也没有把缺模块或语法错误作为业务红。

- `red-source/`：运行红灯时六个文件的原始快照；叠在 base 上可重现。`red.receipt.json` 绑定命令、Node 版本、source SHA-256；`red.stdout` / `red.stderr` 保留原输出。11 case 中 6 通过、5 因业务断言失败：延迟空值被 Number 转为 None、外部 Monthly 不清空旧排期、来回切换残留、年份窗不一致、现金弃配说明缺失。
- `green.receipt.json` / `green.stdout` / `green.stderr`：同样三个文件并发，11/11 通过；命令明确 `--test-concurrency=3`、`--test-timeout=10000`，外层 `run-with-deadline.mjs 10000`。
- 两组件通过 Vite 的单模块 SSR loader 获取真实组件，执行其真实事件回调；ReportFrequency 另用 dispatcher harness 提交 state / effect。最终 markup 用 SSR 核对。这里没有 DOM、浏览器事件或 createRoot 生命周期证据，不声称浏览器验收通过。绿灯前仅修正 harness 的 children 传递以消除测试自身制造的 React key 警告，没有删改或弱化断言。
- `lint.*`：六文件窄路径通过。`typecheck.*`：六文件及导入依赖的严格窄类型检查通过。两项初次并发启动；TypeScript 6 第一次因 CLI 要求 `--ignoreConfig` 拒绝运行，原始诊断保留 `typecheck-attempt1.*`，仅修正参数后成功，不将工具诊断追认为 TDD 红灯。每条命令均有 10000ms 进程树 deadline，没有 build、full regression 或 Cargo。

`manifest.json` 记录最终六文件及未改动 Rust / parser / generated 依赖 SHA-256，并列出原始证据 SHA-256。

## 语义依据和限制

核对日期 2026-10-26。本片不修改现行交易制度；机制开关、认购现金边界依据 ADR-0035 / 0038 / 0039（含 2026-10-07 注记）；报告模型与公开信息边界依据当前 Rust `company/query.rs`、`information/queries.rs`、`information/monthly_schedule.rs`。月报明确标额外游戏报告，不替代法定年报、中报、季度报告，年度税务核算不变。正式制度来源和已登记的游戏简化沿用 `docs/company-accounting.md` §5、`docs/simulation-calendar.md` §7、`docs/trading-rules.md`；这里未重新在线核验这些文档引用的官方材料，不把游戏 Preset 说成交易所法定排期。

未改 App / LocalRefreshViews / EngineHost / 三 host / protocol / store / generated / save schema 等公共文件；未接结算、未跑三宿主与整树验收。共享 node_modules 仅用未提交 symlink，没有安装依赖或改 lock。完整 diff 的未实施者 Sol 独审待协调者派发，未审前不宣称本片最终完成。

全暂存区 `diff --check` 会指出原始 `red.stdout` 中 Node 断言格式自带的空行尾空格；保留真实 stdout，没有清洗证据。六个正式源码/测试路径的 `diff --check` 通过。
