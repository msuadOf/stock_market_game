# 当前候选的合并与裁定

本记录绑定产品提交 `08e4fc7`。逐篇阅读见[来源清单](source-index.json)，最终缺口编号与状态见[总账](../implementation-audit-2026-10-02.md)。初轮sweep与指定模型Luna复核均保留，候选标题里的“新确认”只是分片判断，是否进入G以本记录及总账为准。每项升级均反查原文、当前caller与实际consumer；静态证明未写作本轮行为测试通过。

## 原有状态重新核实

G01–G39都重新沿当前代码检查：G27公开前二次tag SHA守卫仍在，其余38项仍有缺失。旧“已实现”也按原承诺粒度复查；Account/Position/OrderBook/Market、P0–P9候选提交与日终保存、机构个人经历等主干存在，不代表所有UI、行业编排或异常边界都完成。旧REJECT的修复、后续取代、测试证据债分开处理，没有仅凭旧记录标题继承判断。

| 原编号 | 本轮补证或收窄 |
|---|---|
| G05 | mode切换真实换连接后，旧socket.onerror缺身份守卫并关闭当前新连接；并入连接生命周期，不另编号。 |
| G11 | CivilUpdate已有rebuild；相邻交易日AfterClose＋BeforeOpen旧日全帧与普通TickBatch槽位合并仍缺隔离，不称所有日界都没处理。 |
| G16 | 除root全PlanBook复制，普通candidate还深拷贝ClosingEngine的版本/重述与PublicLibrary报告/公告历史；同一历史所有权目标，未测幅度，不强指定COW/WAL。 |
| G28 | 补入混行业原始科目码/报告归类冲突，完整ReportSet显式拒绝；不能说已经静默发布错误集团报告。 |
| G35 | 补入银行定期存款本息/地产借款支付经营caller；零额合法折旧被零金额JournalEntry拒绝也列入折旧修复边界。 |
| G36 | 新行业装配、封账、适用冲击过滤仍缺；自定义行业恢复需包含ECL等domain validator，不泛称默认工商局已触发。 |
| G39 | 补入baseline-run after/sensitivity自由rerun完整stdout SHA比较及独立root verifier；保留来源完整性/守恒/失败负控，不能简单删比较来过门禁。 |

## 升级为G的独立遗漏

| 分片候选 | 最终编号与依据 |
|---|---|
| sweep01 S01-C01、sweep63 S63-01；luna01/62 | G40：控制UI不等待确认，Desktop仅入队不提供actor应用回执；订单入队确认不改为成交承诺。 |
| sweep03 N1；luna03 | G41：临时支付失败日报告被Session日结丢弃，缺持久/公开风险事实。 |
| sweep03 N2、sweep21 C01、sweep25；luna03/21/25 | G42：个人price memory修剪与protected＋8恢复校验合并一个闭环。 |
| sweep03 N3；luna03 | G43：旧belief无条件加入候选，淡出后继续新观察/获知/建计划。 |
| sweep02 N01–N04；luna02 | G44零量正柱、G45原自选身份、G46卖档错标、G47间断null跨线；分别与分钟累计/焦点/撮合/整段空态区分。 |
| sweep02 N05、sweep41 N1；luna02/41 | G48：中文locale的HTML语言声明与Grid内置辅助文本两个消费断点。 |
| sweep15 C15-01；luna15 | G49：成本半偶/浮盈小额跨层漂移，独立于安全整数范围。 |
| sweep14 C14-1/2/3；luna14 | G50 React异常出口、G51 stop_session释放rejection、G52协议actual/expected详情；不称全部宿主fatal没有context。 |
| sweep29 C29-01；luna29 | G53：restore nextGeneration非推进输入守卫缺失，正常producer递增不核销消费者防御。 |
| sweep17/sweep18；luna17/18 | G54：无数字Money字符串错误为0；限定公开库API，无当前生产caller。 |
| sweep19/sweep20 | G55：公开Strategy NaN/Inf、非法margin被裁剪和零ticks统一校验，合并同族；Session二次防线明确保留。 |
| sweep12 S12-01；luna12 | G56：Pages根站点缺owner匹配，不否定当前项目已发布。 |
| sweep13 N13-2；luna13 | G57：causal新增u64输出违反无损字符串契约；合法大seed即可证明。 |
| sweep31 N31-1；luna31 | G58：独立lender额度误用全部贷款余额；默认单lender与库级合法多lender边界区分。 |
| sweep56 S56-C1；luna55 | G59：保障结束后仍创建新事故赔案，不禁止期内已发生赔案期后支付；端点短测须保留。 |
| sweep48 C48-1；luna48 | G60：性能旅程没操作现行启动选择，正式默认命令进不了.app-root。 |
| sweep48 C48-2、sweep65 C65-1；luna48/64 | G61：性能工具外部整体期限/资源清理与sampler失败即时收尾，同一异常进程收敛目标。 |
| sweep64 N64-1；luna63 | G62：嵌套POSIX detached组逃出外部PGID终止；不以正常内部timer成功核销阻塞时监督。 |
| sweep51/sweep78 S78-02/sweep81；luna77/79 | G63：Rust普通case独立10秒硬监督未实现，整批PASS不能替代。 |
| sweep41 N2、sweep02 C02；luna02/41 | G64桌面键盘选股等价入口、G65导航/分类程序化选中状态；静态接线证据，不假称浏览器实测。 |
| sweep61 C61-1/2；luna60 | G66：dispose和内部重同步交错遗失已接受Promise终态；同步send throw已reject，只遗留registry，不混成相同触发。 |
| luna15 L15-01 | G67：baseline缺持仓-market交叉校验及估值??0；增量账户runtime-delta已有守卫，不泛化。 |
| sweep02 C03；luna02 | G68：合法非默认setup未进入快捷规则/证券选项；撤回“同code改category必然限价改变”的错误示例。 |

## 未升级与排除的候选

| 候选/观察 | 裁定及反证 |
|---|---|
| CLI两个自由运行报告装入同一JSON | Q12：causal文档允许独立新会话，需明确用途/来源标识，没有必须共一次成交的硬承诺。 |
| 混合沪深不同官方coverage | Q13：当前v1同轨/首所单会话时钟与未来差异政策关系需定；不同于G15，实施规则差异前要官方依据。 |
| 工商客户延付、颜色AA、负成本显示 | Q14/Q15/Q16分别为斜线范围、token/文字角色冲突、金额与收益率歧义；不推翻已有准备或合法负净成本。 |
| ClosingEngine::correct/close_year后置报告失败 | Q17：合法派生总计溢出反例比错配industry证据更强，但整项失败零改动承诺未定位；Journal批次原子不自动延伸整报表API。记录局部风险，不称默认会话已部分提交。 |
| collector不独立验最低平台格式组合 | Q18：producer已强制格式，未证正常绕过；旧工作TODO不是重复校验的产品授权。 |
| 银行ECL独立serde与WASM句柄wrap | Q19/Q20：分别受G36当前会话可达性与极低频耗尽策略边界限制，不冒默认局故障。 |
| 非正价报错优先级 | Q21：Market band/PriceCage/账户资源均可先拒绝，旧OrderBook InvalidPrice未规定全链优先级；正常UI先挡。 |
| NPC向量在前、PreviousCommit优先 | Q22：向量次序不是实际受理证据，receipt区分时间窗；需真实竞争轨迹证明，不恢复来源全序。 |
| 重复年度所得税API | Q23：计提时机/一次性/重试需在G35接线时收口，没有公共幂等完整契约或现行经营caller。 |
| 工商BusinessKind标签债 | 最新notepad明确允许顺延到公开披露实际消费；目前无相应consumer，不升级G59之类新编号，也不称会计金额错。 |
| 任意底层全方法强事务、非法serde MAX失败 | 旧失败面真实存在，但Session候选/自然日回滚独立；没有证据支持泛化强原子需求。 |
| build与execute各五分钟、K7各阶段五分钟 | 正式testing/计划已经批准分阶段监督；不把它报成普通case或整体清理实现豁免，也不凭分阶段本身新增G。 |
| bare pnpm与COREPACK_BIN历史建议 | frontend-build当前有固定工具版本；Playwright命令是否须复用同变量属维护/文漂待核，当前已有E2E历史通过，未证现行必失败。 |
| Windows覆盖存档rename | 当前仅静态跨平台疑点，未复现；标准库平台行为与授权路径需实机或可靠实现证据，不升级确定G。 |
| D7 common JSON原始hash与摘要hash不同 | 表示/归一化摘要口径可能不同，旧comparison已获准退役；本轮未执行原谓词，不猜作当前撮合错误。 |
| company_scenarios restore twin未来字节比较 | 两次自由继续不自动有相同实际受理轨迹；是否有fixture特定调度约束仍待证明，列验收契约观察，不删原断言或称已复现失败；G39的真实工具比较另有确证。 |
| 100k typed恢复超过Server解码上限 | 容量/验收边界已承认，非无限容量新需求；WASM/Desktop入口不同，不能统称三宿主有统一上限。 |
| 图标像素/Wayland/三平台GUI/缺原始日志 | 验收或来源边界；不以工作记录未勾/文件名缺失直接升级代码遗漏。 |
| 根/旧正文CI、8MiB、确定性、clock、税务法源文字 | 文档漂移/证据债按后续批准决定核销，旧记录原文保留。当前实现不恢复真实行情、公共日内档、任意配额、退役语料工具或发布测试。 |

## 独立复核依据

非作者 `/root/current_commit_audit_review` 复查UI、宿主、资产与请求生命周期；`/root/independent_review` 复查经营/会计/日历/原子边界；`/root/narrative_review` 复查工具、API及发布候选，均完整读相应报告并反查原文/当前代码。本轮修正了Tauri unlisten的类型/版本范围、静态setup的错误举例、CLI独立运行的过度定性、标签延后范围，以及“来源向量次序=优先”的误推。各审阅均未运行游戏回归或联网认证制度；最终完整diff门禁另见[覆盖清单](../coverage-index.md)。

这些裁定是当前明确来源与源码边界的审计结果；不构成新增实现授权，也不构成程序无未知缺陷的数学证明。
