# TradingTimeline 独立初审

2026-10-05。审查完整 tracked 与 untracked 源码/测试 diff，排除 PID，未实施产品代码。

## 有效发现

### P2：读档失败不一定表示宿主仍是旧 setup

useSaveCommands 仅在 await host.load 成功后配置 Timeline。TauriHost.load 的顺序为 restore → 安装/分发新 baseline → resume；最后 resume_session 失败时，权威宿主已经采用新档 setup，调用方却进入 catch，刷新新 baseline 但留下旧 Timeline。Worker 恢复后的 baseline 回调拒绝/异常也有同类边界。不能把“失败不改旧配置”作为所有阶段的统一规则。需区分 restore 未提交与已提交后失败；补模拟已提交新 baseline 后拒绝的 fixture，确保权威 setup 与展示一致。首次启动在 connectProtocol 前配置的顺序正确。

### P2：自定义开盘窗口与 engine civil 时钟产生未声明分叉

TradingTimeline 对 auctionEntryTicks 与 PreOpen 分段映射，而 engine observation_civil_instant 对整个 auction_ticks 线性映射900秒。auction_ticks=10 时 tick7 UI为09:25:00，engine观察时刻为09:25:30。auction_ticks=1时没有 PreOpen，当前 tradeTime 把集合成交显示为09:30。需要明确非3整除/无PreOpen的简化边界与测试，不能把两者都称为同一权威 civil 时钟；尤其公司公开披露使用 engine 观察时刻时，应避免与界面时间产生无解释冲突。当前 MobileGameClock 仅 title“按阶段压缩”，尚未明确说明缺开盘或收盘阶段。正式规则说明与UI简化标识待补。

## 三项门禁

- **大 A 语义/依据：** 默认竞价10分钟、盘前5分钟、连续237分钟、收盘3分钟符合现有沪深时段。独立查阅上交所2026规则发布页，确认2026-07-06生效；深交所2026规则2.3.2明确09:15–09:25、09:30–11:30/13:00–14:57及14:57–15:00。来源：[上交所2026规则](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/c_20260424_10816482.shtml)、[深交所2026规则](https://docs.static.szse.cn/www/lawrules/rule/trade/current/W020260424690713155663.pdf)。自定义压缩属于游戏映射，不是真实交易所每tick时间制度，需明确登记。
- **必要最小范围：** 以 SessionSetup 替代默认常量可修复负分钟及短局跨日投影，共用低频 Context 避免两端各算一份；不改变 engine tick、撮合、量价或存档格式。投影 configure 清空旧分时再 reset/baseline合理。新增 Timeline 校验和整数比值计算无新依赖。
- **边界/复杂度：** 默认行为、竞价量与分钟量分离、日界重置已有测试；还需零/极短阶段、非3整除、invalid setup与tick、restore已提交后失败、Context真实短局等边界。上面两项跨层漂移未解决前不关闭门禁。

独立短测：trade-time、market-chart-projection、session-host-lifecycle、save-commands共32项通过，约0.256秒，case与进程树deadline10000ms、并发3。未把通过解释为已覆盖上述缺失边界。主 agent 正追加测试、正式说明和真实E2E；完整目标仍在途。

## 第二次完整复核

开盘时间已改为与 engine observation_clock 同样的整段900秒比例。非3整除、开盘1 tick、无开盘/收盘窗口的简化说明与测试已补，原时间分叉 finding 已关闭。BigInt 比值避免展示层大整数乘法先溢出安全整数；输入仍拒绝不安全整数。未改 engine 规则。

EngineHost 增加恢复提交通知对初审发现是必要的最小跨宿主契约扩展。Worker/Tauri 在确认后、向UI交付baseline前回调；SaveCommands幂等安装setup并保留提交后失败的新配置。真实Tauri resume失败测试有效；首次启动没有callback时仍在connect前配置。Remote成功ack和较早新generation baseline触发一次的路径也已覆盖。

**仍有 P2 Remote 结果未知边界：** 服务端已提交恢复、HTTP响应先因网络失败拒绝，而新generation baseline随后到达时，load finally已经清空pendingRestore。SaveCommands.catch刷新新基线时无提交通知，继续用旧setup/timeline交付。需要补HTTP reject先到、之后resync返回已恢复generation的测试，并保留未知结果到权威基线确认后再判定；不能把所有HTTP失败当作提交前失败。已报告主 agent，等待修复再次复核。

另有文档漂移：UX-CONTRACT 仍无条件写“每个PriceTick是游戏秒、每60tick一分钟”，需限定默认局并引用自定义映射，以免与新增规则矛盾。

独立运行 trading-timeline、save-commands、remote-lifecycle、tauri-host四套39项全部通过，约0.393秒，case/进程树10000ms deadline，并发3。通过不覆盖上面新增结果未知边界。完整目标仍在途，本轮暂不关闭门禁。

## 最终再次复核：通过

Remote结果未知finding已修复。HTTP失败后pendingRestore保留为awaitingAuthority，随后权威新generation在UI交付前触发一次提交通知；旧generation确认不改配置并释放重试屏障。dispose清除待执行回调，初始未知generation不以普通初始baseline冒充恢复。新增测试覆盖HTTP先拒绝后新generation、旧generation确认后重试，原baseline先于成功ack测试保留。

UX-CONTRACT已将PriceTick秒/60tick规则限定为默认局，并修正237分钟连续竞价加3分钟收盘竞价描述。正式trading-rules记录极短、缺阶段、整数阶段粗化及图表区间起点与逐笔端点的区别，原时间跨层finding关闭。初审及后续全部有效finding均已修复再次复核，当前完整diff未发现新增阻断，本批独立复核门禁通过。

独立最终重跑 trading-timeline、save-commands、remote-lifecycle、tauri-host共41项，全部通过，约0.416秒，case及整命令10000ms deadline，并发3；git diff --check通过。主 agent 报告168项定向短测通过及28项WASM专项浏览器通过；浏览器批次早于最后Remote修复，不能把它说成真实远程端到端验证。生产构建最后重跑由主 agent记录，不提前声明。

三项门禁最终结论：展示遵循实际SessionSetup和现有engine civil比例，原始tick/价格/手数/撮合均未修改；新增三宿主提交回调是维持读档后权威配置一致所必需，没有变更wire存档格式；失败分段和远程乱序有对应短测试，未静默吞错。完整终端目标与其他全回归问题仍须继续，不能据本批复核通过宣称全局完成。
