# Q01 Money 分字符串独立复核

## Web 生产消费与金融精度

非作者 `q01_review_web` 完整核对生产差异和相关消费路径。金融计算、排序、冻结资金及
条件单比较使用 `BigInt`，Redux 保留字符串；OHLC 原始分参与缓存判等，`number`
近似只出现在末端坐标与比例显示，不回算委托。A 股整手、T+1、价格 half-up 与费用
语义不变，没有数字兼容。运行快照成本 parser 已在作者首轮使用严格 `money(...)`，
审查中追加大金额接受与数字拒绝覆盖，不把补测冒称为新发现后的生产修复。

有效发现为 `HoldingEpoch.institutional_fees_paid` 的 TS override 仍为数字；作者
取得类型红证据后修复，实际生成文件同步为 `string | null`，29个 Money 短测含
非空 `i64::MAX` 往返及数字拒绝通过。复核者再审修复和实际生成结果，无剩余阻断。
复核者独立运行相关24个 Web 短测通过，没有执行完整回归。

## Rust 与 Server

复核者 `q01_review_rust` 未实施本批代码；审查基于 `675ac4c` 后本轮未提交
diff，完整阅读 `packages/engine` 与 `apps/server` 的改动，并核对中央 `Money`
实现、快照、因果诊断、经历字段、深度 serializer 及已有公司会计金额契约。

- `Money` 严格接受 canonical signed `i64` 十进制整数分字符串；拒绝数字、正号、
  负零、前导零、空白、小数、指数、非 ASCII 数字及越界值。`serialize_str`
  直接输出整数分，不经浮点转换，没有数字兼容、迁移或 schema 版本。
- `Position`、`PositionSnap` 的累计成交额、`BeliefDebugSummary` 的每股估值，以及
  causal 的报价、成交额、价格、预算、金额列表和金额 map 均复用中央 codec。
  可选报价保留 `null`，容器形状不变。`HoldingEpoch.institutional_fees_paid`
  的非空金额及 TypeScript override 同步为字符串；最终实际生成的
  `HoldingEpoch.ts` 已核对为 `string | null`。
- 内部 `i64` 运算及现有舍入代码没有变化；每股价格、资金、费用保持整数分。
  股票价格最小变动 `StockSpec.tick` 迁移为分字符串，交易时钟 tick、股数、
  比例和 bp 不迁移。`AccountingAmount` 保留独立 `i128` 元字符串，累计
  unsigned turnover 与聚合 Resource 不被错误收窄为 `Money`。
- JSON fixture 和既有断言保留相同金额，仅修改 wire representation。
  `SavedLiveEnvelope` 派生字段拒绝测试新增合法 baseline 可解码前置断言，
  避免错误金额形状导致假阳性；WS 先拒绝数字 Fixed，再受理字符串 Fixed。
  `price_volume_baseline` 三处金额 fixture 同步属于必要修改，未改变交易算法。
- 新短测覆盖 `i64::MIN/MAX`、超 JavaScript 安全整数、非法输入、裸金额字段、
  optional/list/map 和 TypeScript 类型。协调者报告 collector 新增 case 真实
  diagnostic binary 通过，example 当前 binary 的 5 个 case 通过；本复核未
  自行重跑测试或完整回归，不把他人报告冒充独立执行。

当前已到位 Rust/Server 部分未发现必须修复项，`git diff --check` 通过。
改动不调整 A 股交易制度、费用和舍入，因此没有引入新的交易规则假设；范围为
用户确认的 Q01 编码与必要消费边界。两个 causal 既存失败已有旧源码复现记录，
本批没有删除、弱化断言或顺带改 fixture。

`extraction_replay.rs`、`step_skeleton.rs` 的最终完整 diff 只更新六个表示摘要，
保留旧锚证据、原 fixture、业务 guards、同 seed、restore 及 seed/order 扰动断言。
完整阅读独立 producer 与逐 token 核验工具：两侧分别来自真实旧、新 engine，
核验只准许已列明金额 path 的数字原 token 精确加引号，并重建完整原字节比较；
不是由失败断言 left 生成预期，也没有给生产加入历史格式兼容。
复核者独立运行 auction 0 的原文核验，外部 10 秒上限，约 9.2 秒、exit 0；
60,815 个许可金额 token 变化精确复现当前摘要 `14588880118627202930`。
首次五个 verifier 并行尝试均未在 10 秒内取得结果，因此不计成功；其余场景
以实施者取证记录为依据，本复核不声称独立执行全部 capture 或完整回归。
至此 Rust/Server 与最终表示摘要 diff 审查通过，无待修复发现。
## Web 既有测试、当前 fixture 与正式文档

复核者 `q01_review_fixtures` 未实施生产代码、既有测试或 fixture；分组完整阅读
全部修改的 Web 既有测试 diff、当前 fixture、三个新增 Money 测试文件及本主题
工作证据。独立逐叶比较 `current-schema-save.json` 与 `675ac4c`，确认仅367个
安全整数金额转换为同值十进制字符串，所有对象键、数组长度及其他值完全不变。
股数、时间、事件序号、比例和 `AccountingAmount` 元字符串没有混改。

旧断言未删除或弱化；原 Web 安全整数金额限制改为完整 `i64` 契约是 Q01 的必要
变化，仍拒绝金额越界与非法股数。图表新增 `rawPrices` 的断言符合精确分值不可从
近似坐标反算的要求。新测试覆盖数字拒绝、非规范编码、`i64` 边界、错误路径、
大金额一分精度、字符串字典序反例、冻结资金与成本舍入，没有增加旧格式兼容。

独立运行 `utils/money`、`save/schema/money-wire`、`save-schema-contract`、
`complete-save-schema` 与 `app/money-wire-ui` 五个相关文件：47个 case 全通过。
外部 `run-with-deadline` 为10000ms，case timeout为10000ms，并发为4，实际
wall-clock约0.58秒。未执行完整回归，`git diff --check -- apps/web` 通过。

完整审查 ADR-0031 与 architecture、tech-stack、trading-rules、ADR-0030 和两份
历史 Money/性能文档的 diff：`1000000000000` 分为100亿元，单位正确；规范
ASCII 有符号 `i64`、Web `BigInt`、图表末端近似、独立 `AccountingAmount` 元
字符串及 unsigned 聚合范围边界一致。历史数字输入保留指纹并明确只在对应旧源码
复测，不建立兼容入口。没有引入新的 A 股交易制度、费用或舍入假设，无阻断发现。

## 表示锚的独立复算

完整阅读独立 producer、逐 token/path verifier、取证记录及两个原测试的最终
diff。producer保留真实成交、库存不注资、股份守恒、T+1、费用减少现金和自然
日日结 guards；测试只修改固定摘要及证据说明，原 fixture 与业务断言不变。
verifier仅许可明确金融类型路径的同值数字 token 加引号，并重建完整原文作字节
相等断言，禁止结构、键、事件顺序或其他数值变化；不读取失败断言的 left。

首次五份 verifier 并发使用外部9秒 deadline，replay与扰动 seed通过，三个 step
在完成前超时，不记为成功。随后在内存中仅将摘要算法替换为独立的高低32位
FNV-1a实现：低位乘435、高位加低位乘256及进位，各中间值均小于安全整数范围；
逐 token/path与字节重建代码不变。三个 step并发在约8.36秒完成，旧六锚全部
复现，当前六锚与正式测试一致。replay及扰动 seed亦取得完整最终路径报告。

独立验证覆盖全部66份真实捕获；step的变化 token分别为60815、60381、59942，
replay为14774，扰动 seed为14703。所有变化仅为已核对金额的同值分字符串。
SHA-256、FNV-1a与取证记录及新测试锚一致；两份 baseline causal测试源码已通过
`cmp` 确认逐字等于 `675ac4c`，其失败日志与既存断言位置相符。此次取证不是
产品完整回归，也不能将两个既存 causal失败报告为通过。

最终工具正式采用高低32位摘要，并将每个 step 的20份完整 projection 切成四个
五tick核验分片，完整20份摘要另设 `digest` 入口，不缩减任何捕获或业务 guards。
独立复核全文后的最终脚本以17个进程并发执行：12个 step 原文核验分片、3个完整
摘要和2个 replay。每进程外部9秒 deadline，整批约5.14秒，17份退出码全部为0；
记录在 `/tmp/q01-review-final-*.log` 与 `.exit`。该结果不依赖内存替换后的脚本。
