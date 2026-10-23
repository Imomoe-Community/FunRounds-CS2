// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::Mode;
use rand::seq::IndexedRandom;

#[derive(Clone, Copy)]
pub(crate) enum WeaponSlot {
    Primary = 0,
    Secondary = 1,
    Knife = 2,
    Extra = 3,
}

pub(crate) fn weapon_slot(weapon: &str) -> Option<WeaponSlot> {
    match weapon {
        "weapon_ak47"
        | "weapon_m4a1"
        | "weapon_m4a1_silencer"
        | "weapon_galilar"
        | "weapon_famas"
        | "weapon_aug"
        | "weapon_sg556"
        | "weapon_mag7"
        | "weapon_awp"
        | "weapon_ssg08"
        | "weapon_g3sg1"
        | "weapon_scar20"
        | "weapon_mac10"
        | "weapon_mp9"
        | "weapon_mp7"
        | "weapon_mp5sd"
        | "weapon_ump45"
        | "weapon_p90"
        | "weapon_bizon"
        | "weapon_nova"
        | "weapon_xm1014"
        | "weapon_sawedoff"
        | "weapon_m249"
        | "weapon_negev" => Some(WeaponSlot::Primary),
        "weapon_glock"
        | "weapon_hkp2000"
        | "weapon_usp_silencer"
        | "weapon_p250"
        | "weapon_fiveseven"
        | "weapon_tec9"
        | "weapon_cz75a"
        | "weapon_deagle"
        | "weapon_elite"
        | "weapon_revolver" => Some(WeaponSlot::Secondary),
        "weapon_knife" => Some(WeaponSlot::Knife),
        "weapon_taser"
        | "weapon_hegrenade"
        | "weapon_flashbang"
        | "weapon_smokegrenade"
        | "weapon_molotov"
        | "weapon_incgrenade"
        | "weapon_decoy" => Some(WeaponSlot::Extra),
        _ => None,
    }
}

impl Mode {
    // Each listed slot supplies one item; an omitted slot stays empty.
    pub fn select_equipment(&self, rng: &mut impl rand::Rng) -> Result<Vec<&str>, String> {
        if self.kill_loadout {
            if !self.weapons.iter().any(|weapon| weapon == "weapon_glock") {
                return Err(format!("{}: kill loadout requires an initial Glock", self.id));
            }
            return Ok(vec!["weapon_glock"]);
        }
        if self.random_loadout {
            let mut candidates = Vec::new();
            for weapon in self.weapons.iter().map(String::as_str).filter(|weapon| *weapon != "weapon_knife") {
                if !candidates.contains(&weapon) {
                    candidates.push(weapon);
                }
            }
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
            if weapon == "weapon_knife" {
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

    pub fn select_kill_weapon(&self, rng: &mut impl rand::Rng) -> Result<&str, String> {
        if !self.kill_loadout {
            return Err(format!("{}: kill loadout is disabled", self.id));
        }
        let mut candidates = Vec::new();
        for weapon in self.weapons.iter().map(String::as_str).filter(|weapon| *weapon != "weapon_knife") {
            if !candidates.contains(&weapon) {
                candidates.push(weapon);
            }
        }
        candidates
            .choose(rng)
            .copied()
            .ok_or_else(|| format!("{}: kill loadout candidates must not be empty", self.id))
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
        assert_eq!(config.modes[0].select_equipment(&mut rng).unwrap(), ["weapon_famas", "weapon_p250"]);
    }

    #[test]
    fn rejects_invalid_equipment_and_deduplicates_candidates() {
        let mut mode = Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        let mut rng = StdRng::seed_from_u64(0);
        mode.weapons = vec!["weapon_famas".into(), "weapon_famas".into()];
        assert_eq!(mode.select_equipment(&mut rng).unwrap(), ["weapon_famas"]);
        mode.weapons = vec!["weapon_knife".into()];
        assert!(mode.select_equipment(&mut rng).is_err());
        mode.weapons = vec!["weapon_shield".into()];
        assert!(mode.select_equipment(&mut rng).is_err());
        mode.weapons.clear();
        assert!(mode.select_equipment(&mut rng).unwrap().is_empty());
    }

    #[test]
    fn recognizes_all_ordinary_guns_and_extra_equipment() {
        let primaries = [
            "weapon_ak47",
            "weapon_m4a1",
            "weapon_m4a1_silencer",
            "weapon_galilar",
            "weapon_famas",
            "weapon_aug",
            "weapon_sg556",
            "weapon_mag7",
            "weapon_awp",
            "weapon_ssg08",
            "weapon_g3sg1",
            "weapon_scar20",
            "weapon_mac10",
            "weapon_mp9",
            "weapon_mp7",
            "weapon_mp5sd",
            "weapon_ump45",
            "weapon_p90",
            "weapon_bizon",
            "weapon_nova",
            "weapon_xm1014",
            "weapon_sawedoff",
            "weapon_m249",
            "weapon_negev",
        ];
        let secondaries = [
            "weapon_glock",
            "weapon_hkp2000",
            "weapon_usp_silencer",
            "weapon_p250",
            "weapon_fiveseven",
            "weapon_tec9",
            "weapon_cz75a",
            "weapon_deagle",
            "weapon_elite",
            "weapon_revolver",
        ];
        let extras = [
            "weapon_taser",
            "weapon_hegrenade",
            "weapon_flashbang",
            "weapon_smokegrenade",
            "weapon_molotov",
            "weapon_incgrenade",
            "weapon_decoy",
        ];
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
        assert!(matches!(weapon_slot("weapon_knife"), Some(super::WeaponSlot::Knife)));
        assert!(extras.iter().all(|weapon| matches!(weapon_slot(weapon), Some(super::WeaponSlot::Extra))));
        assert_eq!(primaries.len(), 24);
        assert_eq!(secondaries.len(), 10);
        assert_eq!(extras.len(), 7);
        assert!(weapon_slot("weapon_c4").is_none());
    }

    #[test]
    fn equipment_selects_at_most_one_candidate_for_each_available_slot() {
        let mut mode = Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        mode.weapons = vec!["weapon_famas".into(), "weapon_p250".into(), "weapon_hegrenade".into()];
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
        mode.weapons = vec!["weapon_famas".into(), "weapon_p250".into()];
        let mut rng = StdRng::seed_from_u64(5);
        for _ in 0..2 {
            let loadout = mode.select_equipment(&mut rng).unwrap();
            assert_eq!(loadout.len(), 2);
            assert!(mode.weapons.iter().any(|candidate| candidate == loadout[0]));
            assert!(mode.weapons.iter().any(|candidate| candidate == loadout[1]));
        }
        assert!(mode.select_kill_weapon(&mut rng).is_err());

        mode.random_loadout = false;
        mode.kill_loadout = true;
        mode.weapons = vec!["weapon_glock".into(), "weapon_famas".into()];
        assert_eq!(mode.select_equipment(&mut rng).unwrap(), ["weapon_glock"]);
        let kill_weapon = mode.select_kill_weapon(&mut rng).unwrap();
        assert!(mode.weapons.iter().any(|weapon| weapon == kill_weapon));
    }
}
