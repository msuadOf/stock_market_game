use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => crate::orderbook::js_safe_u64::serialize(value, serializer),
        None => serializer.serialize_none(),
    }
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    #[derive(Deserialize)]
    struct Safe(#[serde(with = "crate::orderbook::js_safe_u64")] u64);
    Option::<Safe>::deserialize(deserializer).map(|value| value.map(|safe| safe.0))
}
