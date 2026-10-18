// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::Probe,
    fault::{Fault, Mechanism},
    s2sdk,
};
use funrounds_core::{Capabilities, Mode};
use plugify::Str;

const BUNNYHOP: &[(&str, f32)] = &[
    ("sv_enablebunnyhopping", 1.0),
    ("sv_autobunnyhopping", 1.0),
    ("sv_accelerate_use_weapon_speed", 0.0),
    ("sv_staminamax", 0.0),
    ("sv_staminajumpcost", 0.0),
    ("sv_staminalandcost", 0.0),
];
const ACCURATE: &[(&str, f32)] = &[("weapon_accuracy_nospread", 1.0)];

pub fn settings(mode: &Mode) -> Vec<(&'static str, f32)> {
    let mut settings = vec![("mp_buytime", 0.0)];
    if mode.bomb.as_ref().is_some_and(|bomb| bomb.kind == funrounds_core::BombKind::Carry) {
        settings.push(("mp_anyone_can_pickup_c4", 1.0));
    }
    if mode.unlimited_clip {
        settings.push(("sv_infinite_ammo", 1.0));
    } else if mode.unlimited_reserve {
        settings.push(("sv_infinite_ammo", 2.0));
    }
    if mode.miss_damage {
        settings.push(("mp_weapon_self_inflict_amount", 1.0));
    }
    if mode.team_deathmatch {
        settings.extend_from_slice(&[("mp_give_player_c4", 0.0), ("mp_ignore_round_win_conditions", 1.0)]);
    }
    if let Some(charge) = mode.taser_charge {
        settings.push(("mp_taser_recharge_time", charge));
    }
    if mode.bunnyhop {
        settings.extend_from_slice(BUNNYHOP);
    }
    if let Some(friction) = mode.friction {
        settings.push(("sv_friction", friction));
    }
    if mode.no_spread_recoil {
        settings.extend_from_slice(ACCURATE);
    }
    if let Some(decay) = mode.view_punch_decay {
        settings.push(("view_punch_decay", decay));
    }
    settings
}

fn parameter_fault(name: &str, message: String) -> Fault {
    let mechanism = match name {
        "sv_infinite_ammo" => Mechanism::InfiniteAmmo,
        "mp_taser_recharge_time" => Mechanism::TaserCharge,
        "sv_friction" => Mechanism::Friction,
        "weapon_accuracy_nospread" => Mechanism::Recoil,
        "mp_weapon_self_inflict_amount" => Mechanism::MissDamage,
        "mp_give_player_c4" | "mp_ignore_round_win_conditions" => Mechanism::TeamDeathmatch,
        "mp_anyone_can_pickup_c4" => Mechanism::CarryBomb,
        "view_punch_decay" => Mechanism::ViewPunchDecay,
        "sv_falldamage_scale" | "mp_teamname_1" | "mp_teamname_2" => Mechanism::Classic,
        name if BUNNYHOP.iter().any(|(setting, _)| *setting == name) => Mechanism::Bunnyhop,
        _ => return Fault::global(message),
    };
    Fault::mechanism(mechanism, message)
}

fn replicates_clients(name: &str) -> bool {
    matches!(name, "weapon_accuracy_nospread" | "sv_falldamage_scale" | "view_punch_decay")
}

pub fn classic_probe() -> Probe {
    convar_probe(["sv_falldamage_scale", "mp_teamname_1", "mp_teamname_2"])
}

fn convar_probe(names: impl IntoIterator<Item = &'static str>) -> Probe {
    let mut probe = Probe::default();
    for name in names {
        probe.require(has_convar(name), format!("convar:{name}"));
    }
    probe
}

pub fn has_convar(name: &str) -> bool {
    valid_handle(s2sdk::FindConVar(&Str::from_str(name)))
}

fn valid_handle(handle: u64) -> bool {
    // Source SDK ConVarRef stores the access index in the low 16 bits; zero is valid.
    handle as u16 != u16::MAX
}

fn matches_value(actual: &str, expected: f32) -> bool {
    let value = match actual {
        "true" => Ok(1.0),
        "false" => Ok(0.0),
        _ => actual.parse::<f32>(),
    };
    value.is_ok_and(|value| value.is_finite() && (value - expected).abs() < 0.0001)
}

pub fn capabilities(weapon_fire: bool, round_prestart: bool, reasons: &mut Vec<String>) -> Capabilities {
    let mut recoil = convar_probe(ACCURATE.iter().map(|(name, _)| *name));
    recoil.require(weapon_fire, "event:weapon_fire");
    recoil.include(crate::effects::recoil_schema_probe());
    let mut view = crate::effects::recoil_shake_probe();
    view.require(weapon_fire, "event:weapon_fire");
    let mut blue = Probe::default();
    blue.schema("CBaseModelEntity", "m_clrRender");
    let mut miss = convar_probe(["mp_weapon_self_inflict_amount"]);
    miss.require(weapon_fire, "event:weapon_fire");
    let mut team = convar_probe(["mp_give_player_c4", "mp_ignore_round_win_conditions"]);
    team.require(round_prestart, "event:round_prestart");
    team.include(crate::game::team_score_probe());
    Capabilities {
        classic: false,
        equipment: true,
        health: true,
        armor: true,
        gravity: true,
        unlimited_reserve: convar_probe(["sv_infinite_ammo"]).record("unlimited_reserve", reasons),
        taser_charge: convar_probe(["mp_taser_recharge_time"]).record("taser_charge", reasons),
        bunnyhop: convar_probe(BUNNYHOP.iter().map(|(name, _)| *name)).record("bunnyhop", reasons),
        friction: convar_probe(["sv_friction"]).record("friction", reasons),
        no_spread_recoil: recoil.record("no_spread_recoil", reasons),
        recoil_view: view.record("recoil_view", reasons),
        blue: blue.record("blue", reasons),
        replenish_he: true,
        random_loadout: true,
        kill_loadout: true,
        lifesteal: false,
        unlimited_clip: convar_probe(["sv_infinite_ammo"]).record("unlimited_clip", reasons),
        headshots_only: false,
        miss_damage: miss.record("miss_damage", reasons),
        team_deathmatch: team.record("team_deathmatch", reasons),
        replenish_decoy: false,
        no_headshots: false,
        view_punch_decay: convar_probe(["view_punch_decay"]).record("view_punch_decay", reasons),
        ..Capabilities::default()
    }
}

#[derive(Default)]
pub struct Parameters(Vec<(&'static str, String)>);

impl Parameters {
    pub fn is_restored(&self) -> bool {
        self.0.is_empty()
    }
    pub fn apply(&mut self, mode: &Mode) -> Result<(), Fault> {
        let mut settings = settings(mode);
        settings.push(("sv_falldamage_scale", 0.0));
        for (name, value) in settings {
            let actual = self.write(name, &value.to_string()).map_err(|error| parameter_fault(name, error))?;
            if !matches_value(&actual, value) {
                return Err(parameter_fault(name, format!("{name} write verification failed")));
            }
        }
        Ok(())
    }

    pub fn set_string(&mut self, name: &'static str, value: &str) -> Result<(), Fault> {
        if self.write(name, value).map_err(|error| parameter_fault(name, error))? != value {
            return Err(parameter_fault(name, format!("{name} string write verification failed")));
        }
        Ok(())
    }

    fn write(&mut self, name: &'static str, value: &str) -> Result<String, String> {
        let handle = s2sdk::FindConVar(&Str::from_str(name));
        if !valid_handle(handle) {
            return Err(format!("{name} unavailable"));
        }
        // Capture once, before writing; repeated team-name updates retain the original value.
        if !self.0.iter().any(|(owned, _)| *owned == name) {
            self.0.push((name, s2sdk::GetConVarValue(handle).to_string()));
        }
        s2sdk::SetConVarValue(handle, &Str::from_str(value), replicates_clients(name), false);
        Ok(s2sdk::GetConVarValue(handle).to_string())
    }

    pub fn restore(&mut self) -> Result<(), String> {
        self.restore_with(None, restore_parameter)
    }

    pub fn restore_c4_assignment(&mut self) -> Result<(), String> {
        self.restore_with(Some("mp_give_player_c4"), restore_parameter)
    }

    fn restore_with(&mut self, selected: Option<&str>, mut restore: impl FnMut(&str, &str) -> bool) -> Result<(), String> {
        let mut failed = Vec::new();
        for index in (0..self.0.len()).rev() {
            let (name, previous) = &self.0[index];
            if selected.is_some_and(|selected| selected != *name) {
                continue;
            }
            if restore(name, previous) {
                self.0.remove(index);
                continue;
            }
            failed.push(*name);
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(format!("ConVar restoration failed: {}", failed.join(", ")))
        }
    }
}

fn restore_parameter(name: &str, previous: &str) -> bool {
    let handle = s2sdk::FindConVar(&Str::from_str(name));
    if !valid_handle(handle) {
        return false;
    }
    s2sdk::SetConVarValue(handle, &Str::from_str(previous), replicates_clients(name), false);
    if s2sdk::GetConVarValue(handle).to_string() != previous {
        return false;
    }
    s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] restored {name}={previous}")));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_c4_restoration_uses_the_snapshot_and_leaves_other_parameters_owned() {
        for previous in ["true", "false"] {
            let mut parameters = Parameters(vec![
                ("mp_give_player_c4", previous.into()),
                ("mp_ignore_round_win_conditions", "false".into()),
            ]);
            let mut writes = Vec::new();
            parameters
                .restore_with(Some("mp_give_player_c4"), |name, value| {
                    writes.push((name.to_owned(), value.to_owned()));
                    true
                })
                .unwrap();
            assert_eq!(writes, [("mp_give_player_c4".into(), previous.into())]);
            assert_eq!(parameters.0, [("mp_ignore_round_win_conditions", "false".into())]);
            parameters
                .restore_with(Some("mp_give_player_c4"), |_, _| panic!("unowned ConVar written"))
                .unwrap();
            parameters
                .restore_with(None, |name, value| name == "mp_ignore_round_win_conditions" && value == "false")
                .unwrap();
            assert!(parameters.is_restored());
        }
    }

    #[test]
    fn failed_early_c4_restoration_retains_the_snapshot_for_full_cleanup() {
        let mut parameters = Parameters(vec![("mp_give_player_c4", "true".into())]);
        assert_eq!(
            parameters.restore_with(Some("mp_give_player_c4"), |_, _| false),
            Err("ConVar restoration failed: mp_give_player_c4".into()),
        );
        assert_eq!(parameters.0, [("mp_give_player_c4", "true".into())]);
        parameters
            .restore_with(None, |name, value| name == "mp_give_player_c4" && value == "true")
            .unwrap();
        assert!(parameters.is_restored());
    }

    #[test]
    fn restoration_panic_keeps_every_convar_snapshot_owned() {
        let mut parameters = Parameters(vec![("mp_buytime", "15".into()), ("sv_infinite_ammo", "0".into())]);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parameters.restore())).is_err());
        assert_eq!(parameters.0, [("mp_buytime", "15".into()), ("sv_infinite_ammo", "0".into())]);
    }

    #[test]
    fn parameter_failures_are_classified_by_the_actual_write_not_the_current_mode() {
        use crate::fault::Scope;
        assert_eq!(
            parameter_fault("sv_infinite_ammo", "readback".into()).scope,
            Scope::Mechanism(Mechanism::InfiniteAmmo)
        );
        assert_eq!(
            parameter_fault("mp_anyone_can_pickup_c4", "readback".into()).scope,
            Scope::Mechanism(Mechanism::CarryBomb)
        );
        for (name, _) in BUNNYHOP {
            assert_eq!(parameter_fault(name, "readback".into()).scope, Scope::Mechanism(Mechanism::Bunnyhop));
        }
        assert_eq!(parameter_fault("mp_buytime", "readback".into()).scope, Scope::Global);
        assert_eq!(parameter_fault("unknown", "readback".into()).scope, Scope::Global);
    }

    #[test]
    fn camera_decay_is_opt_in_and_replicated() {
        assert!(replicates_clients("view_punch_decay"));
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        assert!(!settings(&mode).iter().any(|(name, _)| *name == "view_punch_decay"));
        mode.view_punch_decay = Some(2.0);
        assert!(settings(&mode).contains(&("view_punch_decay", 2.0)));
    }

    #[test]
    fn team_names_are_set_without_client_replication() {
        assert!(!replicates_clients("mp_teamname_1"));
        assert!(!replicates_clients("mp_teamname_2"));
        assert!(replicates_clients("view_punch_decay"));
    }

    #[test]
    fn carry_bomb_mode_allows_c4_pickup() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        assert!(!settings(&mode).contains(&("mp_anyone_can_pickup_c4", 1.0)));
        mode.bomb = Some(funrounds_core::BombSettings {
            kind: funrounds_core::BombKind::Carry,
            ..Default::default()
        });
        assert!(settings(&mode).contains(&("mp_anyone_can_pickup_c4", 1.0)));
    }

    #[test]
    fn captured_convars_remain_pending_until_their_snapshot_is_released() {
        assert!(Parameters::default().is_restored());
        assert!(!Parameters(vec![("mp_teamname_1", "original name".into())]).is_restored());
    }

    #[test]
    fn convar_reference_uses_the_sdk_invalid_access_index() {
        assert!(valid_handle(0));
        assert!(valid_handle(42));
        assert!(!valid_handle(u64::from(u32::MAX)));
        assert!(!valid_handle(u64::MAX));
        assert!(!valid_handle(0x1234ffff));
    }

    #[test]
    fn cvar_readback_accepts_sdk_boolean_text_and_numeric_values() {
        for (actual, value) in [("true", 1.0), ("false", 0.0), ("1", 1.0), ("0.5", 0.5), ("100.000000", 100.0)] {
            assert!(matches_value(actual, value));
        }
        for actual in ["true", "nan", "inf", "bad", "0.1"] {
            assert!(!matches_value(actual, 0.0));
        }
    }

    #[test]
    fn generic_mode_only_disables_buytime_by_default() {
        let config = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml")).unwrap();
        assert_eq!(settings(&config.modes[0]), [("mp_buytime", 0.0)]);
    }

    #[test]
    fn reserve_keeps_reload_and_no_spread_is_replicated_without_missing_recoil_cvars() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        mode.unlimited_reserve = true;
        mode.no_spread_recoil = true;
        let settings = settings(&mode);
        assert!(settings.contains(&("sv_infinite_ammo", 2.0)));
        assert!(settings.contains(&("weapon_accuracy_nospread", 1.0)));
        assert!(!settings.iter().any(|(name, _)| *name == "weapon_accuracy_shotgun_spread_patterns"));
        assert!(
            !settings
                .iter()
                .any(|(name, _)| matches!(*name, "weapon_recoil_scale" | "weapon_recoil_view_punch_extra"))
        );
        assert!(replicates_clients("weapon_accuracy_nospread"));
        assert!(!replicates_clients("weapon_accuracy_shotgun_spread_patterns"));
        assert!(!replicates_clients("sv_infinite_ammo"));
    }

    #[test]
    fn recoil_view_is_a_message_effect_not_a_missing_cvar() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        mode.recoil_view = Some(2.0);
        assert!(!settings(&mode).iter().any(|(name, _)| name.contains("recoil")));
    }

    #[test]
    fn ammunition_and_round_flags_map_to_their_convars() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        mode.unlimited_clip = true;
        let clip = settings(&mode);
        assert!(clip.contains(&("sv_infinite_ammo", 1.0)));
        assert!(!clip.contains(&("sv_infinite_ammo", 2.0)));

        mode.unlimited_clip = false;
        mode.unlimited_reserve = true;
        mode.miss_damage = true;
        mode.team_deathmatch = true;
        let effects = settings(&mode);
        assert!(effects.contains(&("sv_infinite_ammo", 2.0)));
        assert!(effects.contains(&("mp_weapon_self_inflict_amount", 1.0)));
        assert!(effects.contains(&("mp_give_player_c4", 0.0)));
        assert!(effects.contains(&("mp_ignore_round_win_conditions", 1.0)));
    }
}
