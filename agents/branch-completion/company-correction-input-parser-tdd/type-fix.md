# green-v1 测试类型诊断修复

green-v1 的 Node13/13、lint退出0，strict TS真实失败。原因：嵌入的真实Rust金额probe包含normalized/error两种字段，测试标注未列出；另将接口转为动态字段Record需经过unknown。只修两个类型标注，没有改fixture值、case、输入或断言。实现前冻结规格和red-v1同字节快照原样保留，不把旧红冒充当前测试文件SHA。
