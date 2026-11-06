# 参数与公司诊断叶独立复核汇总

本 reviewer 未实施各 leaf 的初版或修正版；逐完整 diff 复核后，仅对合并机械项去除无用 mut、为未接自动提案算法补当前 test cfg 边界说明、将 u32 的非正数比较等价缩成 ==0。主树自己实施的 WorkerContext、INFO 验证与 Admin 参数聚合仍须上级未实施者独审。

## args-tests

完整 8 文件原参数→命名字段逐项比对，14 个 GroupState::new 真实 consumer 均为 unit test 后代；生产 World 使用 new_with_initial_tick，仓库 tests/examples/apps 没有这个 constructor 消费。GroupSetup 是 test-only/runtime 范围可见，保字段求值与 new_with_initial_tick 传值顺序。ExpectedDayK 七处调用保所有量/额/笔/OHLC 断言及具体数字。初版格式与 redundant_field_names 发现已修，复核无源码阻断。

## settlement

初版拒绝：增加四容器 clone，read/write 可来自不同 authority。fresh 修正版 b21296d1 逐完整 diff 重审：只从同一 write context 在 prepare 调用中临时 reborrow，准备完成后再 apply；无 read_* clone，无双 authority，原 canonical 去重/receipt 选择/T+1/经验投影/patch 顺序不变。去除不必要的 mut binding，不更改 reborrow。源码通过，真实编译待统一窗口。

## args-leaf

完整10文件核对：open action 的 assessment/policy 是原注释已有事实组；不改字段值、原求值顺序和闭包捕获。RootReadContext 去掉纯赋字段工厂，原8字段初始化保序，无第二个同义输入类型。RetailCandidateInput 仅搬原只读引用，删除原未读取 plans 形参不丢表达式副作用。InformationDeliveryState 与 context 都保持只读借用。初版命名 closure 的参数类型与丢失的领域注释由 fresh b5ce43c3 修正，逐点重审无源码阻断。

## company

完整14文件 diff逐项看过；生产披露批更正仍用 from_owner_results，PreparedLegacy payload 与 source/proof 访问由生产 enum 引用故保留；旧 mutating bridge 和对应行业 tax owner adapter 只在单元测试引用，精确 test cfg。五个税账 accessor 和三个 IndustryBooks 转发均没有任何消费者，底层事实/序列化/正式按行业生产路径保留。科目6801改为原值相同的已有 TAX_EXP 常量，无虚假调用。Simple cfg 遵上级已核当前支持边界：偏好 config/ledger/ex_dates 生产保留，自动提案算法保完整源及强测试，仅 test 构建；不宣称自动日结已接通。

## 诊断9处

初版两个 Option 式漏删前导 ! 被独审拒绝，未入候选。fresh 修正版分别核 None/Some合法/Some非法，拒绝条件原/新均为 真/假/真；u32 的 <=0 等价为 ==0。CashDividend 的 a!=!b 与 a==b 四种布尔输入相同。六个 must_use Result 对应合法 fixture，expect 保显式失败；外来单场景按后续 Sol 勘正注释是合法跳过，不误改 expect_err。全部原后续断言保留。

## A 股与验证边界

沿用 ADR-0017/0037/0043 与 docs/trading-rules.md 已登记语义；未改变分/股单位、T+1、整手、受理顺序、交易阶段、公开信息真实 cutoff、资金可见时点或 JSON。只改变内部参数表达、可见性及精确测试构建范围。没有新增制度判断，未扩公开功能。原 Clippy stderr 为已存在红灯，尚未启动 Cargo，不能把源码审查/格式化称为测试或 Clippy 绿灯。
