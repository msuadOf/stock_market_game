# WASM 句柄耗尽与会话保全

## 当前契约

实现审计 Q20 的真实风险是 AtomicU32 无条件递增在边界回绕并覆盖活会话。
当前明确使用单调、不复用的 Worker 内句柄：1..u32::MAX 可各分配一次；最后一个
合法句柄分配后，0 仅表示耗尽，不返回给 JS，不因删除会话重新发号。
分配由 fetch_update 的 SeqCst CAS 完成，多个请求不会拿到同一编号。
耗尽明确返回 SessionError::ResourceLimit，已有会话可继续使用；新的分配空间需要
重新启动 Worker。HashMap Entry 另显式拒绝已占句柄，保留原会话，报告内部不变量错误。

create/restore 只在成功构建后申请句柄，共用同一注册路径；非法构造不会消费编号。
此项只处理 adapter 资源身份，不改变交易账户、A 股撮合/费用、日终存档或任何格式版本，
没有旧名字 alias、会话迁移、格式兼容或静默重试。

## 短验证与独立复核

先为注册器提供可注入的 AtomicU32 测试接缝，生产仍使用原全局 NEXT。
边界行为红测真实复现第三次注册得到0号、注册表3条而非2条。首次测试误用不存在的
ProtocolSession::seed 导致编译失败，不冒充行为红测；改用 game().save().seed 后才取得
上述行为失败。修复后新增三类短例：末尾耗尽保全/删除不重用、已占编号不覆盖、8线程
32次申请仅16个唯一合法编号且16个显式耗尽错误。

Cargo targeted no-run 编译使用 jobs=32 和300000ms进程外监督；实际产物为
`target/debug/deps/web_wasm-ab55f7c3c3bb294a`。root运行全部14个普通case，threads=8，
10000ms外部deadline，14/14通过，0.82秒。非作者 review_remaining_engineering 全文复核
最终diff并独立运行同14例，14/14通过，0.81秒；三项门禁通过。
未执行浏览器WASM实机验收或完整回归，不以边界注入声称已运行数十亿次创建。
