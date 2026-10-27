# R2 分红与送转独立候选冻结记录

状态：实现与候选短测已完成，等待未实施者完整 diff 复核；不是生产权益验收。

## 基线与范围

独占树 `.worktree/distributions-dayend-r2` / `feat/b4c-distributions-dayend-r2`，基 main
`b172c1ab`。R2 红测试 `91608f5f`；依赖 R1 两提交精确叠为
`f9e2c8a0` / `0ed77510`（原 `fb0b5135` / `48cd9c50`）。这些 R1 提交的最终独立审查
与真实 closing 增强 fixture 由原 writer 和 root 负责，R2 十项短测不冒充 R1 #119 验收。

只新增 `session/corporate_actions/distributions_day_end.rs` 与同目录测试；公共容器仅追加
mod 声明。复用现 main `company/cash_dividend`、`stock_distribution`、`cash_dividend_tax`
和 `CompanySystem` 的领域算法，不搬回旧 4000 行、不改 runtime/drive/failure/真实账户。

## 冻结 API

`SessionCorporateActions::distributions_day_end_candidate(&self, DistributionsDayEndInput)`
返回 `Result<DistributionsDayEndCandidate, DistributionsDayEndError>`。

- 前置：调用者在隔离副本安装 R1 `registries` / `dividend_tax_books`；历史公司账簿已验证。
- 输入：`day`、`calendar`、`catalog`、只读 `company_system`、最终真实组
  `available_cash: BTreeMap<AccountId, Money>`、`tax_mode`、三态 `flat_withholding_bp`。
  现金不得包含在途 buy purse；必需账户缺失、负余额、加法溢出显式错误。
  新增 `capital_reserve_tax_sources` 为逐 event 的 `StockPremium { evidence, capital_increase }`
  或 `Other`；仅为输入候选事实，调用者不得从 kind／混合总科目推造来源。
- 输出：`actions` 和 `company_system` 隔离副本、逐笔 `intents`、逐 holder `sources`，
  以及供后续候选使用/核验的 `projected_available_cash`。外部持有人回执在
  `actions.external_receipts`，新股外部来源在 `sources` 与名册回执。
- `DistributionIntent`：`DividendGrossCredit`、`DividendWithholding`、
  `StockDistributionCredit`；分别保留 payment/plan/collection/event 身份与
  `AccountId` / `StockId`，单位为分/股。送转新股 `available_on=day`，不带买入冻结。
- 税款来源分别为 `FlatPayment` / `IndividualCollection`，绝不合并净额冒名。
  `DistributionSource` 的 holder 明确区分 `Account` / `External`；只有 Account
  税前股利是游戏账户注入，External 付款也是实际候选事实但不算账户注入。
- 个税不足按真实局部余额部分实收并保留税账 `needs_funds` / `outstanding`；不会因为
  全额不足拒绝整个候选。Flat 对所有账户统一扣，无税账；Exempt 保留显式装配税账。
- 错误保留日期/事件/StockId/AccountId 及原领域 typed source，不把错误压成日结日期。
  缺账户、负实际余额及来源金额 expected/actual 均有专用 typed 变体。Flat 零代扣只保回执，
  不发零金额扣款意图；有个人税账的 Conversion 必须有完整溢价来源事实，缺／Other／
  空 evidence／覆盖金额不等均拒绝。个人 BonusShares 显式 Unsupported，见下述法源边界。
- 返回 `Paid` / `Credited` 只是拟提交候选事实；生产调用方仍须等待逐笔真实行政回执
  与来源勾稽后整体安装，不能仅因准备成功声称已真实入账。

## 规则依据与未覆盖边界

作者本轮实际取回并核正文的税务原件在 [sources](distributions-dayend-r2-sources/manifest.json)。
旧文档把 `c5203902` 命名为 85号，但该 URL 正文实际上是 101号；本轮按正文纠正新记录，
保留原 HTML，不改写历史台账。101号第一/二/三条与其链接的 85号第二/三/六（八）条
直接支持本片的个人档位、转让扣收、资金不足、FIFO、日净额及送转取得股票来源；未修改
共同算法税率或日历。

作者另逐条核对只读研究者取回的官方文本（原件由该 worker 冻结，不复制进本批）：
根仓 `agents/branch-completion/share-fraction-sources/chinaclear-shanghai-2026.pdf.txt`
655–673 附近、`chinaclear-shenzhen-2025.pdf.txt` 821–840 附近。对应官方 PDF SHA256
与 URL 在同目录 manifest：沪业字〔2026〕33号/2026-05-15 版取代旧2024版；深业〔2025〕68号。
这些原文支持账户碎股排序/随机补1与专户失权；不支持统一 aggregate floor/ceil 总量推导。
每股 micros 不穷尽深市“每十股六位”的精度。本片保持显式批准总量，不推新总量规则、
不加碎股现金。其他市场限售继承不做跨市场推定；仍受限新股显式拒绝。

后续官方研究补齐后，作者实际读了根仓 `tax-basis-sources/089-central.txt` 第70–71行的
股票红利按票面金额收入额，以及 `289-zhejiang.txt` 第17行仅股票溢价发行资本公积免税；
完整原件/URL/SHA在同目录 manifest（该独立研究报告由 root 另行安排可信审，不冒充已审）。
089总局现行修正版正文：
`https://www.chinatax.gov.cn/chinatax/n810214/n810641/c102061/c102062/c5172932/content.html`；
289浙江官方全文：`https://zhejiang.chinatax.gov.cn/art/1998/5/15/art_8409_11735.html`。

当前 `CapitalReserveConversion` 仅扣资本公积总科目，未细分/校验股本溢价来源；不声称
任何资本公积均免税。root/readiness/独审共同明确候选事件来源门：对有个人税账的转换，
必须显式 StockPremium、证据非空且金额与实际 fact 精确相等；缺/Other/空/错金额均 typed
拒绝，原 source 回传 R4。该输入契约不证明来源权威已经实现，R4/finance 必须从真实
来源构造并在受理/恢复统一校验，本片不增完整来源追踪或非premium计税。

独审还核出既有 BonusShares 税基把本次红利绑定到新 lot：原股已持>1年，ex 日卖原股，
次日卖新股会按新持期收20%，反向短期原股税款则可能延至新股卖出。作者实际复核了
`cash_dividend_tax.rs` 原评税代码及沪2026指南第939–941行“面值×送股比例×税率×待缴税股数”
公式，确认没有用户批准该错误简化的原文。因此原先“个人送股按新 lot 计基成功”的测试
契约必须纠正：保留旧10绿/13绿历史，改成显式 typed Unsupported 并验证失败原子，
不是为过测试弱化；Flat／无个人税账送股正控和 premium 新股未来 lot 正控仍保留。
个税送股完整原登记权益税基/处置关联留后续独立批次，不在本片重写底层税务模型。

同日“先市场、后送转”的税账分列沿既有明确简化，不声称符合85号逐日合并净额；
正式边界已同步 trading-rules。

## 原始验证

证据根目录 `.tmp/distributions-dayend-r2/`（所有 target/日志在仓库根 `.tmp`，不提交二进制）。
编译均通过仓库进程外 `runBoundedCommand` 共享 300000ms deadline；Cargo `-j32`，
`RUSTFLAGS=-C codegen-units=32`，debug=0，incremental=0，独立 target。
普通短测直接运行刚生成的同一 lib test binary，外 10000ms，`--test-threads=16`。

| 原件 | 观察 |
| --- | --- |
| `red-contract.log` | 缺失 R2 Candidate/Input/Error/method/intent/source，编译期红；其他 API 拼写先校正 |
| `green-build-1.log` / `green-tests-1.log` | 首轮实现 5/5，通过0.01s |
| `green-build-2.log` / `green-tests-2.log` | 扩充税账与长期限售 9/9，通过0.01s |
| `final-build.log` | 最终同 warm target 编译通过54.68s，既有573 warnings，未将warnings称零 |
| `final-tests.log` | 最终10/10，通过0.02s，16线程；含专户失权、外部户、Flat机构、登记后卖出、部分欠税、显式Exempt税账、送股税基、送转三账一致/无买入冻结、失败输入不变、幂等重试 |

末次资源抽样时编译已结束，采到IO PSI some17.49/full14.95%；未取得运行中 R2 rustc
的 CPU 活跃采样，不据 `-j32` 声称实测满核。已向 root 报告所有 build 结束后才开放 R3b
窗口。未跑全库、宿主、生产跨拍、save/restore、mixed 或全链原子验收。

## 独立复核修复轮

`7438c720` 两红与 `7664a128` source shape/行为红：`fix-red-build.log`54.69s，
`fix-red-tests.log`为13项10过3败，分别准确暴露零代扣意图、负现金丢实际值、来源缺失静默免税。
`8e920eba`修复后 `fix-green-build.log`53.81s、13/13过0.02s，785项编译源码前后hash相同，
见 `fix-green-sources.json`。该中间绿不能代表 BonusShares 已修。

`850d94bb`固定个人 BonusShares 拒绝正确契约，并补来源四项 typed match；
`bonus-red-build.log`51.21s，`bonus-red-tests.log`为13项12过1败，准确暴露旧候选仍成功。
原错误新lot计基本次红利正控已改为原子拒绝；premium新lot仅用于未来股息仍有正控。
本轮独立进程监督不变，附 `/proc`进程树0.5s CPU采样 `bonus-red-cpu.json`：峰值6.826核、
26线程，wall51.334s；不是满核承诺。绿 `bonus-green-build.log`56.73s，`bonus-green-tests.log`13/13过0.02s；
`bonus-green-sources.json`785项编译源码前后完全一致，CPU采样峰值6.473核/26线程，wall57.075s。
实际绿binary SHA256 `38000d1cb58988438cbd651c2edfdef55aebf3eb7915be3b299ce2d9ac9dcc51`。
最终来源/committed blob/build/test/二进制绑定见同主题 `distributions-dayend-r2-artifact.json`。
本轮未再扩全库验证；仍保留573项既有编译warnings，不称零警告。
