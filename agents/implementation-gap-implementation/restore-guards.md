# 恢复校验批：G69、G71、G75、G76、G78、G80

## 实现

- TradingPlan 期限校验共用于开户、serde、PlanBook 与完整 SaveSlot validator。先计算 horizon−1，MAX/1 保持合法；不改变订单有效期。
- CivilInstant serde 复用构造器秒域；CalendarPolicy 内容身份独立绑定每条 OfficialCoverageEntry 的 source_digest，默认空覆盖政策 digest 不变。
- OperatingScheduler 与 CivilClock 各自拒绝重复待办身份。序号游标 MAX 表示耗尽，MAX−1 是最后可分配值；不允许回绕、不复用旧 ID。失败在队列修改之前显式返回。时钟恢复按冻结政策核实所有 pending 日期；空队列零游标仍合法。
- 完整 SaveSlot 恢复调用 Bank EclPolicy::validate，错误包含公司 ID。独立 BankBooks serde 与发行错误次序未修改，四行业生产闭环不因此核销。

## 定向验证

构建独立使用 `CARGO_TARGET_DIR=.tmp/gap-target cargo test -p engine --test audit_restore_guards --test save_contract --no-run -j 16`，未运行全量测试。新依赖首次构建过程中有多个 rustc/编译任务；后续增量构建复用该专用目录。

测试 binary 使用 `RAYON_NUM_THREADS=8 timeout 10s <binary> <filter> --test-threads=8`，外部期限覆盖单条命令，以下每个 case 自然完成均不足10秒。

| 验证 | 红测 | 绿测 |
|---|---|---|
| audit_restore_guards 11项 | 原码10失败、1通过，0.01s | 11通过，0.01s |
| SaveSlot Bank 政策1项 | 原码错误安装损坏政策，失败，2.13s | 1通过，1.56s |
| SaveSlot 期限、scheduler、clock恢复3项 | 独立审查要求补充；没有冒称执行过该组旧码红测 | 3通过，3.92s |

完整恢复测试先确认未修改的合法基线可恢复，再核具体错误上下文。未来 clock due 作为独立追加待办测试，保留原经营镜像所需的待办，不用先破坏镜像制造假阳性。未运行回归、浏览器、性能矩阵或发布流程。

## 独立复核

非实施者 `restore_batch_review` 审查完整产品 diff 和所有新增文件，确认不改变 A 股语义、没有无关复杂度，提出 G69/G71/G78 缺完整 SaveSlot 定向测试。已补这3项并通过，最终新增测试与台账再次由该 reviewer 复核，A 股语义、必要性及边界/跨层三项门禁均通过，没有新增阻断发现；reviewer 未另行运行测试。
