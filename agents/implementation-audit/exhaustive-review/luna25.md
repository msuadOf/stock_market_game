# luna25：Task 17/19/2 历史复核与生产调用追踪

## 范围与证据纪律

审计产品基线 `08e4fc7`（当前审计树 HEAD `a7c7ce3` 是将其合入审计分支的 merge commit；产品文件来自 `08e4fc7`）。连续读至 EOF 的三份材料如下；`wc -l` 与全文读取结果相符。本轮不改产品文件、不做 Git 写操作、不运行测试。

| 文件 | 行数 | 连续全文覆盖 |
| --- | ---: | --- |
| `.omo/evidence/company-information-npc-intentions/task-17-review.md` | 110 | §0–§7；diff 范围、K5 数学契约、方法链接/恢复/RNG、最小性、边界、偏离、验证日志及非阻塞项 |
| `.omo/evidence/company-information-npc-intentions/task-19-review.md` | 93 | §0–§5；8项命令/环境记录、技术指标与价格记忆、边界、TS绑定、五项裁决、残余风险 |
| `.omo/evidence/company-information-npc-intentions/task-2-review.md` | 26 | 法源忠实度、blocked证据、K1/K4、ADR、测试质量、三项非阻塞发现 |

合计229行完整读取。以下生产核对是静态源码追踪，不把历史 `APPROVE` 或当时测试数字冒充当前运行验证。

## Task 17 原文逐章复核

| 原文章节 | 当前调用/代码 | 复核判断 |
| --- | --- | --- |
| §0（1–21）：7文件全新增，工厂和类型未接Session | 当前 `packages/engine/src/session.rs` 已用独立 derived stream 生成分析档案并创建机构 `BeliefBook`；`session/decision_chain/roots.rs` 取档案权重并组合候选信号 | 原审查所述“当时没有会话调用点”仅对当时 commit 成立；不能延伸为当前没有调用。 |
| §1.1–1.4（27–45）：13风格权重表、非零权重抽样、最大余数与金样 | `strategy/factory_profiles.rs` 仍保留权重表、采样和归一；`analysis_profile.rs` 保留权重不变量 | 本次没有发现表、整数单位或声明顺序漂移。没有复跑历史 gold 测试。 |
| §1.5–1.7（47–62）：方法/行业链接、持久化拒绝、无共同 V | `analysis_profile.rs` 仍执行方法与基本面权重互斥校验；`strategy/beliefs.rs` 按个人档案更新预测；当前候选 `fundamental_signal` 消费个人条目 | 未见本批重新引入共同 V。历史方法链接裁定仍针对模型层，不能证明每种 profile 均有生产消费者。 |
| §1.8（64–67）：当时无 Session RNG 调用、旧回放锚点稳定 | 当前装配使用按 `AccountId` 派生的 `analysis-profile` 与 `belief-assumptions` 独立随机流，机构执行 policy 又有自己的派生流 | 历史的“完全没有调用”已被后续接线替代；仍符合随机流隔离方向。本文不主张跨worker次序必然可复现，也没有运行回放。 |
| §2–§4（73–89）：最小范围、边界覆盖、登记偏离 | 原结论系 `02e484d` 范围；随后工厂能力接入 Session。当前 Retail 路径与机构 `BeliefBook` 路径不同 | 历史任务的必要性判断不覆盖后续完整产品链。Retail 是否应消费 K5 五路分析需按正式功能范围另审，不能由13行工厂表本身认定已接通。 |
| §5–§7（91–110）：测试计数、三项观察、APPROVE | 历史计数按该提交及当时 HEAD 自洽；当前无运行验证 | 没有将历史计数作为当前通过证据。`expect` 诊断精度和采样助手重复是原审查已接受的非阻塞点，本轮未发现其成为新语义问题。 |

## Task 19 原文逐章复核

| 原文章节 | 当前调用/代码 | 复核判断 |
| --- | --- | --- |
| §0（14–29）：33/649测试、Tauri缺构建产物、worktree被移除 | 都是当时验证环境事实；与本次静态审计产品状态无冲突 | 不移植旧环境错误为当前产品缺陷，也不把旧测试结果报告为本轮通过。 |
| §1（31–51）：SMA/RSI/ATR、已完成日K、无成交日/未来边界 | `session/decision_chain.rs` 对 `candle_book.histories()` 按股票并行构造技术观测；`roots.rs` 在技术方向只使用 SMA20/SMA60/RSI14，ATR仍不作方向信号 | 技术核及其生产数据入口存在；ATR仅风险/执行的原限制仍保持。无A股交易制度规则改变证据。 |
| §1 价格记忆（53–56）：亲历锚点与公开读取区分、protected=held∪active、额外8条 | `roots.rs` 对候选股票调用 `observe_price`，末尾构造 `held ∪ plans.active_codes(id)`，但只对 `watchlist` 调 `prune`；全仓生产源码没有 `price_memory.prune` 或 `record_public_history_read` 调用。`PersonalPriceMemory::prune` 本身有纯状态测试。 | 原文把上限明确当作契约接缝。当前实现只接入了观察，未接入容量修剪或公开历史读取留痕，见新候选 L25-01/L25-02。 |
| §1 分层与 §2（57–70）：纯内核/观测/记忆分层、范围最小 | `strategy/technical.rs`、`observation/technical.rs`、`experience/price_memory.rs` 仍分层；当前技术指标全市场行情历史预构造后传给候选评估 | 观察层没有被信念/公司账本污染；但“本人读取公开历史”的事件没有跟到个人状态，不能仅凭技术信号可用说读取记忆已落实。 |
| §3（72–77）：TS绑定波次债 | 目前生成目录中能找到 `PersonalPriceMemory`、`StockPriceMemory` 类型，Web memory schema 有对应解析 | 原两绑定未提交的观察已不能当当前缺失结论；本文未跑生成器或 types 检查。 |
| §3–§5（79–93）：技术边界、五项偏离裁决、APPROVE | 历史测试与偏离裁决仅对 `b1f638d`；当前容量/读记忆 caller 需看后续Session入口 | 原 review 的纯内核边界覆盖结论仍可与生产调用缺口并存，不构成反证。 |

## Task 2 法源与旧结论

| 原文范围 | 当前证据 | 复核判断 |
| --- | --- | --- |
| 第8–12行：11个官方来源、CAS/CSRC/交易所条款核对 | `docs/company-accounting.md` 引用 `packages/engine/tests/fixtures/company-model/policy-sources.json`；fixture仍保留来源状态、适用范围和取证信息，官方来源锚可定位 | 本轮未联网重取官方正文，不复述“当前重新核验”结论。应把旧 review 的来源核验当历史证据。已标 `blocked` 的CAS28/31/部分旧准则仍清晰登记，不能用已核条款数掩盖这些法源债。 |
| 第14–15行：受阻来源诚实登记 | fixture 对受阻来源保留 `status=blocked` 与原因/检索记录；正式文档对部分行显示未核验/不支持 | blocked诚实性旧结论仍有当前本地证据支撑，没有把未取得原文伪装成已核验。 |
| 第17–19行：K1/K4、ADR与结构测试 | 正式文档仍按法规生效日期与游戏假设分别叙述，policy manifest继续作为机器可读证据台账 | 此份历史审查本身不宣称运行时消费整个法规清单；当前业务适用范围以各公司模型和正式文档为准。 |
| 第22行：年报日期算术 typo 应从4-27修成3-27 | `docs/company-accounting.md` 已改为3-27；但 `policy-sources.json` `game-assumption-report-schedule` 的 `game_assumptions` 仍写 `3-20+7=最晚4-27`（同项又声明最晚3-27） | **旧结论只修了一半：fixture自然语言仍有算术笔误，与正式文档自相矛盾。** `policy_manifest.rs` 的结构验证不校验这段算式。属于政策证据/文档一致性债，不改变年报≤4月30日的结论。 |
| 第23行：部分通知“废止”与“不再执行”措辞差 | 当前正式文档个别法规概述仍使用“废止”；旧review把它作为 wording nit | 未重新取得每份通知逐字核对，保持为已登记待斟酌的措辞债，不升格为规则适用错误。 |
| 第24行：CAS33总结把统一政策/期间压缩到§26 | 当前 fixture 仍有压缩表述风险 | 历史非阻塞观察尚未由这轮检查证明已修；不把它升级为新的 A 股/会计处理逻辑缺陷。 |

## 新候选与反证

### L25-01：价格记忆未执行“受保护集 + 8”修剪

计划契约原文在 `docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md` 第135行明确个人价格记忆上限为持仓+8未持仓；task-19-review.md 第53–56行进一步规定受保护集合为 `held ∪ active plan`。当前 `roots.rs` 对根候选逐项推进 `price_memory.observe_price`，其后只见 `watchlist.prune(&protected)`。`PersonalPriceMemory::prune` 仅在实现自身及直接单测中出现。恢复校验 `session/persistence.rs` 仅限制 `memory.stock_count() <= setup.stocks.len()+8`，且逐项要求 code 属于配置股票；这最多将总键数限制到整个市场规模，不能实现每账户“持仓∪活跃计划永久保留，其余至多8只”。

反证：watchlist和price memory是两个独立成员；watchlist成功修剪不会删除另一个容器条目。保存期全市场股票数+8边界也不是每账户保护集边界。该发现与之前 `sweep25.md` 的 S25-01 是同一问题，本记录独立复核予以确认，不重复编号；未构造会话运行复现，故仅报告静态调用链缺口。

### L25-02：公开历史读取留痕没有生产 caller

`record_public_history_read` 在 `price_memory.rs` 只有API实现和单测；生产源码没有调用。与此同时，`decision_chain.rs` 在逐账户评估前对全市场 `candle_book` 并行生成技术指标，再把同一股票的指标交给候选根使用。由此技术历史信号可能参与个人分析，但 `last_public_history_read_minute`、计数与 `last_touched_minute` 不会记录这次分析读取；即使补接也应只记实际进入个人分析的股票/事件，不能将全市场预计算误记为每个NPC主动读取。task-19的“读取刷新 recency、公开历史不冒充亲历”目前只有纯API测试，没有端到端 caller。

适用边界：如果正式产品决定公共技术观测是系统统一提供而非NPC主动查阅，则应修正该记忆功能契约/文档；若仍承诺“主动读取需要记录”，则需在个人分析的消费边界记录，而不是共享预计算阶段。本文不替产品作新规则裁决。

## 结论

Task 17历史纯工厂审查大体与当前数学实现相符，但其“尚无Session调用”只描述当时版本。Task 19已接入个人观察和技术指标生产链，却没有价格记忆修剪/主动读取留痕 caller；L25-01与已存在的S25-01一致，L25-02是独立调用缺口。Task 2来源核验结论本轮不重做在线法源核对，已登记blocked债仍诚实；其年报日期typo正式文档已修、机器fixture仍留4-27错误，构成L25-03文证不一致。没有发现改变A股撮合、结算、交易单位或真实市场规则的代码变更。本轮没有运行测试。
