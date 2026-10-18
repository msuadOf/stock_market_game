//! u64 的十进制字符串 serde 约定（JS 安全整数边界的 wire 契约）。
//!
//! 中立位置：session 与 company 等域都可能承载需要跨端传输的 u64 字段；
//! 依赖方向规则是 session → company 合法、company → session 非法，因此该
//! helper 放在 crate 根，双方从 `crate::u64_decimal` 引用，company 生产代码
//! 不再经由 `crate::session::u64_decimal` 反向依赖会话（Astra #15 复核阻断项）。

use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer)?
        .parse::<u64>()
        .map_err(serde::de::Error::custom)
}
