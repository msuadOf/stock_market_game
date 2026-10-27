# engine 首红十例的独立静态复核

未实施 reviewer：原生 `/root/sol_company_queries_28/query_red_runner_review`，工具显式指定 `gpt-6.1-sol/high`。本记录按实际消息保存结论，不将其只读核查改写为执行验证。reviewer 未运行 Cargo／runner／测试，未修改源文件。

## 初轮发现与修复

1. 原 launcher 固定 anchor 后调用相对 `300000ms` 的 long helper；若 Node 启动前被暂停，外层真实截止会超过 anchor+300 秒。修复为本批局部 `absolute-supervisor.mjs`，在 import／校验完成后、真 spawn 前最后一步取 `outerDeadline-Date.now()`，不足 1000ms 拒绝启动，复用已审 `runBoundedCommand`，cleanup reserve 固定 1000ms。CLI 失败沿既有短强退，未修改共享生产 helper。
2. 原 P1 通用“至少一红”条件可把严格输入 case 也失败的结果误记为完成。修复为本片指定九红一绿；九个红必须命中测试 query 断言位置，五个 unwrap 还必须出现明确 stub 原因，拒绝 fixture setup 的提前 panic。
3. 原七例漏两个本人非空及 CompanyId／StockId 不等正控。root 明确授权在同一尚未执行 stub 上补三例；不是事后补写红证据。旧七例准备另存 `prepared-seven-case-v1/`。
4. 新 Rights fixture 最初给两个 holder 相同 100 股，两个 owner view 同为 10 股权利，错误固定查他人仍可能过。修复为真实 `join_market` 新 member，0／other 分别 100／200 股，名册总 300 股，权利分别 10／20，并先断言真实领域 owner views 不相等。
5. writer 静态核对时发现原 `crate::company::error::CompanyErrorClass` 引用 private 模块，改用现有 public reexport；reviewer 承认其初轮未发现该项，复核确认修正正确。原快照保留；没有把编译错称为业务红。

## 最终冻结结论

**`ENGINE_RED10_READY`，仅表示静态准备就绪。slot 与 protectedPrior 仍 pending，不能据此 launch，尚无实际首红结果。**

- plan：`25161802f14156dac77bc6cae2f33a0d63a9864129ac121280ac9fd0e77925a2`
- source-freeze：`de3b90ef75ee523205117e243bbcf21461ba8589753d07807d33b34de1adc1c4`
- tests：`b5fdc11d3919e90f98fdc0abb7bef12c9404dec0565cdfaf94f424823ca35057`
- runner：`dd8ed9acd24a509325a0802b9dedbea1942b139ee0ff4a1c1461070d99babd8f`
- 两份 manifest 的 893 项 productSources、7 项 runnerSources 一致，reviewer 已逐项核当前哈希；九个预期 panic 行全部对齐。
- Rights 日期经现模拟日历控制流核对；CompanySystem 两公司代码排序形成 unlisted 0／listed 1，而 StockCatalog 仅证券 0。公司 1 的收入不同，错误用股票数值 0 查询公司无法通过相等断言。

资源范围为单 engine lib 的一次长构建，j8／INC0、统一 RAM target、十个短 exact 最多八进程并发、每命令树 10000ms、共享 297／299／300 秒执行／清理发布／外层绝对截止。实际资源利用与退出收口仍需运行原件及 root 外层 actual wait 证明。

最后一项窄核追加：actual artifact 门补为恰一 engine、`features=[]`、`fresh=false`，明确拒绝将旧 fresh 产物称为本次新编译。原 reviewer 已复核短路条件、新 runner／plan／freeze 与全部 893／7 指纹。此后执行准备以 runner `23dbd3771a1856a7221b495a250c4d0e814ae41a3e506ef867fb7e471e4e6bbe`、plan `7d13e46605346b4a875adb069d0ec46e12e8ac27b0c7dbb914aec202d045ca49`、freeze `545c1a2f9cce8cc8b393fc9942ed7c0256e74508925c8a1611aed07894adc029` 为准；上方旧 hash 保留为历史，不是当前执行身份。真实前槽绑定仍未到，未启动。

AGENTS 三问：未改变 A 股交易制度或精确单位；修改为本只读 query 的首红所必需且范围较小；已补此前指出的查询与身份边界。该准备不证明三宿主已注册、六项 wire 已实测或公司行为生产结算已完成。
