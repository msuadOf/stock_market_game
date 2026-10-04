use super::{PlanId, TradingPlan};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Default, Debug, Eq, PartialEq)]
pub(super) struct PlanRecords {
    root: Arc<Node>,
}

#[derive(Clone, Default, Debug, Eq, PartialEq)]
struct Node {
    children: BTreeMap<u8, Arc<Node>>,
    value: Option<(PlanId, TradingPlan)>,
}

impl PlanRecords {
    pub(super) fn get(&self, id: &PlanId) -> Option<&TradingPlan> {
        let mut node = self.root.as_ref();
        for byte in id.0.to_be_bytes() {
            node = node.children.get(&byte)?.as_ref();
        }
        node.value.as_ref().map(|(_, plan)| plan)
    }

    pub(super) fn get_mut(&mut self, id: &PlanId) -> Option<&mut TradingPlan> {
        self.get(id)?;
        let mut node = Arc::make_mut(&mut self.root);
        for byte in id.0.to_be_bytes() {
            node = Arc::make_mut(node.children.get_mut(&byte)?);
        }
        node.value.as_mut().map(|(_, plan)| plan)
    }

    pub(super) fn insert(&mut self, id: PlanId, plan: TradingPlan) {
        let mut node = Arc::make_mut(&mut self.root);
        for byte in id.0.to_be_bytes() {
            node = Arc::make_mut(node.children.entry(byte).or_default());
        }
        node.value = Some((id, plan));
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&PlanId, &TradingPlan)> {
        self.root.iter()
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = &PlanId> {
        self.iter().map(|(id, _)| id)
    }
}

impl Node {
    fn iter(&self) -> Box<dyn Iterator<Item = (&PlanId, &TradingPlan)> + '_> {
        Box::new(
            self.value
                .iter()
                .map(|(id, plan)| (id, plan))
                .chain(self.children.values().flat_map(|node| node.iter())),
        )
    }
}

impl From<BTreeMap<PlanId, TradingPlan>> for PlanRecords {
    fn from(plans: BTreeMap<PlanId, TradingPlan>) -> Self {
        let mut records = Self::default();
        for (id, plan) in plans {
            records.insert(id, plan);
        }
        records
    }
}

impl serde::Serialize for PlanRecords {
    fn serialize<Serializer: serde::Serializer>(
        &self,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error> {
        use serde::ser::SerializeMap;
        let mut entries = serializer.serialize_map(None)?;
        for (id, plan) in self.iter() {
            entries.serialize_entry(id, plan)?;
        }
        entries.end()
    }
}
