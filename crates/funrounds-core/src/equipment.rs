// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{KillLoadoutStart, Mode};
use rand::seq::IndexedRandom;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
/// Inventory category for supported canonical item IDs.
pub enum WeaponSlot {
    Primary = 0,
    Secondary = 1,
    Knife = 2,
    /// Extra equipment category; unlike firearm slots, it can contain multiple items.
    Extra = 3,
}

/// Converts a canonical item ID to the engine classname used at the adapter boundary.
const ITEM_CLASSES: &[(&str, &str)] = &[
    ("ak47", "weapon_ak47"),
    ("m4a1", "weapon_m4a1"),
    ("m4a1_silencer", "weapon_m4a1_silencer"),
    ("galilar", "weapon_galilar"),
    ("famas", "weapon_famas"),
    ("aug", "weapon_aug"),
    ("sg556", "weapon_sg556"),
    ("mag7", "weapon_mag7"),
    ("awp", "weapon_awp"),
    ("ssg08", "weapon_ssg08"),
    ("g3sg1", "weapon_g3sg1"),
    ("scar20", "weapon_scar20"),
    ("mac10", "weapon_mac10"),
    ("mp9", "weapon_mp9"),
    ("mp7", "weapon_mp7"),
    ("mp5sd", "weapon_mp5sd"),
    ("ump45", "weapon_ump45"),
    ("p90", "weapon_p90"),
    ("bizon", "weapon_bizon"),
    ("nova", "weapon_nova"),
    ("xm1014", "weapon_xm1014"),
    ("sawedoff", "weapon_sawedoff"),
    ("m249", "weapon_m249"),
    ("negev", "weapon_negev"),
    ("glock", "weapon_glock"),
    ("hkp2000", "weapon_hkp2000"),
    ("usp_silencer", "weapon_usp_silencer"),
    ("p250", "weapon_p250"),
    ("fiveseven", "weapon_fiveseven"),
    ("tec9", "weapon_tec9"),
    ("cz75a", "weapon_cz75a"),
    ("deagle", "weapon_deagle"),
    ("elite", "weapon_elite"),
    ("revolver", "weapon_revolver"),
    ("knife", "weapon_knife"),
    ("taser", "weapon_taser"),
    ("hegrenade", "weapon_hegrenade"),
    ("flashbang", "weapon_flashbang"),
    ("smokegrenade", "weapon_smokegrenade"),
    ("molotov", "weapon_molotov"),
    ("incgrenade", "weapon_incgrenade"),
    ("decoy", "weapon_decoy"),
    ("c4", "weapon_c4"),
];

/// Converts a canonical item ID to the engine classname used at the adapter boundary.
pub fn item_classname(item: &str) -> Option<&'static str> {
    ITEM_CLASSES.iter().find_map(|(id, classname)| (*id == item).then_some(*classname))
}

/// Normalizes a canonical item ID, event item name, or engine classname to its canonical ID.
pub fn item_id(item: &str) -> Option<&'static str> {
    let item = item.strip_prefix("weapon_").unwrap_or(item);
    match item {
        "knife_t" | "knife_ct" | "knife_default_ct" | "knife_default_t" | "knife_projectile" => Some("knife"),
        _ => ITEM_CLASSES.iter().find_map(|(id, _)| (*id == item).then_some(*id)),
    }
}

/// Returns the loadout slot used by a supported canonical item ID.
pub fn weapon_slot(weapon: &str) -> Option<WeaponSlot> {
    match weapon {
        "ak47" | "m4a1" | "m4a1_silencer" | "galilar" | "famas" | "aug" | "sg556" | "mag7" | "awp" | "ssg08" | "g3sg1" | "scar20" | "mac10"
        | "mp9" | "mp7" | "mp5sd" | "ump45" | "p90" | "bizon" | "nova" | "xm1014" | "sawedoff" | "m249" | "negev" => Some(WeaponSlot::Primary),
        "glock" | "hkp2000" | "usp_silencer" | "p250" | "fiveseven" | "tec9" | "cz75a" | "deagle" | "elite" | "revolver" => {
            Some(WeaponSlot::Secondary)
        }
        "knife" => Some(WeaponSlot::Knife),
        "taser" | "hegrenade" | "flashbang" | "smokegrenade" | "molotov" | "incgrenade" | "decoy" => Some(WeaponSlot::Extra),
        _ => None,
    }
}

impl Mode {
    // Each listed slot supplies one item; an omitted slot stays empty.
    pub fn select_equipment(&self, rng: &mut impl rand::Rng) -> Result<Vec<&str>, String> {
        if self.kill_loadout {
            match self.kill_loadout_start.unwrap_or(KillLoadoutStart::Glock) {
                KillLoadoutStart::Glock => {
                    if !self.weapons.iter().any(|weapon| weapon == "glock") {
                        return Err(format!("{}: kill loadout requires an initial Glock", self.id));
                    }
                    return Ok(vec!["glock"]);
                }
                KillLoadoutStart::Random => {}
            }
        }
        if self.random_loadout {
            let candidates = self.loadout_candidates();
            if candidates.is_empty() {
                return Err(format!("{}: random loadout candidates must not be empty", self.id));
            }
            return Ok(vec![
                candidates.choose(rng).copied().expect("nonempty candidates"),
                candidates.choose(rng).copied().expect("nonempty candidates"),
            ]);
        }
        let mut candidates: [Vec<&str>; 4] = Default::default();
        for weapon in &self.weapons {
            if weapon == "knife" {
                return Err(format!("{}: knife is configured separately from equipment", self.id));
            }
            let slot = weapon_slot(weapon).ok_or_else(|| format!("{}: unsupported weapon: {weapon}", self.id))?;
            let pool = &mut candidates[slot as usize];
            if !pool.contains(&weapon.as_str()) {
                pool.push(weapon);
            }
        }
        Ok(candidates.iter().filter_map(|pool| pool.choose(rng).copied()).collect())
    }

    /// Select a configured kill-upgrade weapon, preferring a different entity class.
    /// If the configured pool contains only excluded classes, retain its legal behavior.
    pub fn select_kill_weapon_excluding(&self, rng: &mut impl rand::Rng, excluded: &[&str]) -> Result<&str, String> {
        if !self.kill_loadout {
            return Err(format!("{}: kill loadout is disabled", self.id));
        }
        let candidates = self.loadout_candidates();
        let preferred = candidates.iter().copied().filter(|weapon| !excluded.contains(weapon)).collect::<Vec<_>>();
        preferred
            .choose(rng)
            .or_else(|| candidates.choose(rng))
            .copied()
            .ok_or_else(|| format!("{}: kill loadout candidates must not be empty", self.id))
    }

    fn loadout_candidates(&self) -> Vec<&str> {
        let mut candidates = Vec::new();
        for weapon in self.weapons.iter().map(String::as_str).filter(|weapon| *weapon != "knife") {
            if !candidates.contains(&weapon) {
                candidates.push(weapon);
            }
        }
        candidates
    }
}

pub fn self_push_force(weapon: &str) -> Option<f32> {
    match crate::item_id(weapon)? {
        "nova" | "mag7" | "sawedoff" | "xm1014" | "ssg08" => Some(700.0),
        "awp" | "scar20" | "g3sg1" => Some(1300.0),
        "deagle" | "revolver" => Some(450.0),
        "taser" => Some(135.0),
        weapon if matches!(weapon_slot(weapon), Some(WeaponSlot::Primary | WeaponSlot::Secondary)) => Some(135.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::weapon_slot;
    use crate::Config;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn generic_example_selects_one_item_per_listed_slot() {
        let config = Config::parse(include_str!("../../../config/funrounds.example.toml")).unwrap();
        let mut rng = StdRng::seed_from_u64(0);
        assert_eq!(config.modes[0].select_equipment(&mut rng).unwrap(), ["famas", "p250"]);
    }

    #[test]
    fn rejects_invalid_equipment_and_deduplicates_candidates() {
        let mut mode = Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        let mut rng = StdRng::seed_from_u64(0);
        mode.weapons = vec!["famas".into(), "famas".into()];
        assert_eq!(mode.select_equipment(&mut rng).unwrap(), ["famas"]);
        mode.weapons = vec!["knife".into()];
        assert!(mode.select_equipment(&mut rng).is_err());
        mode.weapons = vec!["shield".into()];
        assert!(mode.select_equipment(&mut rng).is_err());
        mode.weapons.clear();
        assert!(mode.select_equipment(&mut rng).unwrap().is_empty());
    }

    #[test]
    fn recognizes_all_ordinary_guns_and_extra_equipment() {
        let primaries = [
            "ak47",
            "m4a1",
            "m4a1_silencer",
            "galilar",
            "famas",
            "aug",
            "sg556",
            "mag7",
            "awp",
            "ssg08",
            "g3sg1",
            "scar20",
            "mac10",
            "mp9",
            "mp7",
            "mp5sd",
            "ump45",
            "p90",
            "bizon",
            "nova",
            "xm1014",
            "sawedoff",
            "m249",
            "negev",
        ];
        let secondaries = [
            "glock",
            "hkp2000",
            "usp_silencer",
            "p250",
            "fiveseven",
            "tec9",
            "cz75a",
            "deagle",
            "elite",
            "revolver",
        ];
        let extras = ["taser", "hegrenade", "flashbang", "smokegrenade", "molotov", "incgrenade", "decoy"];
        assert!(
            primaries
                .iter()
                .all(|weapon| matches!(weapon_slot(weapon), Some(super::WeaponSlot::Primary)))
        );
        assert!(
            secondaries
                .iter()
                .all(|weapon| matches!(weapon_slot(weapon), Some(super::WeaponSlot::Secondary)))
        );
        assert!(matches!(weapon_slot("knife"), Some(super::WeaponSlot::Knife)));
        assert!(extras.iter().all(|weapon| matches!(weapon_slot(weapon), Some(super::WeaponSlot::Extra))));
        assert_eq!(primaries.len(), 24);
        assert_eq!(secondaries.len(), 10);
        assert_eq!(extras.len(), 7);
        assert!(weapon_slot("c4").is_none());
    }

    #[test]
    fn equipment_selects_at_most_one_candidate_for_each_available_slot() {
        let mut mode = Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        mode.weapons = vec!["famas".into(), "p250".into(), "hegrenade".into()];
        let mut rng = StdRng::seed_from_u64(44);
        let equipment = mode.select_equipment(&mut rng).unwrap();
        assert_eq!(equipment.len(), 3);
        assert!(matches!(weapon_slot(equipment[0]), Some(super::WeaponSlot::Primary)));
        assert!(matches!(weapon_slot(equipment[1]), Some(super::WeaponSlot::Secondary)));
        assert!(matches!(weapon_slot(equipment[2]), Some(super::WeaponSlot::Extra)));
    }

    #[test]
    fn random_and_kill_loadouts_use_their_candidate_lists() {
        let mut mode = Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        mode.random_loadout = true;
        mode.weapons = vec!["famas".into(), "p250".into()];
        let mut rng = StdRng::seed_from_u64(5);
        for _ in 0..2 {
            let loadout = mode.select_equipment(&mut rng).unwrap();
            assert_eq!(loadout.len(), 2);
            assert!(mode.weapons.iter().any(|candidate| candidate == loadout[0]));
            assert!(mode.weapons.iter().any(|candidate| candidate == loadout[1]));
        }
        assert!(mode.select_kill_weapon_excluding(&mut rng, &[]).is_err());

        mode.random_loadout = false;
        mode.kill_loadout = true;
        mode.weapons = vec!["glock".into(), "famas".into()];
        assert_eq!(mode.select_equipment(&mut rng).unwrap(), ["glock"]);
        let kill_weapon = mode.select_kill_weapon_excluding(&mut rng, &[]).unwrap();
        assert!(mode.weapons.iter().any(|weapon| weapon == kill_weapon));
    }
}
