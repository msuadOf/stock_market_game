# CompanySystem 与 GameSession 接入

本轮接通 `company/simple`，`company/simulation` 由用户在后续独立分支实现。`GameSession` 不在 Simple 内运行旧 `CompanyOperations`，也不为了初始化发行人创建 `Books`、`ContractBook`、客户或占位现金。最新蓝图要求 Simple 提供完整共同财务及股本行为，允许模型内部采用汇总账簿，省略细粒度客户与经营网络，不省略结算能力。

## 权威状态与边界

- `SessionSetup.company_system` 必填，所有金额与生成参数显式指定；用户已允许可编辑的 seeded 虚拟预设，不以股价反推经营规模。`Simulation` 当前明确拒绝，不降级到 Simple。
- `CompanySystem` 同时提供唯一发行人身份和选中实现状态；Session 不另存一份财务注册表。tick candidate 共享 immutable Arc，日终在 candidate 上推进公司状态，失败整体回滚。
- `SimpleCompanyConfig.kind` 必填，发行人 `CompanySpec.kind` 按同一公司 ID 精确装配，不依据证券代码、交易所或上市板块猜测行业，不将银行、保险、地产静默改为 Industrial。缺失或重复公司配置、尚未实现的 Simulation 在装配时显式拒绝；恢复使用同一装配规则核对身份。FinanceState 的种类、行业科目表及报告列报由共同财务模块保持一致。
- `SaveSlot.company_system` 保存继续运行所需的状态；删除旧公司经营、封账、顶层集团与到期镜像字段。恢复验证配置、发行映射、模式与日期，不迁移、不补字段。
- 仅公开材料进入玩家查询；NPC 仍仅依据本人获知的材料判断。发行人种类和总股本来自共同身份，不从经营账套读取。
- 前史截止开局前一日；月度生成是模型事实，正式公开仍遵守唯一 `ReportFrequency` 的期间及排期。来源说明标为简化生成，完整三表依据汇总财务事实形成，不捏造客户或研发经历。
- 用户进一步明确 Simple 是按上次结算状态的百分比更新账面展示，不追踪公司真实资金去向，不设每月 `SyntheticFunding` 金额；分红也不以公司展示 cash 充当实际资金预算。Common 股本行为仍依据合法方案、股份登记和真实投资者结算；回购不伪造成交，配股／增发不为投资者补认购款。相关模型状态与接口由共同财务模块同步，不把旧资金提案当作当前要求。

## 短测试门禁

先写四个 Session 层测试：明确模式、拒绝 Simulation、无旧仿真字段、休市日推进及恢复连续性。host51 首轮四项有效失败，整命令 0.25 秒。host60 八项边界测试六绿二红，0.29 秒；两个有效失败分别为 Monthly 盘中内存 checkpoint 被昨日披露游标拒绝，以及 Simple 加载错误模型的公开报告来源仍被接受。

修复复用真实交易时钟算法，Monthly 盘中只接受已经提交 tick 对应的精确披露时点，最后一个市场 tick 为 15:00。没有开始当日交易时仍要求上一成功自然日日终游标；公共 `ProtocolSession::restore` 继续拒绝盘中 checkpoint，不由放宽存档门禁解决。公开报告必须是 `SimpleGenerated` 且属于实际发行人，公告同样不得引用未知发行人；更正原版和结果保持来源一致。原八项断言全部保留，Monthly 测试另保三个未来游标拒绝负例；第九项用两 tick、零 NPC 的代表场景验证两种报告频率在开局、收盘前后及休市日日结的内存／公共加载边界。一次性 `gpt-6-luna medium` 静态复核未发现确认缺陷；host62 当前 Engine 的九项定向短测全部通过，8.99 秒，日志为 `.tmp/checklist-wave4/host62-session.log`。未执行完整回归。

为避免接近十秒硬上限后受负载影响超时，本模块 fixture 再缩为零 NPC、十二个历史结算周期；仍有 2029 年实际季度和半年度公开材料供来源篡改负例。第九项只验证时钟及存档边界，独立使用零前史；所有断言保留，不修改其他模块依赖的二十四周期 shared macro，也不放宽测试 deadline。host63 fresh Engine 已将缩量后的九项全部执行通过，0.90 秒，日志为 `.tmp/checklist-wave4/host63-session.log`；root 的聚焦批次显式使用 Rayon 4、测试线程 16，并受十秒外部进程 deadline 监督。该结果覆盖本模块的 selected system、严格配置及来源恢复、Monthly 盘中内存 checkpoint、三个未来游标负例和交易／休市公共档边界，不代表全部 checklist、股本行为或完整回归通过。

发行人种类补充测试先在 host69 取得两个有效失败：Bank 被装配为 Industrial，缺失／重复配置仍被接受；证据为 `.tmp/checklist-wave4/host69-real-kind-red.log`。后续精准修复种类装配并新增真实 Session 的 Bank、Insurance、RealEstate 初始化／公开报告政策／恢复往返测试，使用各行业合法期初 cash 科目。host70 fresh Engine `engine-c8042d8b049f99aa` 已将本模块十二项全部执行通过，0.96 秒，日志为 `.tmp/checklist-wave4/host70-simple-short.log`；原九项恢复／来源／游标／周末公共档边界也继续通过。该证据不涉及用户后续独立实现的 Simulation，不将虚拟统一环境分组 `listed-simple` 冒称真实行业分类。

## 跨模块协作

Core 实现负责 `CompanySystem`、Simple 算法、严格恢复；information 实现负责共用报告载荷、公开排期、个人材料与估值。Session 接入负责 CivilClock、候选状态、保存恢复、hash、clone 和 commit。宿主新局及 schema 消费者需同步当前严格契约，不能仅引入新 DTO 后保留旧生产路径。

旧完整财务测试不再访问 Simple Session 的影子 `CompanyOperations`。工商账套、前史和 ClosingEngine 的断言保留在独立低层 fixture；公告消费者测试先由真实独立 `CompanyOperations`、`CivilClock`、`CompanyOperationsClockWiring` 和 `DisclosureDispatch` 形成公开材料，再验证 Session 的个人资格、获知时点、真实事件路由、公开索引、账户不变及恢复。该证据不证明 Simple 会生成具体客户或真实合同事件。原 `direct_save_slot_rejects_missing_consolidated_parent_income` 迁为 `consolidated_closing_state_rejects_missing_parent_income`，保留缺失归母利润的拒绝断言，不在 Simple 存档中伪造集团账套。

现有 `information_revaluation_failure_rolls_back_entire_day_end` 进一步改为真实月末休市日：一个机构账户关注发行人，删除原本已经公开的 Annual 材料以注入既存信息不变量故障。公司月末收入、所得税和闭账候选形成后，个人重估失败须保持整份 CompanySystem、随机状态、账簿、时钟、公开库和完整 save/hash 不变；恢复原公开库后实际重试，验证同日收入摘要、税计提和月报闭账版本各仅一次。正常无历史数据的不可用结果不等同于该故障注入，不将合法 `Unavailable` 变为错误。

旧完整经营路径曾利用 `CivilClock.next_due_seq` 耗尽导致新增债务调度失败；Simple 没有该经营 due 调度器，因此该旧驱动不被保留，也不为测试制造假到期业务。旧固定资产折旧的金额与断言仍在独立低层财务测试中；Simple 的跨层事务覆盖改用上述真实后置信息失败。此新增验证仍待 root fresh 编译后的单条短测与独立复核，不由先前十二项绿灯替代。
