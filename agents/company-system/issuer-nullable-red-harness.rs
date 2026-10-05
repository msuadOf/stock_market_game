extern crate engine;

use engine::company::{CompanyId, CompanyKind, CompanySpec, IndustryId};

#[test]
fn issuer_requires_explicit_nullable_fields() {
    let spec = CompanySpec {
        id: CompanyId("nullable-test".into()),
        name: "虚拟发行人".into(),
        industry: IndustryId("虚拟行业".into()),
        kind: CompanyKind::Industrial,
        listed_stock: None,
        issued_shares: 1000,
        group_parent: None,
    };
    for field in ["listed_stock", "group_parent"] {
        let mut encoded = serde_json::to_value(&spec).unwrap();
        encoded.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<CompanySpec>(encoded).is_err(), "{field}");
    }
}
