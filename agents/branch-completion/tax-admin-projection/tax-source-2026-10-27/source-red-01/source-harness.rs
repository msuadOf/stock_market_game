#![allow(dead_code)]
extern crate engine;
pub use engine::{account,calendar,money,runtime,Money,AccountId,Side};
mod orderbook {pub use engine::orderbook::*;
pub(crate) mod js_safe_u64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub const MAX: u64 = 9_007_199_254_740_991;

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if *value > MAX {
            return Err(serde::ser::Error::custom(format!(
                "u64 value {value} exceeds JavaScript's safe integer range"
            )));
        }
        serializer.serialize_u64(*value)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        if value > MAX {
            return Err(serde::de::Error::custom(format!(
                "u64 value {value} exceeds JavaScript's safe integer range"
            )));
        }
        Ok(value)
    }
}
pub(crate) mod canonical_u128_decimal {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn parse(value: &str) -> Result<u128, String> {
        let bytes = value.as_bytes();
        let nonzero_prefix = bytes.first().is_some_and(|byte| (b'1'..=b'9').contains(byte));
        if bytes != b"0" && !(nonzero_prefix && bytes.iter().all(u8::is_ascii_digit)) {
            return Err("成交额必须使用规范非负十进制分字符串".to_owned());
        }
        value
            .parse::<u128>()
            .map_err(|error| format!("成交额超出 u128 范围：{error}"))
    }

    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
pub(crate) mod canonical_u64_decimal {
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
        let value = String::deserialize(deserializer)?;
        let bytes = value.as_bytes();
        let nonzero_prefix = bytes
            .first()
            .is_some_and(|byte| (b'1'..=b'9').contains(byte));
        let canonical = bytes == b"0" || (nonzero_prefix && bytes.iter().all(u8::is_ascii_digit));
        if !canonical {
            return Err(serde::de::Error::custom(
                "u64 must use canonical decimal notation",
            ));
        }
        value.parse::<u64>().map_err(serde::de::Error::custom)
    }
}
}
mod company {pub use engine::company::*;
#[path="/data1/baiyifan/workplace/stock_market_game/.worktree/tax-admin-projection/packages/engine/src/company/share_registry.rs"] pub mod share_registry;
#[path="/data1/baiyifan/workplace/stock_market_game/.worktree/tax-admin-projection/packages/engine/src/company/cash_dividend_tax.rs"] pub mod cash_dividend_tax;
}
