# 付款三门 tests-only 独立源码审查

日期：2026-10-29。Reviewer：未参与本批实现的原生 subagent。结论：**APPROVED_TESTS_ONLY_ADMISSION**。

本结论只允许进入实际编译与指定业务红验证；不表示已编译、已取得真实红、满足 10000ms 时限或付款生产已经实现。本审未执行 Cargo、编译、target 操作或修改产品文件，仅新增本报告。

## 冻结范围与核对

实际 HEAD：`4ad5a291acd6cc72e2981de2b66115b9b1abc715`。

完整读取两个文件及完整 patch，亲自核对 SHA-256 与 `payment-three-red-source-01-fingerprints.json`：

| 对象 | SHA-256 |
| --- | --- |
| `packages/engine/src/session/ordinary_tax_day_end.rs` | `ecad092789c498c068db42d4beee126b27a54704ec5571d22801c514aa6b45a1` |
| `packages/engine/src/session/ordinary_tax_day_end/payment_runtime_tests.rs` | `81a21a6d8a9fbf2dbc6e062fa76f27010c5a60c578a9142f9e4f610deae34bca` |
| `payment-three-red-source-01.patch` | `e584d4c089767b5bf0695e67a77637b62f02d92953ce8036b6334802286a0385` |

父文件仅追加 native test module 注册；新文件包含三个真实 GameSession 行为测试和共用 fixture。无付款生产改动。已读本树 AGENTS、ROLE、principles、testing、architecture、open-questions，以及相关 ADR-0031、ADR-0040；本轮未变更交易制度。

## fixture 与真实接口

- `payment_session` 使用实际 `setup_fixture`、Simple 配置与公共构造器。1200 股全流通按六名 Retail 等分，各 200 股；玩家初始无股，不属于登记权益。零 arrival、无其他 NPC 类型与无公司偏好配置控制额外自主委托。
- 17 个真实月度 prehistory 从 2029-01 起覆盖 2029 完整自然年。核对 `SimpleFundamentals::create`、`latest_complete_fiscal_year`、`distributable_profit`、`declare_dividend`：不删除年度利润或法定公积金检查，批准使用自动装配的真实注册资本事实。
- 方案日期经本局 Calendar 的 `next_trading_day` 推导，随后由真实 `CashDividendPlan::new` 校验。fixture 只添加批准来源和未登记、未付款账簿，再走完整公共 `GameSession::restore`；未手工伪造 Registered、付款、税账所得或成交确认。
- `restore_with_three_groups` 先完成 strict restore，确认 lazy runtime 尚未建立，再只配置运行资源 `runtime_group_count=3`。第一拍后核对真实 Owner `(1,0)→AccountId(3)`、`(2,0)→AccountId(6)` 与 tail `(2,1)→None`，没有将 padding 当账户。
- 登记来自 6 月 3 日真实 market step 与 ordinary civil 日结；付款现金基线在 6 月 4 日 market step 之后取得，排除了撮合收支、交易费用与日内现金变化对付款 delta 的混入。
- 03 在 runtime 已存在时调用 `seed_order_for_test`，实际分流到 `RuntimeDriver::inject_player_intents`，按 Owner 投递 `ExternalPlan`。公开 `enqueue_player_intent` 的买单经 `take_runtime_player_intents` 和同一真实注入路径进入 runtime。`GroupIn::ExternalPlan` 使用 `external_pending.extend(ops)`，两次调用不会相互覆盖；卖方与买方同属 group 0，缓冲内先卖后买，符合 maker=seller、taker=buyer 的断言。随后通过真实 Event::Trade、持仓与最终 personal confirmation 固定成交，未直接修改持仓或伪造税务处置。
- 已核对被引用的可见性、方法与字段，包括 test-only CompanyState 修改器、`payment_by_id`、`successful_payments`、`outstanding_tax`、TaxCollectionReceipt 各现金字段及 include 相对路径。静态检查未发现编译阻断；实际构建仍是独立必要证据。

## 三个预期首红

实际 civil 路径调用 `prepare_ordinary_tax_day_end`；当前该方法只准备真实 REG/个人税日与登记，不执行付款。已有 `distributions_day_end` 仍是候选逻辑，不能当作真实账户到账。由此预计以下首个付款 cash assertion 红，仍必须以实际运行确认。

| case | 预期实际 delta | 应有 delta | 必须出现的 panic marker |
| --- | --- | --- | --- |
| 01 | 0 分 | 200 分 | `R4 付款日必须真实增加持有人现金，不能只保持 Registered` |
| 02 | 0 分 | 180 分 | `R4 简税付款必须在真实账户留下税后净额` |
| 03 | 0 分 | 180 分 | `R4 真实转让后的付款必须按原登记权益收缴个人税` |

与 `payment-three-red-cases.json` 一致。02 已先断实际现金，再断 receipt；03 已先证明真实转让，再取得付款前现金。源码未发现更早 fixture 断言必然失败的原因。若实际运行停在 restore、Calendar、成交或其他 marker，不能计作本三门目标红，也不能改弱原行为断言掩盖原因。

## AGENTS 三问

1. **是否符合大 A 语义、依据是否可靠：本轮 tests-only 通过。** 亲自读取根 `agents/branch-completion/tax-basis-sources/85-correct-fgk.txt` 与 `101-correct-fgk.txt` 官方全文原件。财税〔2012〕85 号第三条规定 FIFO 与日终净增减，第八条规定自然年月；税率与暂扣以自 2015-09-08 施行的财税〔2015〕101 号第一、二、三条更新。官方链接分别为 [85 号](https://fgk.chinatax.gov.cn/zcfgk/c102416/c5204415/content.html)、[101 号](https://fgk.chinatax.gov.cn/zcfgk/c102416/c5203902/content.html)。以本轮审查日 2026-10-29 的既有现行口径审查；2030 日期是模拟日历 fixture，不声称预知未来法令。01 未出售者应收 gross、不在付款时按固定比例扣个人税；03 初始 lot 来源于自动装配的 2030-06-02 分配，6 月 4 日售出 100 股属于一个月内，税额为 `100×1分×20%=20分`，原登记 200 股权益仍为 gross 200 分，付款净增 180 分；新买方无原登记权益。02 按 ADR-0040 的 FlatWithholding 游戏简化配置 1000bp，gross 200 分扣 20 分，净 180 分且不创建个人 FIFO 税账，未冒充法定统一 10% 税率。
2. **是否必要、保持最小范围：通过。** 两个文件只为真实付款来源取得测试红，复用现有真实构造、恢复、Calendar、runtime 路由与成交确认。重设三组仅为 test fixture 资源配置，没有新增生产 API、资金算法或税公式。
3. **是否遗漏边界、跨层漂移或不必要复杂度：未发现阻断本批源码准入的问题。** 三门包含个人无处置 gross、简税净额与逐账户 receipt、个人真实处置后税收缴，并包含 01 付款后公共 save/restore 的不重复支付检查。尚不覆盖付款阶段/后组/inspect 拒绝的全域回滚、部分收缴或余额不足、混合 INFO/资金事务；这些是后继生产与独立边界门，不能由本批正向测试替代。

## 有效发现与证据限制

本轮无须先修复的有效源码发现。没有弱化断言或扩大修改范围。实际 Cargo 编译、三门指定 marker 红及单 case/整命令 10000ms 外部 deadline 尚未执行；完成这些之前，状态保持 tests-only 源码准入，不写为已 red 或生产完成。
