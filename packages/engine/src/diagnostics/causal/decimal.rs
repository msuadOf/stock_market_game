use serde::{ser::SerializeSeq, Serialize, Serializer};

pub(super) fn optional<S: Serializer>(
    value: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value.map(|number| number.to_string()).serialize(serializer)
}

pub(super) fn delays<S: Serializer>(
    values: &[(u64, i64)],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut sequence = serializer.serialize_seq(Some(values.len()))?;
    for (identity, delay) in values {
        sequence.serialize_element(&(identity.to_string(), delay))?;
    }
    sequence.end()
}
