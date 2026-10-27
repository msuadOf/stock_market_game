# Proposal parser 主线精确移植

授权范围：既有 main-merge worktree 从 b9cdc70d 创建 topic，精确 cherry-pick 84173142 和 6b1800bc。两个提交只引入两个正式 parser/test 文件及本主题工作记录，无 B/C、host、engine、API 或 generated 增量；未推送。

移植后的第一次普通验证：Node11/11、目标lint0；strict/noUnused包含正式测试后发现第151行 structuredClone 保留 Readonly 类型，delete触发TS2542。原输出在本目录types.stdout，receipt.json真实记录失败，不覆盖。原作者只检查parser/money的绿不冒充包含测试的绿。

唯一修复是对用于缺字段测试的局部副本增加Record<string,unknown>注解；fixture输入和断言不变，运行时字节不变。green-v2三job并发重跑：11/11、strict/noUnused含test、目标lint全部0；每命令真实main run-with-deadline外10000ms、Node case10000ms/concurrency4。PlanId、AccountId、money.ts和main supervisor的哈希一并记录；其内容未被移植修改。

原叶片功能与新TDD的独立核证已完成；移植净diff及上述类型注解仍等未实施者独立复核后才允许本地FF main。本片不声明生产提案入口、真实host、Outcome或全B/C通过。

未实施者sol_resume_independent_review已独立确认b9→87dde完整44path仅parser2+agents文本，production源码与6b18逐字相同；唯一后续局部Record注解未改变输入或断言，green-v2哈希及实际11/TS含test/lint0吻合，PlanId{owner:AccountId,sequence:string}与AccountId字符串无diff，批准本地FF。此裁决只覆盖Proposal输入parser精确移植。
