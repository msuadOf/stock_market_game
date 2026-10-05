# Q23：所得税年度基准与差额计提

## 用户回答与本批范围

同一公司、同一核算期间的同一次处理不得重复生效；新期间可继续计提，已有结果变化
以差额登记，失败允许重试。财报默认季度、可选月度，财报周期不等于纳税年度，不能
每次发布财报都重复收取一份全年税。实现范围包含四行业所得税年度基准、跨年差额、
单体／固定集团原子更正、严格保存，以及 `SessionSetup.report_frequency` 月／季配置；
排期与跨宿主入口分别收口，不能用初期年度处理器的完成代替全部 Q23 完成。

## 实现

- 首批单年度基准在后续级联实现中扩为 `IncomeTaxPosition.assessments` 年度链，
  并保留 `loss_pool`、`initial_deferred_tax_asset` 与 `restatements`；均为严格必填。
  旧顶层 `loss_pool` 数组与单 `assessment` 对象明确拒绝，不迁移、不默认补齐、
  不新增 schema 版本。跨年度实现与本批验证见 [级联记录](q23-tax-cascade.md)。
- 同年每次从 `opening_loss_pool` 计算全年已入账业务的税前利润与累计税额，排除
  `6801` 本身；不以已经扣减过的本年期末亏损池反复抵扣。
- `IncomeTaxOutcome.current_tax` 为请求年度累计评估税额，`current_tax_delta` 为
  本次包含受影响后续年度的当期税差额合计；递延税按各年目标运动差额处理。
  正负差额均用合法正金额借贷分录表达。
- 已支付税后下调税额形成应交税净借方，不自动发放退款。新增年度从上一年度
  期末池起算。所有计算与过账成功后才提交评估、亏损池与事件身份。
- 保存恢复验证政策、亏损起源年份及正金额、年度基准计算与期末池／税额的一致性。
  Web parser 同步严格字段，不把完整机制藏在只能由 Rust 发现的不完整结构后。
- 生产 `settle_period_end` 已直接调用该处理器；仍采用年末评估、每日尝试缴税及
  真实资金不足保留债务的既有游戏节奏，不增加客户／投资者补钱。
  四行业共用 `company::income_tax` 的纯候选、严格年度状态与 `IncomeTaxOutcome`，
  各行业 Owner 保存显式政策并绑定真实开局 DTA，不复制四套亏损池算法。
  `IndustryBooks` 统一分派税状态、付款和 Owner 校验；通用原子更正共用
  `company::report_correction`，工商专属包装也委托它，不保留第二套组合算法。
  贷款、保险合同、地产项目等专属科目仍须结构化事实，不能以纯 Journal 改账绕过
  子账所有权；该边界明确拒绝，不作为所有结构化业务更正已完备的证据。
- `GameSession` 更正队列只在内存持有，经营日结后、月／年封账前使用真实
  `CivilDayEndReport.disclosure_instant` 完成更正及披露。整个日结失败则公司业务、
  单体／集团报告、公开库、队列与时钟全部回滚；公开取消入口允许删除仍待处理的
  错误请求，已完成操作只能通过新的更正调整。没有提前写入今日 18:00 的公开事实。
- `CompanyReportCorrection.operation_id` 在待处理和已完成集合中绑定精确输入；
  相同载荷幂等，不同载荷冲突。非序列化 `ReportCorrectionEpoch` 隔离换档请求。
  `SaveSlot.report_correction_operations` 严格保存成功日终的精确输入、真实公开 ID
  与完成时点；日内队列不保存，低层快照遇到待处理更正显式拒绝，不静默丢失。
  该成功事实不是日内 WAL，也没有任何格式兼容或版本迁移。

## 依据与明确简化

沿用 `docs/company-accounting.md` 的工商税务来源矩阵与全额确认亏损递延税游戏简化。
CAS 18 正文及现行企业所得税条款取证仍有登记缺口，本批不定稿新税率、扣除政策或
法定付款期限，也不把工程幂等性冒充完整税法合规。`TaxPolicy` 保持显式参数。

## 验证

- 先新增三个 Rust 行为测试，root 使用独立 10 秒进程 deadline 实际运行均红：
  重复亏损年税前利润从 -4.00 错变 -3.00；重复评估生成新事件；缴税后重评把
  税费又计入税前利润。日志在 `.tmp/checklist-wave2/*tax*-red.log`。
- Web 严格年度基准 case 同样先红，旧 parser 错要求顶层 `loss_pool`；实现后
  使用 `run-with-deadline.mjs 10000` 和 `--test-timeout=10000` 实际通过（0.25 秒）。
- 新增失败重评可重试、保存恢复幂等、新旧结构拒绝、损坏基准拒绝测试。
- root 的 build11 统一编译通过，7 个所得税相关 owner 短 case 与
  `industrial_accounting::tax_gold` 6 个金样实际绿。非作者完整 diff 复核发现首次
  基准可能含本年／未来亏损而产出不能自恢复的状态，已补显式原子拒绝与短 case；
  Web 年度基准正例也改为税率下自洽事实。非作者 `review_q23_income_tax` 再次完整
  静态复核通过；root 的 build13 使用 `--jobs 32` 刷新编译后，后补原子拒绝 case 在外部 10000ms deadline 下实际通过，执行 0.00 秒；此前 7 个税务状态 exact case 与 6 个税务金样也已实际通过。非作者对最新代码、文档和真实 fixture 税状态增量复核通过，不认证 Q07 日期能力。
- root 用实际 engine 新建、两日日结、保存恢复再保存的结果重新生成当前存档，
  `current-company-slice.json` 为其完整 `company_operations` 原样投影，JSON 实际
  对比相等。五家公司均保存真实 2029 年度评估；`C-000812` 保留 4367 个 Journal
  batch、672 个应收开项、1339 条往来身份和 2671 条库存来源，不截断账务事实。
  更新后 `company-books-schema.test.ts` 全部 9 个 case 实际通过（统一 10 秒
  进程 deadline 与 case timeout，整命令 0.73 秒）。
- host-build-8 当前二进制的 15 个年度级联／单体更正／税资产 owner exact case
  各自通过，执行 0.00–0.02 秒；以 15 个独立进程并发运行，每进程外 10000ms
  deadline 与 `RAYON_NUM_THREADS=8`，日志为 `.tmp/checklist-wave4/q23-short-*-green.log`。
- 日终外层第一版先写 prototype 后补测试，未满足测试先行，记录这一实际限制，
  不事后制造 stub 红冒充 TDD。host-build-10 的队列幂等／换 epoch 真实短测绿，
  两个日终成功 case 实际红：默认公司前史后现金已耗尽，补记费用被真实负现金
  守卫拒绝。现金守卫未改；fixture 改为明确漏记的已收现收入凭证，再验证实际日终
  过账和历史有效期列报，不以新增注资补救付款失败。下一产物仍需验证，不能宣称绿。
  非作者发现缺少公开取消与完成 ID 归属校验，已补 API 及 owner／Scope 守卫，
  再次静态复核和集团／月末真实矩阵尚待完成。
- host-build-15 的共享税核心七项及 Bank 五项、Insurance 七项、RealEstate 五项
  Owner 短测均实际通过；各独立进程外 10000ms deadline、`RAYON_NUM_THREADS=8`，
  执行 0.00–0.01 秒。host-build-16 更新通用候选和应用调用后，五个共享组合更正
  与十个真实日终／集团／保存恢复边界也实际通过（0.00–0.74 秒）；原工业 Owner
  十五项中十四项绿，另一个隐藏年度链负例因新的开局 DTA 守卫先返回而缺少空链
  上下文，断言不修改，保留两项强守卫并修正直接错误事实后仍待下一产物验证。
  日终外层包括真实月末封账前更正、最后集团公开失败整日回滚、公开取消后同 source
  重试、恢复后继续 root／child 更正、80%／20% 手算、私有 Original 实际序号及
  Consolidated 目标身份篡改拒绝；不把四种列报纯候选冒称全部真实业务子账更正。
  真实日志分别在 `.tmp/checklist-wave4/q23-{owners,estate,build15,build16}-*.log`。
- 比较覆盖复核发现实际一月开账、全年没有十二月交易的合法账簿被误拒；反向又可用
  当年凭证映射历史制造覆盖。三项测试先写，host17 仅取得错误 Annual 返回值解构的
  编译红，不算业务红；修正为真实 `close_year` 的 Annual 句柄后，host18 两个边界
  实际红、早期开账重述正控制绿。事实覆盖已改为真实日期与真实 `OpeningBalance`，
  不改有效期间金额或现金日期，不补未知历史零值；host19 三项 coverage 短测已实际全绿，
  非作者完整静态复核通过，随后十项真实日终／集团矩阵也再次全绿。
  host18 同时复验隐藏年度链负例已通过，未修改旧断言；共享空链守卫及 Opening DTA
  守卫均保留。日志为 `.tmp/checklist-wave4/q23-coverage-build18-*-actual.log`。
  host19 还复验六项地产完整 Owner 边界均绿，包括合法一致资产编辑、成熟与偿清借款；
  项目时序复核另发现开发前完工及中断顺序缺口，已先写两个新测试，host19 尚未编入，
  不把原六项绿色冒称新时序守卫完成。绿色日志为
  `.tmp/checklist-wave4/q23-build19-*.log` 与 `q23-outer-build19-*.log`。

- 历史旧 candidate（已由下文 P1 根修及新 Protocol 产物替代）：此前 `GameSession::save` 生成的两日日终 JSON 虽通过 Web strict schema 与 Engine restore/resave 等值，却因 `pending_npc` 含真实待执行买单而被 `ProtocolSession::restore` 拒绝；原始拒绝证据保留于 `.tmp/protocol-archive-restore-probe-old-candidate.log`。旧 candidate SHA 为 `ab3ff5d196400770e31cb00e4e1c60b08cb6d881b6087a1114aad74175368a6f`；未清除该事实或放宽 restore。
- 本次进一步由 root 提供日终 NPC P1 根修 release Engine（release 编译由 root 完成，JSON artifact `.tmp/checklist-wave4/day-end-npc-current-release-build.jsonl`）。更新后的 producer 使用真实 `ProtocolSession::new`、两组 60 帧 `step_frame` 与 `end_civil_day_update`、`ProtocolSession::save`、公共 `ProtocolSession::restore` 后 save 深度相等；随后在原会话与恢复会话各推进 3 个公共帧，验证连续 tick/sequence、真实 NPC `OrderAccepted` 和 receipt cursor 不回退，不比较并行执行后字节状态。producer 用授权 Engine rlib 和 build JSON 指定的匹配 serde_json rlib 编译，日志 `.tmp/current-save-fixture-generator-protocol-final-build.log`；直接 `RAYON_NUM_THREADS=32 node scripts/run-with-deadline.mjs 10000 -- .tmp/current-save-fixture-generator-protocol-final apps/web/src/save/fixtures/current-schema-save.json .tmp/current-schema-save-protocol-final.json` 进程树 exit 0，产物 native Public `ProtocolSession` restore/resave 验证通过。
- 新产物经 `parseStrictSaveEnvelope` 与 `validateDayEndArchive` 均通过；`snapshot.tick=120`、`current_date=2030-01-09`、`settled_through=2030-01-08`、`pending_npc.intents=[]`，另有 27 个 `history_reads`、96 条真实成交确认。完整存档 SHA-256 为 `f81dc44c86912666e9c78fbe2af541c7c36448d60c33ce2b82b9686442db43fb`；完整 `company_operations` 投影切片 SHA-256 为 `afa1ae92b0321f4c6e326b375a43064ac1787b858f13c5cebf36713559964a98`。文件已安装于两个 Web fixture 目标路径，严格解析/archive gate 日志 `.tmp/current-protocol-archive-validation.log`。
- 安装新产物后六个直接 fixture 消费者短测试仍 27/27 通过，单命令进程树 10 秒期限、每 case 10 秒、并发 4、耗时 4.79 秒，日志 `.tmp/current-fixture-protocol-consumer-tests.log`。后续真实 WASM Worker `restore_json` 跨层验收发现 `attention_probability` 在 Native/WASM 恢复重算差异 `0.03979332740595576` vs `0.03979332740595565`；原始失败日志 `.tmp/checklist-wave4/real-browser-wasm-day-end-current.log`，两隔离 Browser contexts 各 Rayon 2，Node suite 6.4455 秒、进程 wall-clock 6.5819 秒、exit 1；未到 IndexedDB、未手改存档或放宽守卫，故 Browser 持久化仍未通过。此差异由另一实施者报告，等根修后须重跑；不能以 native Protocol 和 Web strict parse 通过冒充跨平台 archive 完成。
- 随后的采样数学根修将 `1 - exp(x)` 改为数值稳定且数学等价的 `-exp_m1(x)`，无 restore 容差、seed 或默认值变更。root 以新源码完成 release build（`.tmp/checklist-wave4/observation-probability-current-release-build.jsonl`），我仅用该 build 的 Engine rlib 与 artifact 指定 serde_json rlib 重新链接 producer，编译日志 `.tmp/current-save-fixture-generator-probability-fixed-build.log`。原 seed／Setup 的 Protocol producer 运行继续执行两日日终、restore/save 深等、双方恢复前后各 3 公共 tick 的真实 NPC OrderAccepted 与 receipt cursor 单调断言；唯一外层 `node scripts/run-with-deadline.mjs 10000` runner exit 0，日志 `.tmp/current-schema-save-probability-fixed-run.log`。新 JSON 严格解析与 `validateDayEndArchive` 通过，tick 120、settled through 2030-01-08、`pending_npc.intents=[]`；新 save SHA-256 `5457be5b68eba83576a0a87f4f54b8123d119a252543b7e49721924977a5fcb1`，完整 company slice SHA-256 `afa1ae92b0321f4c6e326b375a43064ac1787b858f13c5cebf36713559964a98`，已安装至原 fixture 路径。严格/archive日志 `.tmp/current-protocol-archive-validation.log`；六个直接消费者测试 27/27 通过，十秒外进程树 deadline、case timeout、并发 4、耗时 5.52 秒，日志 `.tmp/current-fixture-probability-consumer-tests.log`。
- 前述 WASM `restore_json` 失败对应被替换的旧 hash `f81dc44c86912666e9c78fbe2af541c7c36448d60c33ce2b82b9686442db43fb`。数学根修后的新 normal WASM 尚待独立真实 Browser/Worker 复测；不能将旧版本失败转述成新 hash 已失败，也不能预先宣称新跨平台恢复或 IndexedDB 持久化通过。
## Q23 接缝进度

1. 财报月／季频率设置及跨宿主保存恢复接口已接线，真实月报排期仍待用户决定与
   短测收口，见 [频率记录](q23-report-frequency.md)，不能把未定日期当默认。
2. 跨年度亏损／税额级联和组合更正原子机制已写入，原年度拒绝已删除；后续已公开
   报告的更正版本链和合并税资产不抵销仍在收口，见 [级联记录](q23-tax-cascade.md)
   与 [税资产记录](q23-tax-assets.md)。不能因此将整个 Q23 核销。

首批完整存档及公司 slice 曾随 Q07 日期事实在 `aeb979b` 提交；本轮已由冻结源码的
release Engine producer 再次生成、restore/resave 等值验证、Web 严格解析及相关短测，
当前 fixtures 对应上文数学根修后的 Protocol candidate SHA-256，公共 `ProtocolSession`
archive gate 已验证；匹配新 normal WASM 的 Browser/Worker restore 与 IndexedDB 验收仍待复测。
后续领域改动仍须按相同流程重建，不手工改历史
凭证、不假造新税务评估，也不通过运行时兼容旧结构规避 fixture 更新。
