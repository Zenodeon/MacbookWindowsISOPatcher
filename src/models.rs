//! Buildable model identities and their hardware verification status.
use crate::domain::MODEL_ID;

pub struct ModelProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub identifier: &'static str,
    pub details: &'static str,
}
impl ModelProfile {
    pub fn hardware_verified(&self) -> bool {
        self.id == MODEL_ID
    }
}
pub const MODEL_PROFILES: &[ModelProfile] = &[
    ModelProfile {
        id: MODEL_ID,
        name: "MacBook Pro 13-inch · 2016 · Four Thunderbolt 3 ports",
        identifier: "MacBookPro13,2",
        details: "A1706 · Intel Iris 550 · Touch Bar · T1",
    },
    ModelProfile {
        id: "a1708-2016",
        name: "MacBook Pro 13-inch · 2016 · Two Thunderbolt 3 ports",
        identifier: "MacBookPro13,1",
        details: "A1708 · Intel Iris 540 · Without Touch Bar",
    },
    ModelProfile {
        id: "a1707-2016",
        name: "MacBook Pro 15-inch · 2016 · Four Thunderbolt 3 ports",
        identifier: "MacBookPro13,3",
        details: "A1707 · Intel HD 530 + Radeon Pro 450/455/460 · Touch Bar · T1",
    },
    ModelProfile {
        id: "a1706-2017",
        name: "MacBook Pro 13-inch · 2017 · Four Thunderbolt 3 ports",
        identifier: "MacBookPro14,2",
        details: "A1706 · Intel Iris Plus 650 · Touch Bar · T1",
    },
    ModelProfile {
        id: crate::domain::IMAC_MODEL_ID,
        name: "iMac 21.5-inch · 2017 · Core i5",
        identifier: "iMac18,1",
        details: "Intel Iris Plus 640",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_a1706_generations_and_their_verification_status() {
        let available: Vec<_> = MODEL_PROFILES
            .iter()
            .filter(|model| model.hardware_verified())
            .collect();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].identifier, "MacBookPro13,2");
        let later_a1706 = MODEL_PROFILES
            .iter()
            .find(|model| model.identifier == "MacBookPro14,2")
            .unwrap();
        assert!(!later_a1706.hardware_verified());
        let ids: std::collections::BTreeSet<_> =
            MODEL_PROFILES.iter().map(|model| model.id).collect();
        assert_eq!(ids.len(), MODEL_PROFILES.len());
    }
}
