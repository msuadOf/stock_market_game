# R1 名册日终候选交接（2026-10-25）

## 范围与前置

基线 `b172c1ab`；独占工作树 `.worktree/registry-dayend-r1`，分支 `feat/b4c-registry-dayend-r1`。
恢复素材优先采用已适配 StockId 的 `97ee0b8d`，历史 `18725d07` 只作对照。
原方法原件见 `legacy-97ee0b8d-registry-methods.txt`。

新增 `registry_day_end` child module。不可变 `SessionCorporateActions` 与最终账户持仓、
真实 `PersonalTradeConfirmation` 输入，输出仅 `registries` / `dividend_tax_books` 克隆候选。
不改真实账户，不安装候选，不接生产，不运行分红、回购、除权或完整公司日终链。
历史名册、税账与非交易过户事实须先通过完整链校验；本片仅把新增 PublicMarket
名册日结同步到税账，不用跳过历史 scope 掩盖历史税账坏事实。
回购 book 的 fills / cancellation 与 Treasury 的三方库存勾稽仍须在后续相位处理；
本片只核对名册 Treasury 与输入最终账户股数，不提前要求回购 book 含当日 fill。

## 接口

`SessionCorporateActions::registry_day_end_candidate(&self, RegistryDayEndInput)`。
Input 字段：`day`、`catalog`、数值 `accounts`、`confirmations`、
`issuer_repurchase_account`、`late_holder_tax_accounts`。完整类型见源文件。
后入税账资格集合仅 AShareIndividual 传入个人身份账户；Flat / Exempt 传 None。
`RegistryDayEndCandidate` 仅带 `registries` / `dividend_tax_books`。
公共错误枚举只新增 `RegistryPositionMismatch { date, stock, account, expected, actual }`；
`stock` 是 StockId，`account` 是 AccountId，股数单位为股。

## A 股依据与适用范围

实际读取的官方正文及 SHA256 见 `official-rule-excerpts.txt`。
财税〔2012〕85 号第三条规定证券账户日终净增减、FIFO；2015 年 101 号第三条
继续援引其他操作事项，第一条变更计税档次。适用截至核对日期；本片不更改税率。
后入个人自动税账、Flat / Exempt 游戏简化沿用 ADR-0040；Treasury 身份沿用现有
ShareRegistry / 回购模块，不新增制度默认值。未变动正式规则文档。

## TDD 与实际验证

`fb0b5135`：先写四项业务预期与显式失败骨架；真实四红日志 `red.log`。
实现 `48cd9c50`：七项 domain 测试通过，见 `domain-final.log`。
新增 runtime fixture：四个 Retail + 玩家，三组含非满尾组；2026-01-05，6 ticks，
auction=1、closing=2；普通 tick 3 与 closing tick 5 各买入 100 股；
从 DayEndCandidate 的显式 group/local 映射提取最终账户，真实确认恰两笔，共 200 股。
最终八项通过，libtest 0.21 秒，见 `green9.log`；普通命令外部 10000ms deadline，
`--test-threads=4`，该命令中每个 case 均小于 10 秒。

构建属于显式长验证，以 `run-long-validation.mjs 300000` 启动，
独立 target=`根/.tmp/registry-dayend-r1-target`，`CARGO_BUILD_JOBS=8` / `cargo -j 8` /
`CARGO_PROFILE_DEV_DEBUG=0`。最终 no-run 构建 23.88 秒，原日志 `final-build.log`。
运行中核对进程树：rustc 链接 / codegen 有 4 threads；单个 engine crate 的前端类型检查
受 rustc 工具阶段限制，不能以多个并发 cargo 重复编译同一 crate 改善。
未跑 all / 全量验收，未合 main，未 push。

## 保留失败与 #119 边界

`green.log`：runtime 初始 fixture 误用 quote_setup 的 2030 元旦，civil 校验拒绝；
修正为交易日，未弱化断言。
`green5.log`：测试把尾组 padding 当真实账户，改为与 D0 helper 一致按 AccountId 映射。
`green7.log` / `green8.log` / `diagnostic.log`：普通确认未安装导致只观察到 closing 100 股。
原因是手动 `step_frame_equivalent` 仅产生 pending receipts，生产
`failure::step_via_runtime` 另调用 `take_pending_receipts` / `runtime_append_trade_receipts`。
fixture 现使用相同真实确认安装入口，保留 200 股与两笔确认断言，未伪造确认或修改生产。
核心候选算法在这些 fixture 修正中未加任何持仓补偿。

该小活动证明最终账户与普通+closing 确认可严格勾稽，不证明旧 #119 的
1200/1500 精确场景已复现，不把旧名册多 300 股直接判为重复入账。D0 镜像延迟
是否已经消除原差异，须由独立后续整链证据判断。

## 独立复核状态

实现者尚未独审；由 root 安排未实施者复核完整 diff、依据、必要性与边界。
在独立复核通过前不宣称本片完成，更不宣称公司行为链已接通。
