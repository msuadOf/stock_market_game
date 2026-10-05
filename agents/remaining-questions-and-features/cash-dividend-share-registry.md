# 现金分红股东登记基础模块

## 范围与契约

`company::share_registry` 是独立纯模块，不代表 Session 的现金分红已端到端接线。依据《公司法》2023 修订第210条维护完整已发行股本、显式持有人、取得批次及日终物理净转让。财税〔2015〕101号援引的财税〔2012〕85号第3、4、8条规定的税务 FIFO、限售税期与自然月年属于独立 B 模块，本模块不以物理转出批次冒充税务批次。来源和适用范围见同目录 `cash-dividend-implementation-preparation.md` 与 `dividend-research.md`。

每证券显式输入 `StockCode`、`CompanyId`、总股本、开局已结日期和全部 `ShareHolding`。`HolderId::IssuerTreasury` 只表示该证券发行人自持；其他公司持股必须作为真实 Account 或具名 External 输入，不能归入自持。未模拟流通股没有默认归属。每个 lot 的取得日期、来源、限售条件都必须显式提供，不推断初始税期或送转继承。

日结只接受一日每 holder 一条净变动，不接受逐笔撮合 FIFO；净变动总和必须为零，净增取得来源必须明确。日内买卖回转的零净变动不会消耗旧 lot 或重置取得日期。T+1 由交易账户控制可卖性，本模块不把 T+1 锁定理解为无分红权。`MovementScope::PublicMarket` 按可公开转让的物理批次顺序处理净减：跳过尚未解禁的批次，允许合法混合限售／流通持有人卖出较新的流通股，显式解禁日达到后允许转出；净增只能提供 `SecondaryMarket + Unrestricted`。`DisposedLot` 只是物理变動证据，B 模块须另按适用法律类型维护税务 FIFO。`NonTradingTransfer` 返回明确类型化不支持错误，司法／继承过户、送转限售继承不能借此普通市场 API 冒充完成。

登记必须在指定登记日等于当前已结日时创建，仅保存显式登记事件的不可变快照，不保每天全量名册。已创建快照可查询及相同请求重试，不能用当前持仓补造过去日期权利。自持股份仍参与股数守恒，但不作为现金股息受益人。登记后卖出不转移已经登记的权利。

不提供隐式存档兼容、版本迁移、税务身份推导或现金尾数算法。发行、注销、配股及回购程序仍由后续真实公司行为凭证接线，本模块不自行批准或产生这些行为。

## 提交依赖

当前模块依赖工作区尚未提交的全域 `AccountId` 规范十进制 u64 字符串契约。host40 十二项短测是在包含该契约的工作区编译运行的，不能外推为仅把 A 模块放入当前 HEAD 后就能独立通过；HEAD 的旧数字／JS-safe 序列化无法通过真实 MAX 身份恢复断言。必须先提交 AccountId／成员／投影的完整依赖批次，或者把本模块纳入同一个连贯批次，不为拆分提交另造 Holder 身份序列化或削弱 MAX 测试。`company/mod.rs` 本模块只有 `pub mod share_registry;` 一行，其它未提交 hunk 不属于 A 模块。

## 验证状态

root 统一产生的 host37 Engine executable 中四项初测均为真实业务红，外部命令10000ms、每项9000ms、四项并行且每进程 Rayon4；不是编译错误。host38 exact 七项中六绿，一项暴露同日消耗旧 lot 后重用 ID 的真实红，已改为所有处置之前查完整旧身份。host39 exact 十项中七绿，三项真实红分别固定混合限售合法卖出、独立 snapshot 无校验及 nullable acquisition 缺键；证据见 `.tmp/checklist-wave4/share-registry-host37-*-red.log`、`share-registry-host38-*.log`、`share-registry-host39-*.log`。

三项红后已完成算法及严格恢复修复，并增加普通市场来源伪装、明确解禁及恢复篡改解禁日两项 guard，共十二项短测。root host40 fresh executable `engine-48220fcc6e1a5073` 的 `--list` 确认十二项均实际编入，十二项 exact 全部绿色，单项0.00秒；八进程并行、每进程 Rayon4、整体外部10000ms及单项9000ms门禁，日志 `share-registry-host40-*.log`。非作者完整源码复核通过，并亲读十二份实际 case 日志完成最终签核；未运行完整回归，未 commit，仍不作为生产股本行为端到端完成证据。
