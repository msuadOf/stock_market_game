# R3a 冻结回执候选契约

基线：本地 main `b172c1ab1a69daa44dc57dddc887db255d7f6ce7`；独占树 `.worktree/share-transform-r3a`。仅恢复 engine 候选 substrate，不接生产、不修改 runtime。

## 范围与相位

消费经 `ShareSplitBook::validate` 校验的 Registered 账簿及其不可变 `RegistrationSnapshot` / `ShareSplitReceipt`。本片不生成缩股总量或碎股政策，也不取消现 domain 支持。`ShareReduction` 是已批准行政数值原语，不代表任意缩股产品已获批准。

输入包含 day、Registered book、R 原单位 registry、个人 tax books、显式 `SplitUnitContext` 链、显式 Treasury 桶。不接收 ex 日新单位市场请求；输出用显式 `PreNewUnitMarket` 相位标记。输出字段明确叫 `registry_before_new_unit_market` / `dividend_tax_books_before_new_unit_market`；不是最终当日账户或名册。原 registry holdings 必须与登记 snapshot 一致，禁止将已消耗新单位成交的名册作为原单位输入。

只输出 `ShareCredit` / `ShareReduction` 数值意图，保留 event、account、registered_before、converted_after；真实账户不写。成本政策沿 D1a：正仓保留 invested/recovered，实际归零清除；ex 当日新单位成交可能令 actual position 非零，不能仅按登记 converted_after=0 就预判实际账户清仓。

Treasury 显式桶给出稳定 key、lot ids、before/after、可选真实 account。initial 库存不自动映射回购账户；各桶 before/after 分别勾稽冻结 Treasury 汇总；禁止跨桶重复 lot。由上游负责按回购实际成交和已验证单位链重放提供桶，本片不会凭整个 Treasury 数量伪造实买库存。税账沿用现 domain 既定取得日/非应税缩股核减，不冒充完整官方税务结论。

## 法源与未闭边界

2026-10-25 实时读取上交所现行《交易规则（2026年修订）》官方原文：[发布页](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/c_20260424_10816482.shtml)、[DOCX](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/10816482/files/e3a3b98e36894edca77841837936e006.docx)。4.3.1 规定权益登记日次一交易日除权除息；4.3.2 公式允许发行人申请调整；4.3.3 用参考价计算涨跌幅。本片不据此发明一般 A 股股份拆细或合并登记制度。

ADR-0039 仅授权范围并要求先核验制度。旧 `agents/company-system/share-split.md` 已承认缩股碎股、总量、R/R+1、税务期限没有统一明文；旧 floor(total/ratio) + 余数排序只是送转类比，未找到用户明确批准该细则的原文。反例：两户各1股、3并1，旧 domain 的总量为0、两户均归零。候选消费该冻结事实不等于批准此生产分配政策，产品/法源边界留 B5 显式关闭。

M 尚未交付：本片只验证显式正确日期/单位链输入，不以共享开市天数冒充逐股实际开市序列。R4 仍须解决先转换、后同日新单位市场名册消费的编排；现 ShareRegistry::close_day 不允许在转换同日追加 PublicMarket，本片不绕过此约束。


## TDD 与真实阻断

首轮9例：placeholder编译成功后6个行为红、3个输入拒绝绿（`red.log`）；最小实现后8绿1红。失败用例原 fixture 把“单户归零”误构造成“发行总股数归零”：已冻结 domain 接受1股三并1后总0，而 ShareRegistry 明确拒绝 `issued share conservation mismatch`。保留此事实为独立负例，未放宽原领域守卫；单户归零正例使用账户1+外部5股三并1（总量6→2、账户1→0）。该 fixture 修正有明确制度/跨层原因，不把总0偷偷改成已支持。

成本检查限于数值意图 `PreserveUnlessActualPositionCloses` 与实际量的边界；本片不写真实 Account，实际 invested/recovered 保持/归零集成须由 D1a/R4 验证，不能把意图测试冒充组端成本验收。


顺序单位链又暴露现有 registry API 的历史限制：close_day 虽准许下一自然日 ShareReDenomination，validate 在已有 receipt 时只准该 scope 与上一 receipt 同日。因此代表性双事件测试显式输入 ex 日空 PublicMarket 日界 baseline（无新单位成交），候选不自动伪造 baseline。首轮误用了2030端午休市日，后改为权威 `TradingCalendar::next_trading_day` 决定本股登记/ex 日，并逐自然日推进空基线；不是用共享交易天数充当 M 实现。

Treasury 输入另补数学可行区间验证：Split 必须精确按整数比换算，Consolidate 只允许每桶 floor 或非零余数对应 ceiling；不在本片重排 seed 或生成政策。新红例 before 4/4 二并一、after 3/1（总量仍4）此前被汇总守恒误放行，`partition-red.log` 记录13例中此例单红；该测试阻止把别的桶库存挪给回购 plan。

## 冻结验证与交接

- 叠 R1 红/绿 `fb0b5135` / `48cd9c50`，本树对应 `fcb641fb` / `eda680ad`；R3a 先红 `af9af3ad`。
- 工件独立存于主仓 `.tmp/share-transform-r3a/`，target 不共享。长 build 使用 `CARGO_BUILD_JOBS=16`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`，进程外 `runBoundedCommand` 共享 300000ms deadline。首红冷 build 1m58，绿 build 曾4m07（含 package cache lock），后续 warm 32.74s/1m39/27.77s/37.93s；未超过 deadline。已有 Rust monolithic lib 前端阶段无法按本片拆 crate；编译明确16 jobs，已观察 rustc 11 threads，不静默降并发。
- 普通 binary 每条命令外 10000ms，5个互不冲突过滤组并发、每组 `--test-threads=4`：R3a **13/13**（0.02s）、share_split **8/8**（1.11s）、share_registry **30/30**（0.01s）、cash_dividend_tax **24/24**（3.02s）、R1 registry_day_end **7/7**（0.00s）。日志 `final-0.log` 至 `final-4.log`。
- `git diff --check` 通过。未跑全量、未生产接线、未 push/merge；成本真实入账、mixed-calendar、R4 日内相位、发行总0及碎股政策均未冒称验收。
- 本树只冻结候选实现，独立未实施者完整 diff 复核由 root 安排；未复核前不宣称本片完成。
