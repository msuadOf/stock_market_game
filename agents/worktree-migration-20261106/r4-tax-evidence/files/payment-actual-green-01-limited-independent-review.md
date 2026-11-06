# 付款 cash01..04 实际结果与正式文档有限独审

日期：2026-10-29。裁决：**APPROVED_PAYMENT_CASH4_LIMITED_ACTUAL_AND_DOCS**。

本票承接本主题的 [生产02完整源码审查](payment-production-02-independent-review.md)、[生产03 raw回滚增量审查](payment-production-03-delta-independent-review.md) 与 [cash01 Calendar fixture审查](payment-cash01-calendar-fixture-01-independent-review.md)。本次仅有限复核四个付款 exact 的实际原件、对应测试源码绑定与强尾，并审查两处正式文档状态更新。未重审或重签全部16项、所有公司行为、Host、WASM/Browser 或整树；未运行 Cargo/测试，未读取 target 或计算产物大 hash，只写本报告。

## 实际批次与小原件

实际目录：`/data1/<USER>/workplace/stock_market_game/agents/branch-completion/joint-tax16-r2-2026-10-29`。

本审实际读取 `actual-tool-wait.json`、`actual-wait-cleanup-inventory.json`、`launcher-receipt.json` 和四项付款 stdout/stderr。绑定为 session `48124`、WAIT exit `0`、chunk `0aeadf`，共享 wall `111.3062693089887s`。小原件 SHA-256 已实际核对：

| 原件 | SHA-256 |
|------|---------|
| `actual-wait-cleanup-inventory.json` | `ce62f654e6851d0b6cc6fea35119e63a24e1dbf502d0445c4d2e3f8fc4d56cd0` |
| `launcher-receipt.json` | `493494f10bc4dd21449ac1231c5fdcd1a28afe8af5bd5a38a4d5d0bdf6b49458` |
| `actual-plan.json` | `e526fc98f1f60d8e58c991d4a003ab9cafce1dbba80dfc42685a81b5051750ff` |

actual plan 绑定固定验证镜像 `99dc256ea4a944f4c1635172e14b78e3c39728eb`、tree `226de8def03ac7955b667691d3352cf6a410330e`、clean，cwd 为 `.worktree/development-validation-2026-10-29`。本票只承接既有 source/cfg 与全16项独审，不重算1183项源码或大制品。实际 runner 记录 j8、INC0、pool8，四项均以10000ms外部监督、完整 exact、`--test-threads=1 --nocapture` 调用本批同一 unit。

inventory 的 source/runner 未变记录均为 true，remaining owned identities、exact argv、Cargo/rustc 均空，scratch 不存在。launcher 记录 guard exit0、唯一成功 build-finished、compilerErrors为空及source/artifacts未变。本次未独立重查其余12项或全部清场实现；本票不借整批汇总计数代替下面四项原件。

## 四项实际结果与强尾

四份 stdout 均只有一次 `running 1 test` 和一次 `test result: ok. 1 passed; 0 failed`，完整 exact 名称匹配；四份 stderr 均已亲读，只有既有 INFO PROBE，没有 panic 或失败。各项 launcher 原件均为 `exit: 0`、`wrapperExit: 0`、`expectedBehaviorObserved: true`，且没有 wrapper报告child101。当前原件没有另立 structured child-status，本票不补造独立 child receipt。

| exact 尾名 | wrapper wall | 有限实际结论 |
|------------|--------------|--------------|
| `r4_cash_01_payable_credits_gross_and_activates_personal_income_once` | `1.069067s` | 真实税前现金到账、付款与个人税所得激活；strict restore后续行不重复付款 |
| `r4_cash_02_flat_payment_credits_net_and_records_real_withholding` | `0.720083s` | 真实账户净到账与简税withholding回执相符 |
| `r4_cash_03_actual_sale_before_payment_collects_personal_tax` | `0.728136s` | 真实转让后按原登记权益付款并收缴个人税，登记后买入者不获旧权益 |
| `r4_cash_04_final_projection_rejection_aborts_payment_and_company_candidates` | `0.741924s` | prepared shadow已有+200 cash及`Paid`后拒绝最终投影，原业务/公司/账户/cache/source与真实raw账户回滚对照通过，World健康 |

测试文件来源只核必要小绑定：actual plan与source-manifest的对应单行均为 `packages/engine/src/session/ordinary_tax_day_end/payment_runtime_tests.rs`、20905 bytes、SHA `71f794cbe2067e26e4e2bf463da8c72ce08fffb7b955c5af72def2fedad414c7`，与原fixture准入票一致；本审从上述固定镜像commit读取该文件并核同SHA，也与本主题当前测试文件一致。未遍历或重hash全部source。

cash01 的 stderr 明确出现2030-06-05及06-06的civil PROBE。结合已审同SHA源码和整case成功，可确认休市日按冻结SSE Calendar仅走真实自然日日结，逐日报告及tick/day不变断言实际通过；到达下一交易日后真实step与日结通过。原尾部账户现金相等、gross receipts相等、payments相等均保留并已执行通过，没有把旧日期错误提前退出当作恢复强尾通过。

cash04 的同SHA源码包含实际 `driver.world_accounts_snapshot()` 前后完整相等断言，且事务前的driver guard在进入事务前已释放。本次成功已覆盖该断言；World普通Abort的健康状态不扩张为Session可重试，原已收盘civil poison策略仍保留。case04仍是实现后回归保护，不追记为历史TDD先红。

原 Tax16 的15green+1cash01 Calendar fixture FAIL保留；本票记录新固定来源的四项成功，不回写旧失败。Tax01..05、其他12项、部分收缴/purse/多方案/transport等未被这四项覆盖的组合不据此新增通过结论。

## 正式文档窄票

文档审查树为 `/data1/<USER>/workplace/stock_market_game/.worktree/r4-tax-production-2026-10-29`，基线 HEAD `94ea2896781356b9c5f6f729255d22738f706b6d`。本审核对 [payment-actual-green-docs-01.patch](payment-actual-green-docs-01.patch) 与 [fingerprints](payment-actual-green-docs-01-fingerprints.json)：只有 `docs/trading-rules.md`，文件 SHA `b0f8fbd75ee07020a67a0eb904954ee43f4e55dfacd1938eaefa17b644ea9c96`，patch SHA `46efcb997c62036d269c94caf09812d5e8df0738b1c5c27152decfaa6c0cecaa`，完整live diff与patch逐字节一致，`git diff --check`无输出。

两处更新移除本轮“尚待绿测”的旧状态，明确已验证 native现金付款、简税代扣、真实转让后个人收缴、恢复续行不重付及最终投影拒绝回滚，与上述四项原件相符。仍保留`Paid`等为候选且须真实组回执勾稽、完整内部Commit后固定尾安装的限制；WASM与完整送转链未完成的边界未删除。没有新产品改动或扩大税务能力。

AGENTS三问限定：领域规则与依据沿前述已审产品及Calendar票，本次不新增A股制度；文档只消除已被新实际结果关闭的两句滞后状态，范围最小；四项实际与旧强尾、raw回滚来源匹配，未发现本次有限范围的阻断遗漏或跨层漂移。本票准予文档净片接收，并关闭四个付款exact在该固定镜像上的待运行边界，**不宣称全部公司行为、WASM或整树验收完成**。
