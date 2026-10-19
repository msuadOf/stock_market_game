use super::{
    identity::IssuerRegistry, system::CompanyImplementation, CompanySystem, CompanySystemError,
};
use crate::accounting::AccountingAmount;

pub fn required_equity<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<AccountingAmount>, D::Error> {
    serde::Deserialize::deserialize(deserializer)
}

pub(crate) fn required_nullable<'de, T: serde::Deserialize<'de>, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    serde::Deserialize::deserialize(deserializer)
}

pub(crate) fn unique_map<'de, Key, Value, Deserializer>(
    deserializer: Deserializer,
) -> Result<std::collections::BTreeMap<Key, Value>, Deserializer::Error>
where
    Key: Ord + serde::Deserialize<'de>,
    Value: serde::Deserialize<'de>,
    Deserializer: serde::Deserializer<'de>,
{
    struct MapVisitor<Key, Value>(std::marker::PhantomData<(Key, Value)>);
    impl<'de, Key: Ord + serde::Deserialize<'de>, Value: serde::Deserialize<'de>>
        serde::de::Visitor<'de> for MapVisitor<Key, Value>
    {
        type Value = std::collections::BTreeMap<Key, Value>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("不含重复身份的映射")
        }
        fn visit_map<Access: serde::de::MapAccess<'de>>(
            self,
            mut access: Access,
        ) -> Result<Self::Value, Access::Error> {
            let mut values = std::collections::BTreeMap::new();
            while let Some((key, value)) = access.next_entry()? {
                if values.contains_key(&key) {
                    return Err(serde::de::Error::custom("映射含重复身份"));
                }
                values.insert(key, value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(MapVisitor(std::marker::PhantomData))
}

impl<'de> serde::Deserialize<'de> for CompanySystem {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct State {
            issuers: IssuerRegistry,
            implementation: CompanyImplementation,
        }
        let raw = State::deserialize(deserializer)?;
        let state = Self {
            issuers: raw.issuers,
            implementation: raw.implementation,
            hash_cache: super::system::CompanySystemHashCache::default(),
        };
        state
            .validate_restored()
            .map_err(serde::de::Error::custom)?;
        Ok(state)
    }
}

impl CompanySystem {
    pub fn export_state(&self) -> Self {
        self.clone()
    }
    pub fn validate_restored(&self) -> Result<(), CompanySystemError> {
        let checked =
            IssuerRegistry::new(self.issuers.iter().map(|(_, spec)| spec.clone()).collect())?;
        if checked != self.issuers {
            return Err(CompanySystemError::Invalid(
                "发行人 key 与身份不一致".into(),
            ));
        }
        match &self.implementation {
            CompanyImplementation::Simple(state) => state.validate(&self.issuers),
        }
    }
}
