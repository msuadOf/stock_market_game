# 补缺实现与 main 职责契约合并

## 合并范围

将补缺分支 `codex/implementation-audit-final` 的 `7c207e0` 合入以 `b650dd7` 为基线的
main。共核对 362 个原有合并变动路径；追加的工作记录属于本次合并证据，不计入旧分支
已验证结果。所有工作在当前主工作区完成，未改写分支历史、未推送、未发版。

合并保留补缺的四行业经营/报表与集团、个人获知和分析/经历/持续计划、三宿主真实受理与
恢复、UI 及验证工具能力；保留 main 的职责名称和严格当前存档结构。公共游戏存档没有
`schema_version`，旧 runtime 字段/来源标签拒绝，没有 alias、格式迁移、双字段恢复或默认
补齐。工具证据的独立真实数值版本、会计报告 revision 和历史见证不作为游戏版本兼容。

16 个文本冲突逐处结合双方语义解决，未用整文件 ours/theirs 覆盖功能；自动合并文件也
检查新增片段的旧函数、fixture 路径及机器身份。公司域的 Arc 保存恢复使用真实原对象，
不另造备用实现。旧审计原文与固定见证保留，现行总账的职责名称与 Q23/验证状态已同步。

## 独立复核与修复

五组非作者复核覆盖完整 diff：公司/会计、运行时/策略、宿主/UI、工具/CI、文档/工作记录。
各组完整读至 EOF，修复后再核签；运行时最后补审 Money 和 indicators 两个遗漏路径，
最终覆盖 95 个代码/测试路径及 characterization 记录，宿主 112、工具 20、文档 62，
公司/会计另进行与补缺分支的完整三方功能比对。无未处理的有效阻断发现。

- `review_merge_companies`：报表在成员验证前索引混合 chart 可因重复 MemberId panic；
  改为先既有 consolidate 验证，再扫描/分类，明确返回 DuplicateMember，不重复聚合。
- `review_merge_hosts`：Worker/Tauri 部分基线入口忽略消费者拒绝，以及 Worker 刷新/销毁
  微任务竞态；拒绝不误记已交付、不恢复运行，销毁后不重建状态。
- `review_merge_runtime`：Bank 负控 fixture 未采用真实 Bank setup；改为明确装配的 Bank，
  零 NPC 隔离 ECL 验证，保留合法恢复与全部损坏政策断言；旧 policy 常量亦同步职责命名。
- 集成编译发现 Desktop 发布间隔常量被错误标为 cfg(test)，生产入口无法引用，已移除遮蔽。
- 旧贷款溢出负控依赖默认借款尚未偿还；真实到期接线使其归零，MAX+0 本来合法。
  fixture 明确构造 MAX+1 分，不放宽校验或替换精确错误断言，运行时 reviewer 已再审。

无交易制度变更：A 股交易单位、T+1、费用、真实受理/撮合、失败原子性、日终保存和资金池
不补钱的边界保持。公司报表 scope 优先明确为游戏政策，不冒充交易所法定估值要求。

## 验证边界

workspace 全 feature 测试目标编译通过（仅 no-run，不执行全量测试），jobs=32，外部
300000ms deadline；实际观察到多个 rustc 进程并行，各使用多个线程。Web TypeScript
编译、实际 Rust SaveSlot/SessionSetup 类型导出、fmt 和 diff whitespace 检查通过。
普通 Node/Rust 测试命令使用 10000ms 外部进程树 deadline；Node case timeout 为10000ms。
独立 Rust 二进制并行执行，二进制内部使用 4/8 个 harness 线程。

已实际执行的代表性短测包括四行业/集团真实日結3例、四行业报表23例、经营年末/到期/
付款事实13例、Retail分析5例、存档当前结构拒版本标记、Desktop固定批次4例、WebSocket
10例（原有手工性能探针1例忽略）、三个Web宿主53例及存档结构/公司契约34例。
Server actor 的最终全 feature 19例、受控 characterization 9例也已通过。
SaveSlot 集成套件首次35/36通过，失败的贷款溢出 fixture 已修；最终从 Cargo JSON 实际
产物 `save_contract-ec6068219c5bea83` 执行36/36通过，8.16秒，不掩盖首次失败。
复测时曾误用旧二进制路径，其失败/超时不计作修复后结果；按最新构建产物重新执行确认。

受控 characterization 和 Server 成交 fixture 保留全部真实 Trade、股份守恒、日界、
restore 和扰动断言。开局库存是明确守恒的测试初始条件，不伪造成交、不注资或保证随机市场
持续成交；Server 从真实日级存档加载，仅移除未受理开局意图并保留观察截点。
新固定锚由独立原字节与业务 guards 推导，未把失败输出直接写作期望；取证及非作者复算见
[characterization记录](merge-characterization.md)。旧40秒 Server测试已改为共享2秒短 fixture。

本次未运行完整回归、长统计/性能矩阵、浏览器完整旅程或跨平台安装验收；此前结果不自动
继承为本次合并的通过结论。未定 Q 项和未来产品功能不因完成本次合并而宣称已实现，
完整回归仍按用户要求留到后续全部功能结束时执行。
