extern crate engine;

mod accounting {
    pub use engine::accounting::AccountingAmount;
}

mod calendar {
    pub use engine::calendar::CivilDate;
}

mod company {
    pub use engine::company::CompanySystemError;

    pub mod rng {
        pub use engine::company::rng::OperatingRng;
    }
    pub mod api {
        pub use engine::company::api::PeriodAmounts;
    }

    pub mod persistence {
        pub fn required_nullable<'de, T: serde::Deserialize<'de>, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<T>, D::Error> {
            serde::Deserialize::deserialize(deserializer)
        }
    }
}

#[path = "../../packages/engine/src/company/simple/growth.rs"]
mod growth;

#[path = "../../packages/engine/src/company/simple/period.rs"]
mod period;
