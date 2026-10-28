//! 个人信息信念使用的会话锚；不代替市场 ordinal 或实际完成会话数。
//! 历史顺序与期限 position 分开：Closed 不额外消耗期限，但也不是此前 Trading 时点。

use std::cmp::Ordering;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 本股个人信息时点；当前值必须来自同 source/day 的真实日历投影。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InformationTradingAnchor {
    NoPriorSession,
    TradingDay { ordinal: u64 },
    DayBefore { ordinal: u64 },
}

impl InformationTradingAnchor {
    /// ordinal 先比较，同 ordinal 再按 Trading、随后 Closed 排序；MAX 不参与倍乘。
    pub(crate) fn chronological_cmp(self, other: Self) -> Ordering {
        let key = |anchor| match anchor {
            Self::NoPriorSession => None,
            Self::TradingDay { ordinal } => Some((ordinal, 0_u8)),
            Self::DayBefore { ordinal } => Some((ordinal, 1_u8)),
        };
        key(self).cmp(&key(other))
    }
}

impl Serialize for InformationTradingAnchor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (kind, ordinal) = match self {
            Self::NoPriorSession => ("NoPriorSession", None),
            Self::TradingDay { ordinal } => ("TradingDay", Some(*ordinal)),
            Self::DayBefore { ordinal } => ("DayBefore", Some(*ordinal)),
        };
        let mut state = serializer.serialize_struct(
            "InformationTradingAnchor",
            if ordinal.is_some() { 2 } else { 1 },
        )?;
        state.serialize_field("kind", kind)?;
        if let Some(ordinal) = ordinal {
            state.serialize_field("ordinal", &ordinal.to_string())?;
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for InformationTradingAnchor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AnchorVisitor;
        impl<'de> Visitor<'de> for AnchorVisitor {
            type Value = InformationTradingAnchor;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("带 kind 和必要 canonical decimal ordinal 的个人信息时点对象")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut kind: Option<String> = None;
                let mut ordinal = None;
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "kind" => {
                            if kind.is_some() {
                                return Err(de::Error::duplicate_field("kind"));
                            }
                            kind = Some(map.next_value()?);
                        }
                        "ordinal" => {
                            if ordinal.is_some() {
                                return Err(de::Error::duplicate_field("ordinal"));
                            }
                            let raw: String = map.next_value()?;
                            if raw.is_empty()
                                || !raw.bytes().all(|byte| byte.is_ascii_digit())
                                || (raw.len() > 1 && raw.starts_with('0'))
                            {
                                return Err(de::Error::custom(
                                    "ordinal 必须是规范 ASCII 十进制 u64 字符串",
                                ));
                            }
                            ordinal = Some(raw.parse::<u64>().map_err(de::Error::custom)?);
                        }
                        _ => return Err(de::Error::unknown_field(&field, &["kind", "ordinal"])),
                    }
                }
                let kind = kind.ok_or_else(|| de::Error::missing_field("kind"))?;
                match kind.as_str() {
                    "NoPriorSession" if ordinal.is_none() => {
                        Ok(InformationTradingAnchor::NoPriorSession)
                    }
                    "NoPriorSession" => Err(de::Error::custom("NoPriorSession 不允许 ordinal")),
                    "TradingDay" => Ok(InformationTradingAnchor::TradingDay {
                        ordinal: ordinal.ok_or_else(|| de::Error::missing_field("ordinal"))?,
                    }),
                    "DayBefore" => Ok(InformationTradingAnchor::DayBefore {
                        ordinal: ordinal.ok_or_else(|| de::Error::missing_field("ordinal"))?,
                    }),
                    _ => Err(de::Error::unknown_variant(
                        &kind,
                        &["NoPriorSession", "TradingDay", "DayBefore"],
                    )),
                }
            }
        }
        deserializer.deserialize_map(AnchorVisitor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InformationHorizonError {
    #[error("个人期限必须为正")]
    ZeroHorizon,
    #[error("信息时点回拨：anchor={anchor:?}, as_of={as_of:?}")]
    TimeReversed {
        anchor: InformationTradingAnchor,
        as_of: InformationTradingAnchor,
    },
    #[error("个人期限余数无法表示")]
    ArithmeticOverflow,
}

/// 此守卫只核必要历史上界，不认证某个历史 Closed 日期确实存在。
pub(crate) fn ensure_information_anchor_not_after(
    anchor: InformationTradingAnchor,
    as_of: InformationTradingAnchor,
) -> Result<(), InformationHorizonError> {
    if anchor.chronological_cmp(as_of).is_gt() {
        return Err(InformationHorizonError::TimeReversed { anchor, as_of });
    }
    Ok(())
}

/// 编号位置只用于个人期限，不证明某个会话已经完成。
pub(crate) fn information_session_position(anchor: InformationTradingAnchor) -> u128 {
    match anchor {
        InformationTradingAnchor::NoPriorSession => 0,
        InformationTradingAnchor::TradingDay { ordinal }
        | InformationTradingAnchor::DayBefore { ordinal } => u128::from(ordinal) + 1,
    }
}

/// 数学 position 不认证历史与来源；业务须先校验完整 anchor 顺序及真实投影。
pub(crate) fn information_horizon_remaining(
    anchor: InformationTradingAnchor,
    horizon: u16,
    as_of: InformationTradingAnchor,
) -> Result<u16, InformationHorizonError> {
    if horizon == 0 {
        return Err(InformationHorizonError::ZeroHorizon);
    }
    let anchor_position = information_session_position(anchor);
    let as_of_position = information_session_position(as_of);
    if as_of_position < anchor_position {
        return Err(InformationHorizonError::TimeReversed { anchor, as_of });
    }
    let expiry = anchor_position + u128::from(horizon);
    if as_of_position >= expiry {
        return Ok(0);
    }
    u16::try_from(expiry - as_of_position).map_err(|_| InformationHorizonError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests;
