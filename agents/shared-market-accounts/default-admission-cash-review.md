# 默认入场资金独立审查

- 范围：审阅 `GameConfig::proposed_defaults()`、默认值契约测试与 Web `DEFAULT_SETUP.config.starting_cash` 对齐情况；不修改 production 文件。
- 需求依据：ADR-0030 和用户确认的默认新入场资金为 100 亿元；已加入账户余额不随设置变更追补，身份不会因登录或加载存档自动获得资金。
- 审查方式：检查本批文件差异、完整 Engine 配置字段与相邻默认值测试，并核对 `join_market` 的资金发放路径。没有启动 Cargo。

## 结论

默认值改为 `1_000_000_000_000` 分，换算为 `10_000_000_000` 元，即 100 亿元，符合用户产品决策及 ADR-0030。`Money` 金额单位是分；Web wire 使用十进制分字符串，同步值 `"1000000000000"` 没有单位漂移。

Engine 默认配置测试仍逐项断言佣金率、印花税率、涨跌幅、一手股数、佣金下限和起始资金，起始资金断言改为 100 亿元，没有发现弱化断言。Web 测试把精确的 `starting_cash` 字符串预期改为新值。可见 `DEFAULT_SETUP` 差异还包含独立的 `report_frequency` 添加；它不属于本次资金改动，本审查不对该独立变更作判断。

经济语义上，初始资金只通过新建/加入成员账户时的 `starting_cash` 发放。已有成员重复加入返回原成员和账户，不重新注资；更新入场设置也只影响后续加入者。登录本身没有加入账户的路径。未发现资金自动追补或随存档加载改变当前控制授权的行为。

## 复核结论

上一轮发现的字段注释问题已修正：`packages/engine/src/config.rs` 现在写为“默认 100 亿元 = 1_000_000_000_000 分”，与实现一致。此前 Web 注释已移除，保持单一默认真源，没有新增过时说明。复核未发现其他与默认入场资金有关的阻断问题；**本小批 PASS**。

## 验证状态

- 已读取 `.tmp/checklist-wave4/default-cash-green-default_admission_cash_is_one_hundred_yi_yuan_in_cents.log`、两个 `default-cash-green-proposed_defaults_*.log` 和 `admission-web-green.log`：Engine 默认 100 亿元 case 1 passed，默认配置两项 case 各 1 passed；Web defaults 7 passed。结论仅限这些日志记录的 case，不代表完整回归或编译通过。本轮没有启动 Cargo。
- 本审查不涉及交易所费率、撮合或证券类别差异；只调整以分计价的新入场默认资金，不需新增 A 股交易制度依据。
