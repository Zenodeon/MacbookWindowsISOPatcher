//! Model identity is separate from availability of a servicing recipe.
use crate::domain::MODEL_ID;

pub struct ModelProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub identifier: &'static str,
    pub details: &'static str,
}
impl ModelProfile {
    pub fn build_available(&self) -> bool {
        self.id == MODEL_ID
    }
    pub fn hardware_verified(&self) -> bool {
        self.id == MODEL_ID
    }
}
pub const MODEL_PROFILES: &[ModelProfile] = &[
    ModelProfile {
        id: MODEL_ID,
        name: "MacBook Pro 13-inch · 2016 · A1706",
        identifier: "MacBookPro13,2",
        details: "Iris 550 · Touch Bar · T1",
    },
    ModelProfile {
        id: "a1708-2016",
        name: "MacBook Pro 13-inch · 2016 · Two Thunderbolt 3 ports",
        identifier: "MacBookPro13,1",
        details: "A1708 · Without Touch Bar",
    },
    ModelProfile {
        id: "a1707-2016",
        name: "MacBook Pro 15-inch · 2016",
        identifier: "MacBookPro13,3",
        details: "A1707 · Touch Bar",
    },
    ModelProfile {
        id: "a1706-2017",
        name: "MacBook Pro 13-inch · 2017 · Four Thunderbolt 3 ports",
        identifier: "MacBookPro14,2",
        details: "A1706 · Touch Bar",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_a1706_generations_and_does_not_reuse_its_recipe() {
        let available: Vec<_> = MODEL_PROFILES
            .iter()
            .filter(|model| model.build_available())
            .collect();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].identifier, "MacBookPro13,2");
        let later_a1706 = MODEL_PROFILES
            .iter()
            .find(|model| model.identifier == "MacBookPro14,2")
            .unwrap();
        assert!(!later_a1706.build_available());
        assert!(!later_a1706.hardware_verified());
        let ids: std::collections::BTreeSet<_> =
            MODEL_PROFILES.iter().map(|model| model.id).collect();
        assert_eq!(ids.len(), MODEL_PROFILES.len());
    }
}
