# Correction 输入 parser 重新实施规格

基线 e4db6995，现有空树唯一 writer。未读取旧 parser 实现；依据真实 correction_commands_types.rs、journal.rs、amount.rs、date.rs 和独立 CIP probe 重做。

四个入口分别解析 request/status 对象与 raw JSON。精确字段递归校验；公司及公开 ID 为 u32，source 为可无损 JS safe u64 数值；所有 unsigned 拒绝 -0。raw JSON 在 JSON.parse 舍入或重复键覆盖前拒绝不规范数字 token、解码后重复 key、孤立 surrogate。合法 Unicode pair、特殊 own key、NUL key 保留。

AccountingAmount 为元字符串，使用 Rust Unicode White_Space trim、可选符号、前导零、至多两位小数；接受负零与正负 i128::MAX 分，拒绝 i128::MIN；原样保留输入。日期1900..2199合法公历，秒0..86399，枚举来自真实 Rust。

仅结构 parser，不做凭证平衡、幂等、受理、批准、队列、报告发布或 Outcome。先冻结本规格与测试，再保存 stub 红灯。旧版历史缺口不豁免、不补造。
