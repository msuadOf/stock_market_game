extern crate engine;

mod accounting {
    pub use engine::accounting::*;
}
mod calendar {
    pub use engine::calendar::*;

    pub fn days_in_month(year: i32, month: u8) -> u8 {
        let next_month = if month == 12 {
            CivilDate::from_ymd(year + 1, 1, 1)
        } else {
            CivilDate::from_ymd(year, month + 1, 1)
        };
        next_month.unwrap().prev().unwrap().day()
    }
}
mod information {
    pub use engine::information::industry_presentation;
}
mod company {
    pub mod api {
        pub use engine::company::api::PeriodAmounts;
    }
    pub(crate) use crate::income_tax;
    pub use engine::company::{CompanyId, CompanyKind, CompanySystemError};
    pub mod industrial {
        pub use engine::company::industrial::industrial_account_chart;
    }
    pub mod bank {
        pub use engine::company::bank::bank_account_chart;
    }
    pub mod insurance {
        pub use engine::company::insurance::insurance_account_chart;
    }
    pub mod real_estate {
        pub use engine::company::real_estate::real_estate_account_chart;
    }
    pub mod rng {
        pub use engine::company::rng::OperatingRng;
    }
    pub mod persistence {
        pub fn required_nullable<
            'de,
            Value: serde::Deserialize<'de>,
            Deserializer: serde::Deserializer<'de>,
        >(
            deserializer: Deserializer,
        ) -> Result<Option<Value>, Deserializer::Error> {
            serde::Deserialize::deserialize(deserializer)
        }
    }
}
mod orderbook {
    pub mod canonical_u64_decimal {
        pub fn serialize<Serializer: serde::Serializer>(
            value: &u64,
            serializer: Serializer,
        ) -> Result<Serializer::Ok, Serializer::Error> {
            serializer.serialize_str(&value.to_string())
        }
        pub fn deserialize<'de, Deserializer: serde::Deserializer<'de>>(
            deserializer: Deserializer,
        ) -> Result<u64, Deserializer::Error> {
            let value: String = serde::Deserialize::deserialize(deserializer)?;
            let bytes = value.as_bytes();
            let nonzero_prefix = bytes
                .first()
                .is_some_and(|byte| (b'1'..=b'9').contains(byte));
            let canonical =
                bytes == b"0" || (nonzero_prefix && bytes.iter().all(u8::is_ascii_digit));
            if !canonical {
                return Err(serde::de::Error::custom(
                    "u64 must use canonical decimal notation",
                ));
            }
            value.parse().map_err(serde::de::Error::custom)
        }
    }
}

#[path = "../../packages/engine/src/company/simple/finance.rs"]
mod finance;
#[path = "../../packages/engine/src/company/simple/finance_config.rs"]
mod finance_config;
#[path = "../../packages/engine/src/company/income_tax.rs"]
pub(crate) mod income_tax;
