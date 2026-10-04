# Q06 初始流通盘分配

## 范围

为每个新局配置流通盘的类间与类内分配方式。类间支持 `Random` 或 `Percentage { retail, inst, hot }`；类内支持 `Random` 或 `EqualPercentage`。默认类间比例为散户 45%、机构 53%、游资 2%，类内等比例分配。新玩家账户不获得初始股份。

`Percentage` 字段作为非负有限权重，按现存 NPC 类别归一化。没有 NPC 的类别不参与分配。类内等分的余股按升序 `AccountId` 逐股分配。随机分配继续使用 Session `SplitMix64`；所有模式按整数股数守恒。分配股数可以小于账户数，因此 NPC 可为零持股。

## 现状核对

- 已读根 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0005、ADR-0029、ADR-0031、初始持仓历史设计、`GameConfig` 历史设计、当前 `SessionSetup`/`GameConfig` 与 Web 新局控件。
- 原设计已实现 `StockSpec.float_shares`、`Account::grant_position`、随机及类间权重分配，但 `FloatAllocation` 只能表达一体化 `Random` 或“类间比例 + 类内随机”；Web 没有可编辑的分配控件。
- 当前 Web 默认曾为类间 45/53/2 + 类内随机；本次按用户最新确认改为类内等比例默认。类内随机仍可选。
- 本次不改变沪深交易制度、申报单位、成交、费用或 T+1；改变的是游戏开局持股模拟配置，因此无需改写官方交易规则依据或登记新简化。

## 实现记录

- Engine 类型拆成 `BetweenKindDistribution`、`WithinKindDistribution`，由 `FloatAllocation` 组合；setup 验证类间权重；seed 路径分别分配类别预算及类内账户股数。
- 类间比例只按现存类别归一，最后一个正权重类别承接整数舍入余量；类内等分按账户 ID 递增发放剩余股数。`Random` 类间和类内可以独立选择，仍由 Session RNG 决定。
- Web `FloatAllocationInput` 接入桌面和移动新局入口；类间比例可编辑、类间与类内可独立选择。新局将选项送入 `SessionSetup`；启动与读档回填权威 setup 值。
- Web save parser 精确读取新结构，并拒绝旧 `Random` / `ByKind` 形状；未增加 schema 版本、兼容转换或默认补齐。
- Engine、Web parser、设置渲染及新局 setup 测试已添加或迁移；Engine 测试覆盖两种混合分配轴的同种子确定性，并验证类内等分余股确实给到较小 `AccountId`。所有测试和绑定生成尚未由本实施者运行；遵循协调要求，由 root 统一提供 Cargo 红绿证据和生成 TypeScript。

## 验证与复核

- root 使用 `--jobs 32` 刷新编译；外部 10000ms deadline 下 15 个 `seed_float` 用例以 16 线程运行，全部通过，实际执行 0.37 秒。最初 101 股公司总股本触发开账舍入不平；分配测试保持 101 股流通盘，仅把公司总股本设为 1000000，仍严格断言 `[51, 50]` 及守恒。生产开账问题另行修复，不以调整测试宣称该问题不存在。
- 通过实际 ts-rs 测试生成两轴类型，80 个绑定导出用例通过；Web TypeScript 编译通过。设置渲染、存档 parser、新局／读档及宿主生命周期四组共 52 项短测分别受 10000ms 外部与 case deadline 约束并行运行，全部通过。
- `current-save-fixture-generator.rs` 显式设置本 fixture 的 40/50/10 + `Random` 两轴参数，再由当前真实 Engine 执行两日日终、save→restore→resave 一致校验，重建权威存档。原始完整 fixture 的 Web 严格解析及逐值比较通过；未手工修改历史持仓或补运行时兼容。
- 非作者 `review_q06_allocation` 完整及增量复核通过，包括生成类型、混合轴确定性、余股顺序及生产 fixture 结构摘要。复核未逐字符阅读 9.8 MB JSON，不把结构摘要称为字节级全文审计。
- 配置控件保留归一／余股说明；本局 Engine 创建后、首次开跑前读取真实账户持仓的实际分配展示已接通桌面与移动界面，覆盖各证券类别整数股数、零持股人数、实际占比与流通盘对账，见[实际分配展示](q06-actual-allocation-preview.md)。不重新试跑随机 seed，不用预计比例冒充真实分配；读档不展示恢复持仓作为初始分配。该补充的 3 项 Engine、2 项 Rust 宿主及 30 项 Web 短测通过，三宿主编译、真实生成绑定和 TypeScript 编译通过，最终非作者增量复核以该记录为准。
- 未运行完整回归。
