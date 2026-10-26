# runtime diagnostics D1：完整实现独立审查

- reviewer：`/root/sol61_branch_task_inventory`，未实施生产或测试，仅只读源码、diff、日志；未构建或运行测试，不改main。
- writer：`sol61_runtime_diagnostics_d1`，独占`.worktree/runtime-diagnostics-d1`，基`9065ac1a`；已审storage中间提交`ba696ded`。
- 当前结论：**APPROVED，限最小D1中间提交；不单独合main，不代表完整诊断feature交付。** 最终冻结与三问见末节；下文保留build-1预审历史，不能将其绿绑定到当时尚未重编的新内容。早期观察SHA见`runtime-diagnostics-d1-prereview-source-2026-10-26.json`，最终以build-2身份为准。

## 已读路径与方向

已完整读取当前tracked diff及新增`runtime/diagnostics.rs`、session失败测试、两个integration文件。真实来源在group操作构造处区分ExternalOp、chain cancel和external cancel；CancelAll保未知raw。标签不参与排序或资源校验。desk成功place已知原qty、实际OrderRef和真实preQuote；成功cancel以真实剩余量记录，postQuote来自当时单股全簿，未从Accepted.remaining或末拍盘口猜测。

单股报告buffer经DeskReport→TickEvidence→World跨轮append→TickRecord→driver take消费，take在既有`report_log.last().cloned()`之前，未把新payload再永久复制一份。coordinator显式核tick/round/stock上下文；同股实际轮次顺序保持，跨股仍只作诊断枚举。Abort清desk未上行buffer，失败candidate丢弃。

session入口先捕获原`causal_time()`；driver收束及原写时undo安装之后、扩展安装写入之前prepare，未知来源/身份/checked sequence失败显式rollback baseline并discard driver；普通step不依赖callback专属undo。collector在普通入口全部安装成功、callback入口callback成功后才publish，publish仅追加已校验delta。D1不改Decision/Filled/trace maps；D0b门继续保留。

Quote准确标明O(单股挂单数)。place在编号和交易副作用之前验证现有深度与本单增量，Cancel/CancelAll在纯删除前验证深度；之后postQuote只受单线程删除或已验证增量影响，不能新增深度溢出。feature off的新增载体/字段/hook均由cfg移除；CancelAll返回Result适配不新增默认事实或配置开销。新增featureoff实际编译/absence仍待最终包核验。

## 当前具体发现：非D1路径可能生成假的完整open报告

当前place hook对所有`RoutedPrice`和所有受理phase都记录Submitted，prepare只核来源是否PlayerExternal；但D1未记录Filled、MarketRemainder、TTL或DayEnd终止。

具体输入：无NPC连续盘，player提交无对手的Market买100股，真实desk立即释放余量并产生Cancelled；新sideband只有Submitted+Quote，prepare当作已支持普通external成功发布，`causal_diagnostics()`据此把真实已终结订单算作open100。连续限价同轮全成也会保留Submitted而缺Filled，出现相同假open。此与v3“未支持路径不得伪装完整报告”有张力，已返writer核实。

不要求提前实施D2。最小收口可保留feature-only Unsupported raw/能力缺口，在prepare显式失败并走同一回滚，而非猜typed原因或屏蔽真实事实。中间提交不单独合main的范围保持；若作者认为冻结设计明确容许这类部分成功报告，须给具体依据供协调者核定，不能自行扩大成交实现。

作者已确认发现有效，拟为成功Market/非连续申报、真实settle_fill与expire_due保留Unsupported raw，prepare显错；该必要能力门方向已批准，不生成Filled或猜typed原因。另补核driver有`day_end`、`day_end_with_runtime_market_day`、`world_day_end_for_step`三处Drain入口：自然末拍prepare检查有真实余单尚不足以覆盖手动日结，已要求共用feature-only前置余单能力门，使用既有权威发布resting计数，不添查询往返、不接Drain sideband；空簿无需阻断。未支持标记须来自实际成功点，不能改变原业务拒单优先级、序号消耗或误将已拒绝操作报成诊断失败。此为修订设计准入，尚未读取最终实现或新红绿。

## 已读测试证据及未完成门

build-1实际4个binary的Cargo artifact均fresh=false，记录声明source_unchanged；原两拍red为无facts导致原180行越界，新短red为0对3，绿原两拍保持真实100/100/0、Voluntary、8/480。原restore观测中断case绿；新公开3例包括真实OrderId/pre-postQuote、跨session owned Protocol checkpoint、多成功tick后回退及查询前后save/hash不变；8档精确depth短测在同次日志中计时约3.4ms，不把它称独立hook纯成本。

build-1的d1过滤unit日志为10例（包含若干既有命名d1的case），不能误称10个全为本批新增。5个session新失败例、2个world新例覆盖callback、安装、World validator/Seal、未知来源零泄漏与真实Abort重试；当前新增两例尚待新构建。feature合同日志4例中3个为import带入的既有closure负控，只有1个本批固定操作合同；仍需feature off同一合同与absence原件。

完整终裁需要作者最终冻结全部source/binary与原命令deadline/实际并发、上项具体发现的裁决/修复、最后新增测试的实际结果。D2–D5、默认retail真实生命周期及原16红继续单列，不由本次D1部分绿关闭。

## 最终冻结复核（2026-10-26）：APPROVED最小D1中间提交

最终源码本地提交`35158546`（前序已审storage `ba696ded`、证据`02ee9c4b`、既有feature fixture适配`4948b934`），独占树clean。根`.tmp/runtime-diagnostics-d1/build-2-source-before.json`共794项，SHA256 `fd2bdfd13e6b4b075f131ec8cc74faa6d20fa2b3ee6b9cb7d7b0fcfae135967c`。本人在冻结diff及提交后均重新逐文件hash，零漂移。已完整审tracked diff与4个新增Rust文件；不改生产、测试、main或作者证据原件，未构建或运行测试。

### 有效发现已修复并再次复核

- 无对手Market与有余单自然日末两个负控在实际build-1 rlib上真实红：原step返回Ok，分别含OrderCanceled，断言严格要求在诊断prepare处显式失败；不是编译错误或改断言凑红。
- 最终`Unsupported` raw在已合法取号的Market/非连续place、实际settle_fill成交、真实expire_due出现；prepare拒绝并走baseline回滚/discard driver。未实现Filled、Expiry或auction typed原因，不从缺失facts猜原因。正常业务拒单在这些成功点之前继续按原优先级处理。
- 自然末拍有真实余单时，prepare在扩展安装/Drain前拒绝；三处手动Drain入口共用`ensure_causal_day_end_supported()`前置检查。首拍前World v0发布的resting为占位0，作者自查并用真实`desk_order_seeds`存在性feature-only bool封住checkpoint重建漏洞；只在report_log为空时用此种子事实，成功新tick后用既有权威发布resting_count。没有新查询往返，也不修通用v0或实现D4侧带。
- 新真实测试覆盖双腿成交回滚、自然日末、三入口手动Drain保持facts/hash/挂单、owned checkpoint重建后立即Drain，以及原未知来源、callback/安装/Seal/Abort失败零泄漏。原两拍100/100/0、Voluntary、8 market minutes/480 civil seconds、真实OrderId强断言保留。

### 构建与短门实物

feature build-2实际55.518s、6个artifact均fresh=false；default实际70.371s、2个artifact均fresh=false。两次均`cargo -j8`、外`run-long-validation 300000`，原命令/结果与运行中资源样本保留。资源样本包含同一实际engine rustc多线程累计CPU超过相应wall时长，支持实际多核使用；不把jobs或未经逐线程R采样的24.17峰值写成全机满核。

本人对以下8个实际二进制重新完整SHA，全部匹配各自artifact manifest：

| 配置/target | 实际SHA256 |
|---|---|
| feature diagnostics | `775e954eb1fc77d98b9484b461303c2517ae398b3127aac31902a4e2b36d2ab3` |
| feature diagnostic_parity | `6f09e2a2cb9a8d48614ec420f0cf405417186bf823a6891ec1a71f45b02b8e71` |
| feature causal_diagnostics | `28840076203f5b1490ac9297ee1891b98904a67c4fa7e09993689bae6093b672` |
| feature runtime_diagnostics_d1 | `20a660673c81028fd0e59aa5ddb008b696d8adac9faab3e98ce1255b24519b51` |
| feature runtime_diagnostics_contract | `eea79e333b4c8e080d567f7e2bc4291bd1bc912132c5a91774962701e6e3c913` |
| feature engine unit | `ee1c92f212071d079a17f86e2821433fb0caf8c3f36ac636c099c681be3c9c74` |
| default diagnostic_absence | `b84e2df8c401fc015bd51ef9a7a9c2b7e7a53b6c9941534aa47808ca2ad598ae` |
| default runtime_diagnostics_contract | `1241752c04f61466490b978fd761d0dee946d8f01d0afedf3b462a3a9ceefe65` |

短门日志亲读：unit过滤16/16（本批13+既有3），公开D1 5/5、collector storage 7/7、feature固定合同exact1/1、default absence2/2、default固定合同及既有helper负控4/4，全绿。每条实际argv、外10000ms、exit0、并发组4或2及原工具wall在`short-gates-receipt.json`；SHA256 `07db924968e87346fb75561748f1ac480e075a8cc11a0ceaf253a4e4d010c45c`。六条wall依次3.284/0.903/0.204/2.079/0.560/2.264s，未从harness时间反推监督器时长。default absence保原无feature诊断不可用语义，固定操作合同在feature on/off均通过；序列、资源、恢复均保原不变量。

### 原integration矩阵当前范围

已逐名比对原`.tmp/diagnostics-feature-fix/integration-run-red.json`27项与本包`final-matrix.json`25项：无extra，确只缺`company_scale::{four_industry_fixture_catalog_remains_constructible, short_checkpoint_preserves_bytes_and_rejects_unknown_plan_references}`，作者正在协调单binary补链接/执行，不能把25冒称27。本D1代码门不以D5全矩阵完成为前置，待补结果只追加记录，不因此要求重跑已核25或扩大成交实现。

原27为11绿16红，当前已跑25为13绿12红。明确两项原绿变红：`diagnostics::baseline_excludes_preset_history_and_reconciles_trade_volume_to_daily_candles`、`baseline_keeps_each_seed_visible_and_summarizes_observed_runs`，均因中间slice未知NPC来源能力门显错；不得据16→12简单声称整体改善。6项原红变绿中有5项serde/算术/id纯例，不能全归因为本D1；原两拍生命周期则由本次真实接线恢复。当前其余red分别为未知NPC来源、未接竞价/DayEnd和真实trace缺失；原输入/强断言及旧日志保留，D2–D5继续负责，完整feature仍未通过。

### 项目三问终裁

1. **A股语义：在最小slice范围符合。** 只忠实投影真实原qty、数值StockId/OrderRef、普通external来源及成功主动撤余量；单位继续分/股。真实身份由现有owner映射并校验局部坐标，company取现行发行人映射；不查末态PlanBook猜plan/decision。事实不改撮合、FIFO、价格带/笼子、费用、T+1、RNG或时钟。依据为冻结v3采用的既有正式ADR/领域规则；没有新交易制度，不宣称本次重新联网核验全部A股规则。feature on尚不支持的真实路径显式失败，default行为保留；能力门限中间分支，不能作为已发布完整产品行为。
2. **必要性与最小范围：通过。** Accepted.remaining和混合Cancelled无法恢复所需事实，顺序sideband及最终成功发布必要；共享历史消除本批新增checkpoint O(history)复制。Unsupported与初始种子bool只封住本次会生成假报告的缺口，没有实现未授权D2/D4、添channel/屏障/线程/依赖或通用storage框架。`plan_owner_tests`仅补现有feature编译必需gross字段，取同fixture950分×100股checked计算，原断言未弱化。
3. **测试、跨层与复杂度：有效发现闭合，无剩余D1代码阻断。** 原红、修后真实crate短门、Protocol多tick与跨session恢复、save/hash查询稳定、callback/安装/Seal/未知来源/成交/DayEnd失败零泄漏、事实move恰一次及featureoff均已核。Quote成本诚实为O(单股簿)，全量facts缓存只显式查询；非空Decision/Filled maps和128 trace环仍需D0b独立设计，不由D1空map通过豁免。未跑all；批准仅最小D1中间提交，禁止单独合main或关闭完整诊断任务。
