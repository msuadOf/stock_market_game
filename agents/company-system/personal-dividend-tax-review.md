# 个人公开市场股息税独立复核记录

本批为未实施者复核记录。多轮新起与复用 Luna reviewer 均在平台侧 `thinking_signature_invalid` 处中断，未执行文件修改或得出代码结论；该失败与仓库 diff 无关，不能作为复核通过证据。

主 agent 随后按只读门禁复核本批相关源码：

- 税务身份只接受 `IndividualPublicMarket`，企业、基金、非居民与未配置身份显式保留不支持或 `TreatmentNotConfigured`，不从账户类型或策略风格推断。
- 税账以分红登记与持有 lot 为事实源，只在真实 `PersonalTradeConfirmation` 卖出时按 FIFO 消耗；未成交挂单不触发收缴，也不伪造成交。
- 持有期按自然日计算，不足一个月的卖出以真实卖出净额收缴 20%，不虚构税后现金流。收缴使用受控 `debit_cash`，余额不足显式失败，不给投资者自动补钱。
- Rust restore、Web strict parser 和三份 current fixtures 对同一 `dividend_tax_books` 契约建模；fixtures 相对 HEAD 仅添加空数组，并保持原单行 JSON 格式。

验证范围仅为本批定向短测：两个完整模块路径的 Engine 测试各 1 项通过；Web 公司行为 schema 13 项、当前存档契约 33 项和 `apps/web` TypeScript 检查通过。未运行完整回归。宿主/UI 配置入口、其他税务身份和其他股本行为仍不在完成范围。
