# R3a 冻结回执候选契约

基线：本地 main `b172c1ab1a69daa44dc57dddc887db255d7f6ce7`；独占树 `.worktree/share-transform-r3a`。仅恢复 engine 候选 substrate，不接生产、不修改 runtime。

## 范围与相位

消费经 `ShareSplitBook::validate` 校验的 Registered 账簿及其不可变 `RegistrationSnapshot` / `ShareSplitReceipt`。本片不生成缩股总量或碎股政策，也不取消现 domain 支持。`ShareReduction` 是已批准行政数值原语，不代表任意缩股产品已获批准。

输入包含 day、Registered book、R 原单位 registry、个人 tax books、显式 `SplitUnitContext` 链、显式 Treasury 桶。不接收 ex 日新单位市场请求；输出用显式 `PreNewUnitMarket` 相位标记。输出字段明确叫 `registry_before_new_unit_market` / `dividend_tax_books_before_new_unit_market`；不是最终当日账户或名册。原 registry holdings 必须与登记 snapshot 一致，禁止将已消耗新单位成交的名册作为原单位输入。

只输出 `ShareCredit` / `ShareReduction` 数值意图，保留 event、account、registered_before、converted_after；真实账户不写。成本政策沿 D1a：正仓保留 invested/recovered，实际归零清除；deferred 新单位成交可能令 actual position 非零，不能仅按登记 converted_after=0 就预判实际账户清仓。

Treasury 显式桶给出稳定 key、lot ids、before/after、可选真实 account。initial 库存不自动映射回购账户；各桶 before/after 分别勾稽冻结 Treasury 汇总；禁止跨桶重复 lot。由上游负责按回购实际成交和已验证单位链重放提供桶，本片不会凭整个 Treasury 数量伪造实买库存。税账沿用现 domain 既定取得日/非应税缩股核减，不冒充完整官方税务结论。

## 法源与未闭边界

2026-10-25 实时读取上交所现行《交易规则（2026年修订）》官方原文：[发布页](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/c_20260424_10816482.shtml)、[DOCX](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/10816482/files/e3a3b98e36894edca77841837936e006.docx)。4.3.1 规定权益登记日次一交易日除权除息；4.3.2 公式允许发行人申请调整；4.3.3 用参考价计算涨跌幅。本片不据此发明一般 A 股股份拆细或合并登记制度。

ADR-0039 仅授权范围并要求先核验制度。旧 `agents/company-system/share-split.md` 已承认缩股碎股、总量、R/R+1、税务期限没有统一明文；旧 floor(total/ratio) + 余数排序只是送转类比，未找到用户明确批准该细则的原文。反例：两户各1股、3并1，旧 domain 的总量为0、两户均归零。候选消费该冻结事实不等于批准此生产分配政策，产品/法源边界留 B5 显式关闭。

M 尚未交付：本片只验证显式正确日期/单位链输入，不以共享开市天数冒充逐股实际开市序列。R4 仍须解决先转换、后同日新单位市场名册消费的编排；现 ShareRegistry::close_day 不允许在转换同日追加 PublicMarket，本片不绕过此约束。
