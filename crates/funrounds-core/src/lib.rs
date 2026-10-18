// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;
use std::collections::HashSet;

mod rounds;
pub use rounds::{Capabilities, NextMode, Phase, RoundController, RoundGeneration, RoundKey};
mod players;
pub use players::{PlayerIdentity, PlayerTask, PlayerTracker};
mod rules;
pub use rules::{
    DODGEBALL_DAMAGE_MULTIPLIER, LIFESTEAL_MAX_HEALTH, THROWING_KNIFE_BODY_DAMAGE, THROWING_KNIFE_ELASTICITY, THROWING_KNIFE_GRAVITY,
    THROWING_KNIFE_HEAD_DAMAGE, THROWING_KNIFE_HEADSHOT_DISTANCE, THROWING_KNIFE_LIFETIME, THROWING_KNIFE_SPEED, TeamDeathmatch, TeamResult,
    dodgeball_damage, lifesteal_recovery, low_health_finish,
};
mod classic;
mod equipment;
pub use classic::{classic_damage_scale, decay_health, health_scaling, self_push_force};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BombKind {
    Native,
    Chicken,
    Carry,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BombSettings {
    pub kind: BombKind,
    #[serde(default = "default_plant_time")]
    pub plant_time: f32,
    #[serde(default = "default_explode_time")]
    pub explode_time: f32,
    #[serde(default = "default_defuse_kit_time")]
    pub defuse_kit_time: f32,
    #[serde(default = "default_defuse_time")]
    pub defuse_time: f32,
    #[serde(default)]
    pub bounce: bool,
}

impl Default for BombSettings {
    fn default() -> Self {
        Self {
            kind: BombKind::Native,
            plant_time: default_plant_time(),
            explode_time: default_explode_time(),
            defuse_kit_time: default_defuse_kit_time(),
            defuse_time: default_defuse_time(),
            bounce: false,
        }
    }
}

fn default_plant_time() -> f32 {
    3.0
}

fn default_explode_time() -> f32 {
    45.0
}

fn default_defuse_kit_time() -> f32 {
    5.0
}

fn default_defuse_time() -> f32 {
    10.0
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub diagnostics: bool,
    pub modes: Vec<Mode>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Mode {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub description: String,
    pub weapons: Vec<String>,
    pub health: u16,
    pub armor: u8,
    pub gravity: f32,
    #[serde(default)]
    pub unlimited_reserve: bool,
    #[serde(default)]
    pub taser_charge: Option<f32>,
    #[serde(default)]
    pub bunnyhop: bool,
    #[serde(default)]
    pub friction: Option<f32>,
    #[serde(default)]
    pub no_spread_recoil: bool,
    #[serde(default)]
    pub recoil_view: Option<f32>,
    #[serde(default)]
    pub blue: bool,
    #[serde(default)]
    pub replenish_he: bool,
    #[serde(default)]
    pub random_loadout: bool,
    #[serde(default)]
    pub kill_loadout: bool,
    #[serde(default)]
    pub lifesteal: bool,
    #[serde(default)]
    pub unlimited_clip: bool,
    #[serde(default)]
    pub headshots_only: bool,
    #[serde(default)]
    pub miss_damage: bool,
    #[serde(default)]
    pub team_deathmatch: bool,
    #[serde(default)]
    pub bomb: Option<BombSettings>,
    #[serde(default)]
    pub dodgeball: bool,
    #[serde(default)]
    pub throwing_knives: bool,
    #[serde(default)]
    pub no_headshots: bool,
    #[serde(default)]
    pub replenish_decoy: bool,
    #[serde(default)]
    pub view_punch_decay: Option<f32>,
}

impl Config {
    pub fn parse(source: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(source).map_err(|error| error.to_string())?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for mode in &self.modes {
            if mode.id.is_empty() || !mode.id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
                return Err("mode id must contain ASCII letters, digits, '_' or '-'".into());
            }
            if !ids.insert(&mode.id) {
                return Err(format!("duplicate mode id: {}", mode.id));
            }
            if mode.name.trim().is_empty() || mode.description.trim().is_empty() {
                return Err(format!("{}: name and description must not be empty", mode.id));
            }
            if mode.name.contains(['\r', '\n']) || mode.description.contains(['\r', '\n']) {
                return Err(format!("{}: name and description must each fit one line", mode.id));
            }
            if mode.health == 0 || mode.health > 10000 || !mode.gravity.is_finite() || mode.gravity <= 0.0 || mode.gravity > 10.0 {
                return Err(format!("{}: invalid health, armor or gravity", mode.id));
            }
            if mode.weapons.is_empty() || mode.weapons.iter().any(|weapon| equipment::weapon_slot(weapon).is_none()) {
                return Err(format!("{}: equipment must use the supported CS2 weapon whitelist", mode.id));
            }
            if mode.taser_charge.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: taser charge must be finite and non-negative", mode.id));
            }
            if mode.taser_charge.is_some() && !mode.weapons.iter().any(|weapon| weapon == "weapon_taser") {
                return Err(format!("{}: taser charge requires a taser equipment candidate", mode.id));
            }
            if mode.random_loadout && mode.kill_loadout {
                return Err(format!("{}: random and kill loadouts cannot be combined", mode.id));
            }
            if (mode.random_loadout || mode.kill_loadout) && mode.weapons.iter().any(|weapon| weapon == "weapon_knife") {
                return Err(format!("{}: random loadout candidates must not include a knife", mode.id));
            }
            if mode.kill_loadout && !mode.weapons.iter().any(|weapon| weapon == "weapon_glock") {
                return Err(format!("{}: kill loadout candidates must include the initial Glock", mode.id));
            }
            if mode.unlimited_clip && !mode.weapons.iter().any(|weapon| weapon == "weapon_taser") {
                return Err(format!("{}: unlimited clip requires a taser equipment candidate", mode.id));
            }
            if mode.friction.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: friction must be finite and non-negative", mode.id));
            }
            if mode.recoil_view.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: view recoil must be finite and non-negative", mode.id));
            }
            if mode.view_punch_decay.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: view-punch decay must be finite and non-negative", mode.id));
            }
            if mode.bomb.as_ref().is_some_and(|bomb| {
                [bomb.plant_time, bomb.explode_time, bomb.defuse_kit_time, bomb.defuse_time]
                    .iter()
                    .any(|value| !value.is_finite() || *value < 0.0)
            }) {
                return Err(format!("{}: bomb times must be finite and non-negative", mode.id));
            }
            if mode.headshots_only && mode.no_headshots {
                return Err(format!("{}: headshots_only and no_headshots cannot be combined", mode.id));
            }
            if mode.dodgeball && !mode.weapons.iter().any(|weapon| weapon == "weapon_decoy") {
                return Err(format!("{}: dodgeball requires decoy equipment", mode.id));
            }
            if mode.throwing_knives && !mode.weapons.iter().any(|weapon| weapon == "weapon_knife") {
                return Err(format!("{}: throwing knives require knife equipment", mode.id));
            }
            if mode.replenish_he
                && (!mode.weapons.iter().any(|weapon| weapon == "weapon_hegrenade")
                    || mode
                        .weapons
                        .iter()
                        .any(|weapon| matches!(equipment::weapon_slot(weapon), Some(equipment::WeaponSlot::Extra)) && weapon != "weapon_hegrenade"))
            {
                return Err(format!("{}: HE replenishment requires HE as the only extra equipment candidate", mode.id));
            }
            if mode.replenish_decoy
                && (!mode.weapons.iter().any(|weapon| weapon == "weapon_decoy")
                    || mode
                        .weapons
                        .iter()
                        .any(|weapon| matches!(equipment::weapon_slot(weapon), Some(equipment::WeaponSlot::Extra)) && weapon != "weapon_decoy"))
            {
                return Err(format!(
                    "{}: decoy replenishment requires decoy as the only extra equipment candidate",
                    mode.id
                ));
            }
        }
        if !self.modes.iter().any(|mode| mode.enabled) {
            return Err("enabled mode pool is empty".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../config/funrounds.example.toml");

    #[test]
    fn public_example_contains_one_valid_generic_mode() {
        let config = Config::parse(include_str!("../../../config/funrounds.example.toml")).unwrap();
        assert_eq!(config.modes.len(), 1);
        let mode = &config.modes[0];
        assert_eq!(mode.id, "example");
        assert!(mode.enabled);
        assert_eq!(mode.weapons, ["weapon_famas", "weapon_p250"]);
        assert_eq!((mode.health, mode.armor, mode.gravity), (120, 50, 1.0));
    }

    #[test]
    fn unknown_keys_are_rejected() {
        for value in ["false", "true"] {
            let source = format!("unknown = {value}\n{EXAMPLE}");
            let error = Config::parse(&source).unwrap_err();
            assert!(error.contains("unknown field") && error.contains("unknown"), "{error}");
        }
    }

    #[test]
    fn diagnostics_defaults_off_and_requires_an_explicit_boolean() {
        let source = EXAMPLE.replace("diagnostics = false", "");
        assert!(!Config::parse(&source).unwrap().diagnostics);
        assert!(Config::parse(&format!("diagnostics = true\n{source}")).unwrap().diagnostics);
        assert!(!Config::parse(&format!("diagnostics = false\n{source}")).unwrap().diagnostics);
        assert!(Config::parse(&format!("diagnostics = 1\n{source}")).is_err());
    }

    #[test]
    fn rejects_invalid_values_and_incompatible_options() {
        for invalid in [
            EXAMPLE.replace("enabled = true", "enabled = false"),
            EXAMPLE.replace("health = 120", "health = 0"),
            EXAMPLE.replace("gravity = 1.0", "gravity = nan"),
            EXAMPLE.replace("weapon_famas", "weapon_shield"),
            EXAMPLE.replace("unlimited_clip = false", "unlimited_clip = true"),
            EXAMPLE.replace("# view_punch_decay = 5.0", "view_punch_decay = -1.0"),
            format!("{EXAMPLE}\n[modes.bomb]\nkind = \"native\"\nplant_time = -1.0\n"),
        ] {
            assert!(Config::parse(&invalid).is_err(), "unexpected acceptance: {invalid}");
        }
    }

    #[test]
    fn rejects_conflicting_headshot_options() {
        let invalid = EXAMPLE
            .replace("headshots_only = false", "headshots_only = true")
            .replace("no_headshots = false", "no_headshots = true");
        assert_eq!(
            Config::parse(&invalid).unwrap_err(),
            "example: headshots_only and no_headshots cannot be combined"
        );
    }

    #[test]
    fn bomb_settings_have_documented_defaults() {
        assert_eq!(
            BombSettings::default(),
            BombSettings {
                kind: BombKind::Native,
                plant_time: 3.0,
                explode_time: 45.0,
                defuse_kit_time: 5.0,
                defuse_time: 10.0,
                bounce: false,
            }
        );
    }
}
