# LiveReservations 在途记录（2026-10-06）

第一页投影cache真实server候选136.408x；随后改为共享projection目录、只重算失效页、短fixture在caller执行，密集>=512行使用Rayon。最新普通测试live5/account_book/decision_resources11/ledger56/continuous101/NPC21定向通过，命令9s deadline。仍未独立复核、未完成目标。

server-live-reservations-sparse-16 错误启动在release编译handle仍在途的瞬间；存在构建重叠及可能读取旧artifact，保留为无效诊断，不采用其速度。必须确认编译exit0和基准exit后再无重叠正式重跑，不能将此样本重命名冒充有效证据。


## 最新实现与红绿

LiveReservations 保存Arc分页投影目录；每页按AccountId保存cash与StockCode→shares，保留Buy shares=0条目。ReservationCache 用live ledger leaf page的Weak allocation identity辨认输入变化，Arc::make_mut使身份失效、Weak避免ABA。只重算失效页，>=512行使用Rayon，少量短汇总在caller执行；先完成全部可失败checked汇总并按page/order选择原首错，再安装新projection目录。输入页删除则删除对应projection；终结页不参与live预留。所有账户/股票/资源/股权/费用检查保持；未知account先于未知stock错误，与旧完整扫描同序。

AccountBook资源校验缓存保持同一不可变AccountPage与完整prices依赖，预留比较改为projection Arc identity或完整值相等；account任何mutation仍invalidate。page范围与ledger预留共用明确PAGE_SHIFT=5，防止两层独立常量漂移。Projection cache从不参与业务hash、Eq或save serde；DecisionResourceSnapshot值相等只比较实际预留，不比较Weak/source allocation。无新增依赖、交易制度、份额/金额单位或公开协议字段。

初始无复用scaffold红测相同页重复汇总：要求64行，实际128行。新测试验证单页mutation只增加32行、旧密封快照不变、u64::MAX与canonical fold、多order现金/股份checked汇总、跨账户股份不混加、两个错误page在真正达到512行并行门槛且Rayon1/4均先报告早page现金溢出、release/removal清页/再插入新合法envelope无ghost；未知StockCode即使Buy shares=0且缓存命中仍报错，未知账户错误优先。

最新短测live_reservations5、account_book5、decision_resources11、ledger56、continuous101、NPC21通过，filter可能重叠，不合计唯一case。命令9s TERM+1s KILL、4 test threads，最长continuous7.21s。lib和release已真实重编；无临时performance_profile模块残留。

## 有效真实server测量

第一版全部page派发Rayon136.408x。共享目录且只派发失效密集工作后，无构建/其他本任务基准重叠正式server-live-reservations-sparse-clean-16：139.665x，3000tick/21.479989s，actor最近窗口91.75x，CPU样本均值4.06核（16核预算25.39%），SHA256 1ac350cc146d0f98fe259575e5f4af17d3d3684c505c07f4ae4030caf6e484ec。与独立原版相邻配对和再次candidate复测在途，当前不能宣称稳定吞吐或1000x达成。

此前server-live-reservations-sparse-16为无效样本：实际SHA与第一版c2184d4b...相同，确实读取旧artifact且有构建重叠。已确认该构建exit0且旧基准exit0，才启动上述正式clean测量；保留旧证据不冒充新实现。

当前尚未独立复核/提交本批。WASM、HTTP/WS、全交易日、frontend仍未验收；既有save_contract日历错误仍未解决。>=1000x目标保持active、frontend继续暂停。


相邻配对已审查c06151ac原版127.847x /23.473411s，新版139.665x /21.479989s，吞吐约增加9.24%。两次按顺序执行、构建/其他本任务基准关闭、相同默认负载和16核affinity；case/live事务不因测量省略。新candidate再次复测在途。


同一latest SHA无构建/其他本任务基准重叠重复候选复测142.364x /21.079767s，actor最近0.5秒约96.69x。因此本批真实server平均约140–142x、末窗口约92–97x，不挑最好样本当稳态目标达成。没有计入任何虚构tick；实测3000/3001已提交tick，逐帧连续性检查保留。


## 独立复核后补强

全新Sol静态通过6份生产diff，没有阻塞生产逻辑问题，报告live-reservations-independent-review.md；其4组非阻塞专项测试建议的核心路径已由root补强：unique get_mut暖cache失效；projection失败保持原Arc目录且修复后重算；CreatedAtValidation新增行的rebase重新投影、reset清页并旧快照金额不变；nonempty真实合法订单预热前后的ledger Eq、业务/会话hash与SaveSlot成功序列化字节完全相等；空ledger序列化不存在reservation_cache字段；AccountBook失败同输入须重复执行且成功cache仍None。生产实现没有因补测试再改。最新live8、account_book6、hash_contract14、phase_timing12通过。protocol rollback使用正确filter；此前错filter0case不算绿灯、未计入任何测试数。

root当前已重新编译lib test binary，但release binary仍来自同一生产实现（后续只增cfg(test)测试）；正式后续构建需核对所有生产文件与SHA。本批最终一次性Luna只检查新补4项边界，与实现者Luna/前Sol均不是同一agent，不复用reviewer。


正确filter的protocol rollback26通过3.45s、hash_contract14、phase_timing12通过；先前零case错误filter没有计作通过。新增真实合法非空订单的cache预热Hash/SaveSlot字节测试也已执行，最新live8/account_book6通过，生产实现没有继续改变。


全新Luna固定边界复核无断言弱化、非法fixture或单位问题，4项指定case均独立通过，报告live-reservations-boundary-final-review.md。它明确这些非阻塞建议不是穷尽覆盖：尚未把“删除另一个旧页与其他页overflow同时发生”、独立source allocation的LiveReservations/DecisionResourceSnapshot值相等、AccountBook失败后转成功缓存/完整None→Some→None组合分别做专项断言。生产Sol已静态证明相关分支且判为非阻塞；不把已覆盖核心路径夸大为完整覆盖，也不为可选组合开启无限再审。这些局限保留在记录中。两位reviewer均全新、未参与本批实现。


补测试后的最终release真实重编exit0/51.43s，当前SHA bf08dda9941d3b66a82cc4b457b652807eb1bed73a5e1eae17c3f463503fbbb5 与先前release不同；因此重新做无重叠最终server测量，不凭生产实现没改就复用旧artifact作为最终绑定证据。


最终重编后正式server-live-reservations-final-16：142.419x，3000已提交tick/21.064587s，actor最近窗口91.28x，CPU逐秒样本平均4.19核 /16核预算26.20%，SHA bf08dda9941d3b66a82cc4b457b652807eb1bed73a5e1eae17c3f463503fbbb5。仍未满核，未达1000x。已确认此次release构建与全部基准handle均exit0，没有在途长进程；本批生产实现及最终边界检查通过，准备本地保存。
