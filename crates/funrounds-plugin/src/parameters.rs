// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::Probe,
    fault::{Fault, Mechanism},
    s2sdk,
};
use funrounds_core::{Ammo, Capabilities, Mode, SelfDamage, TeamFilter, WinCondition};
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

pub fn settings(mode: &Mode) -> Vec<(&'static str, String)> {
    let mut settings = Vec::new();
    if let Some(buy_time) = mode.buy_time {
        settings.push(("mp_buytime", buy_time.to_string()));
    }
    if let Some(buy_limit) = mode.buy_limit {
        settings.push(("mp_maxmoney", buy_limit.to_string()));
    }
    if let Some(scale) = mode.friendly_fire_scale {
        let enabled = if scale > 0.0 { "1" } else { "0" };
        settings.extend([
            ("mp_friendlyfire", enabled.to_owned()),
            ("ff_damage_reduction_bullets", scale.to_string()),
            ("ff_damage_reduction_grenade", scale.to_string()),
            ("ff_damage_reduction_other", scale.to_string()),
        ]);
    }
    if let Some(enemies) = mode.teammates_as_enemies {
        settings.push(("mp_teammates_are_enemies", if enemies { "1" } else { "0" }.to_owned()));
    }
    if let Some(spawn) = mode.random_spawn {
        settings.push((
            "mp_randomspawn",
            match spawn {
                TeamFilter::None => "0",
                TeamFilter::All => "1",
                TeamFilter::Terrorists => "2",
                TeamFilter::CounterTerrorists => "3",
            }
            .to_owned(),
        ));
    }
    if let Some(drop) = mode.drop_weapons {
        settings.push(("mp_death_drop_gun", if drop { "3" } else { "0" }.to_owned()));
    }
    if let Some(drop) = mode.drop_grenades {
        settings.push(("mp_death_drop_grenade", if drop { "2" } else { "0" }.to_owned()));
    }
    if let Some(scale) = mode.fall_damage_scale {
        settings.push(("sv_falldamage_scale", scale.to_string()));
    }
    if mode.bomb.as_ref().is_some_and(|bomb| bomb.kind == funrounds_core::BombKind::Carry) {
        settings.push(("mp_anyone_can_pickup_c4", "1".into()));
    }
    match mode.ammo {
        Ammo::Inherit => {}
        Ammo::Clip => settings.push(("sv_infinite_ammo", "1".into())),
        Ammo::Reserve => settings.push(("sv_infinite_ammo", "2".into())),
        Ammo::SingleShot => settings.push(("sv_infinite_ammo", "0".into())),
    }
    match mode.self_damage {
        Some(SelfDamage::MissPenalty) => settings.push(("mp_weapon_self_inflict_amount", "1".into())),
        Some(SelfDamage::Native { amount }) => settings.push(("mp_weapon_self_inflict_amount", amount.to_string())),
        None => {}
    }
    if mode.c4_count.is_some() {
        settings.push(("mp_give_player_c4", "0".into()));
    }
    if mode.win_condition == WinCondition::TeamDeathmatch {
        settings.push(("mp_ignore_round_win_conditions", "1".into()));
    }
    if let Some(charge) = mode.taser_charge {
        settings.push(("mp_taser_recharge_time", charge.to_string()));
    }
    if let Some(enabled) = mode.bunnyhop {
        if enabled {
            settings.extend(BUNNYHOP.iter().map(|(name, value)| (*name, value.to_string())));
        } else {
            settings.extend([("sv_enablebunnyhopping", "0".into()), ("sv_autobunnyhopping", "0".into())]);
        }
    }
    if let Some(friction) = mode.friction {
        settings.push(("sv_friction", friction.to_string()));
    }
    if let Some(enabled) = mode.no_spread {
        settings.extend(
            ACCURATE
                .iter()
                .map(|(name, value)| (*name, if enabled { value.to_string() } else { "0".into() })),
        );
    }
    if let Some(decay) = mode.view_punch_decay {
        settings.push(("view_punch_decay", decay.to_string()));
    }
    settings
}

fn parameter_fault(name: &str, message: String) -> Fault {
    let mechanism = match name {
        "sv_infinite_ammo" => Mechanism::Ammo,
        "mp_taser_recharge_time" => Mechanism::TaserCharge,
        "sv_friction" => Mechanism::Friction,
        "weapon_accuracy_nospread" => Mechanism::NoSpread,
        "mp_weapon_self_inflict_amount" => Mechanism::SelfDamage,
        "mp_give_player_c4" => Mechanism::C4Assignment,
        "mp_ignore_round_win_conditions" => Mechanism::WinCondition,
        "mp_anyone_can_pickup_c4" => Mechanism::CarryBomb,
        "view_punch_decay" => Mechanism::ViewPunchDecay,
        "mp_buytime" => Mechanism::BuyTime,
        "mp_maxmoney" => Mechanism::BuyLimit,
        "mp_friendlyfire" | "ff_damage_reduction_bullets" | "ff_damage_reduction_grenade" | "ff_damage_reduction_other" => {
            Mechanism::FriendlyFireScale
        }
        "mp_teammates_are_enemies" => Mechanism::TeammatesAsEnemies,
        "mp_randomspawn" => Mechanism::RandomSpawn,
        "mp_death_drop_gun" => Mechanism::DropWeapons,
        "mp_death_drop_grenade" => Mechanism::DropGrenades,
        "sv_falldamage_scale" => Mechanism::FallDamageScale,
        name if BUNNYHOP.iter().any(|(setting, _)| *setting == name) => Mechanism::Bunnyhop,
        _ => return Fault::global(message),
    };
    Fault::mechanism(mechanism, message)
}

fn replicates_clients(name: &str) -> bool {
    matches!(name, "weapon_accuracy_nospread" | "sv_falldamage_scale" | "view_punch_decay")
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

fn parsed_value(value: &str) -> Option<f64> {
    let value = match value {
        "true" => 1.0,
        "false" => 0.0,
        _ => value.parse::<f64>().ok()?,
    };
    value.is_finite().then_some(value)
}

fn same_value(actual: &str, expected: &str) -> bool {
    match (parsed_value(actual), parsed_value(expected)) {
        (Some(actual), Some(expected)) => (actual - expected).abs() < 0.0001,
        _ => actual == expected,
    }
}

fn matches_value(actual: &str, expected: f32) -> bool {
    parsed_value(actual).is_some_and(|value| (value - f64::from(expected)).abs() < 0.0001)
}

pub fn capabilities(weapon_fire: bool, round_prestart: bool, reasons: &mut Vec<String>) -> Capabilities {
    let mut live_player = Probe::default();
    crate::capability::imports!(live_player;
        __s2sdk_GetMaxClients, __s2sdk_IsClientInGame, __s2sdk_IsClientAlive, __s2sdk_GetClientPawn,
        __s2sdk_PlayerSlotToEntHandle, __s2sdk_EntPointerToEntHandle, __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer,
    );
    let mut equipment = live_player.clone();
    crate::capability::imports!(equipment;
        __s2sdk_GetClientTeam, __s2sdk_GetClientWeapons, __s2sdk_GetClientActiveWeapon, __s2sdk_GetEntityClassname,
        __s2sdk_GiveNamedItem, __s2sdk_RemoveWeapon, __s2sdk_RemoveEntity, __s2sdk_SelectWeapon, __s2sdk_GetEntSchema2,
    );
    let clear_weapons = equipment.clone();
    let give_weapons = equipment.clone();
    let mut helmet = Probe::default();
    helmet.include(live_player);
    crate::capability::imports!(helmet;
        __s2sdk_GetSchemaOffset, __s2sdk_EntHandleToEntPointer, __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2,
    );
    helmet.schema("CBasePlayerPawn", "m_pItemServices");
    helmet.schema("CCSPlayer_ItemServices", "m_bHasHelmet");
    let mut periodic_speed = Probe::default();
    crate::capability::imports!(periodic_speed; __s2sdk_GetClientSpeed, __s2sdk_SetClientSpeed);
    let speed = periodic_speed.clone();
    let mut periodic_gravity = Probe::default();
    crate::capability::imports!(periodic_gravity; __s2sdk_GetClientGravity, __s2sdk_SetClientGravity);
    let no_spread = convar_probe(ACCURATE.iter().map(|(name, _)| *name));
    let mut no_recoil = crate::effects::recoil_schema_probe();
    no_recoil.require(weapon_fire, "event:weapon_fire");
    let mut view = crate::effects::recoil_shake_probe();
    view.require(weapon_fire, "event:weapon_fire");
    let mut color = Probe::default();
    color.schema("CBaseModelEntity", "m_clrRender");
    let mut self_damage = convar_probe(["mp_weapon_self_inflict_amount"]);
    self_damage.require(weapon_fire, "event:weapon_fire");
    let mut win_condition = convar_probe(["mp_ignore_round_win_conditions"]);
    win_condition.require(round_prestart, "event:round_prestart");
    win_condition.include(crate::game::team_score_probe());
    let mut c4_assignment = convar_probe(["mp_give_player_c4"]);
    c4_assignment.require(round_prestart, "event:round_prestart");
    c4_assignment.include(crate::effects::c4_removal_probe());
    Capabilities {
        clear_weapons: clear_weapons.record("clear_weapons", reasons),
        give_weapons: give_weapons.clone().record("give_weapons", reasons),
        give_knife: give_weapons.record("give_knife", reasons),
        equipment: equipment.record("equipment", reasons),
        health: true,
        armor: true,
        gravity: true,
        speed: speed.record("speed", reasons),
        helmet: helmet.record("helmet", reasons),
        periodic_speed: periodic_speed.record("periodic_speed", reasons),
        periodic_gravity: periodic_gravity.record("periodic_gravity", reasons),
        ammo: convar_probe(["sv_infinite_ammo"]).record("ammo", reasons),
        taser_charge: convar_probe(["mp_taser_recharge_time"]).record("taser_charge", reasons),
        bunnyhop: convar_probe(BUNNYHOP.iter().map(|(name, _)| *name)).record("bunnyhop", reasons),
        friction: convar_probe(["sv_friction"]).record("friction", reasons),
        no_spread: no_spread.record("no_spread", reasons),
        no_recoil: no_recoil.record("no_recoil", reasons),
        recoil_view: view.record("recoil_view", reasons),
        color: color.record("color", reasons),
        single_shot: {
            let mut probe = crate::effects::single_shot_probe();
            probe.include(convar_probe(["sv_infinite_ammo"]));
            probe.record("single_shot", reasons)
        },
        fast_switch: crate::effects::fast_switch_probe().record("fast_switch", reasons),
        random_loadout: true,
        kill_loadout: true,
        lifesteal: false,
        headshots: false,
        self_damage: self_damage.record("self_damage", reasons),
        win_condition: win_condition.record("win_condition", reasons),
        c4_assignment: c4_assignment.record("c4_assignment", reasons),
        view_punch_decay: convar_probe(["view_punch_decay"]).record("view_punch_decay", reasons),
        no_scope: crate::effects::no_scope_probe().record("no_scope", reasons),
        fov: crate::effects::fov_probe().record("fov", reasons),
        hide_crosshair: crate::effects::crosshair_probe().record("hide_crosshair", reasons),
        buy_time: convar_probe(["mp_buytime"]).record("buy_time", reasons),
        buy_limit: convar_probe(["mp_maxmoney"]).record("buy_limit", reasons),
        friendly_fire_scale: convar_probe([
            "mp_friendlyfire",
            "ff_damage_reduction_bullets",
            "ff_damage_reduction_grenade",
            "ff_damage_reduction_other",
        ])
        .record("friendly_fire_scale", reasons),
        teammates_as_enemies: convar_probe(["mp_teammates_are_enemies"]).record("teammates_as_enemies", reasons),
        random_spawn: convar_probe(["mp_randomspawn"]).record("random_spawn", reasons),
        drop_weapons: convar_probe(["mp_death_drop_gun"]).record("drop_weapons", reasons),
        drop_grenades: convar_probe(["mp_death_drop_grenade"]).record("drop_grenades", reasons),
        fall_damage_scale: convar_probe(["sv_falldamage_scale"]).record("fall_damage_scale", reasons),
        ..Capabilities::default()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct OwnedParameter {
    name: &'static str,
    previous: String,
    written: String,
}

#[derive(Default)]
pub struct Parameters(Vec<OwnedParameter>);

impl Parameters {
    pub fn is_restored(&self) -> bool {
        self.0.is_empty()
    }
    pub fn apply(&mut self, mode: &Mode) -> Result<(), Fault> {
        let settings = settings(mode);
        validate_settings(&settings)?;
        let retained = settings.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        self.restore_omitted(&retained).map_err(Fault::global)?;
        for (name, value) in settings {
            let actual = self.write(name, &value).map_err(|error| parameter_fault(name, error))?;
            if !same_value(&actual, &value) {
                return Err(parameter_fault(name, format!("{name} write verification failed")));
            }
        }
        Ok(())
    }

    pub fn set_number(&mut self, name: &'static str, value: f32) -> Result<(), Fault> {
        let actual = self.write(name, &value.to_string()).map_err(|error| parameter_fault(name, error))?;
        if matches_value(&actual, value) {
            Ok(())
        } else {
            Err(parameter_fault(name, format!("{name} numeric write verification failed")))
        }
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
        self.write_with(
            name,
            value,
            || s2sdk::GetConVarValue(handle).to_string(),
            |value| {
                s2sdk::SetConVarValue(handle, &Str::from_str(value), replicates_clients(name), false);
                s2sdk::GetConVarValue(handle).to_string()
            },
        )
    }

    pub fn restore(&mut self) -> Result<(), String> {
        self.restore_with(None, restore_parameter)
    }

    pub fn restore_c4_assignment(&mut self) -> Result<(), String> {
        self.restore_with(Some("mp_give_player_c4"), restore_parameter)
    }

    fn restore_omitted(&mut self, retained: &[&str]) -> Result<(), String> {
        self.restore_omitted_with(retained, restore_parameter)
    }

    fn restore_omitted_with(&mut self, retained: &[&str], restore: impl FnMut(&str, &str, &str) -> bool) -> Result<(), String> {
        self.restore_if(|name| !retained.contains(&name), restore)
    }

    fn write_with(
        &mut self,
        name: &'static str,
        value: &str,
        mut read: impl FnMut() -> String,
        mut write: impl FnMut(&str) -> String,
    ) -> Result<String, String> {
        let index = self.0.iter().position(|owned| owned.name == name);
        let previous = if let Some(index) = index {
            if !same_value(&read(), &self.0[index].written) {
                self.0.remove(index);
                return Err(format!("{name} ownership lost before write"));
            }
            self.0[index].previous.clone()
        } else {
            read()
        };
        let actual = write(value);
        if let Some(index) = index {
            self.0[index].written = actual.clone();
        } else {
            self.0.push(OwnedParameter {
                name,
                previous,
                written: actual.clone(),
            });
        }
        Ok(actual)
    }

    fn restore_with(&mut self, selected: Option<&str>, restore: impl FnMut(&str, &str, &str) -> bool) -> Result<(), String> {
        self.restore_if(|name| selected.is_none_or(|selected| selected == name), restore)
    }

    fn restore_if(&mut self, mut should_restore: impl FnMut(&str) -> bool, mut restore: impl FnMut(&str, &str, &str) -> bool) -> Result<(), String> {
        let mut failed = Vec::new();
        for index in (0..self.0.len()).rev() {
            let owned = &self.0[index];
            if !should_restore(owned.name) {
                continue;
            }
            if restore(owned.name, &owned.previous, &owned.written) {
                self.0.remove(index);
                continue;
            }
            failed.push(owned.name);
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(format!("ConVar restoration failed: {}", failed.join(", ")))
        }
    }
}

fn validate_settings(settings: &[(&str, String)]) -> Result<(), Fault> {
    for (index, (name, value)) in settings.iter().enumerate() {
        if settings[index + 1..]
            .iter()
            .any(|(other, other_value)| name == other && !same_value(value, other_value))
        {
            return Err(Fault::global(format!("conflicting writes for ConVar {name}")));
        }
    }
    Ok(())
}

fn restore_parameter(name: &str, previous: &str, written: &str) -> bool {
    let handle = s2sdk::FindConVar(&Str::from_str(name));
    if !valid_handle(handle) {
        return false;
    }
    if !same_value(&s2sdk::GetConVarValue(handle).to_string(), written) {
        return true;
    }
    s2sdk::SetConVarValue(handle, &Str::from_str(previous), replicates_clients(name), false);
    if !same_value(&s2sdk::GetConVarValue(handle).to_string(), previous) {
        return false;
    }
    s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] restored {name}={previous}")));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(name: &'static str, previous: &str, written: &str) -> OwnedParameter {
        OwnedParameter {
            name,
            previous: previous.into(),
            written: written.into(),
        }
    }

    #[test]
    fn early_c4_restoration_uses_the_snapshot_and_leaves_other_parameters_owned() {
        for previous in ["true", "false"] {
            let mut parameters = Parameters(vec![
                owned("mp_give_player_c4", previous, "0"),
                owned("mp_ignore_round_win_conditions", "false", "1"),
            ]);
            let mut writes = Vec::new();
            parameters
                .restore_with(Some("mp_give_player_c4"), |name, value, _written| {
                    writes.push((name.to_owned(), value.to_owned()));
                    true
                })
                .unwrap();
            assert_eq!(writes, [("mp_give_player_c4".into(), previous.into())]);
            assert_eq!(parameters.0, [owned("mp_ignore_round_win_conditions", "false", "1")]);
            parameters
                .restore_with(Some("mp_give_player_c4"), |_, _, _| panic!("unowned ConVar written"))
                .unwrap();
            parameters
                .restore_with(None, |name, value, _written| name == "mp_ignore_round_win_conditions" && value == "false")
                .unwrap();
            assert!(parameters.is_restored());
        }
    }

    #[test]
    fn failed_early_c4_restoration_retains_the_snapshot_for_full_cleanup() {
        let mut parameters = Parameters(vec![owned("mp_give_player_c4", "true", "0")]);
        assert_eq!(
            parameters.restore_with(Some("mp_give_player_c4"), |_, _, _| false),
            Err("ConVar restoration failed: mp_give_player_c4".into()),
        );
        assert_eq!(parameters.0, [owned("mp_give_player_c4", "true", "0")]);
        parameters
            .restore_with(None, |name, value, _written| name == "mp_give_player_c4" && value == "true")
            .unwrap();
        assert!(parameters.is_restored());
    }

    #[test]
    fn restoration_panic_keeps_every_convar_snapshot_owned() {
        let mut parameters = Parameters(vec![owned("mp_buytime", "15", "0"), owned("sv_infinite_ammo", "0", "2")]);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parameters.restore())).is_err());
        assert_eq!(parameters.0, [owned("mp_buytime", "15", "0"), owned("sv_infinite_ammo", "0", "2")]);
    }

    #[test]
    fn parameter_failures_are_classified_by_the_actual_write_not_the_current_mode() {
        use crate::fault::Scope;
        assert_eq!(
            parameter_fault("sv_infinite_ammo", "readback".into()).scope,
            Scope::Mechanism(Mechanism::Ammo)
        );
        assert_eq!(
            parameter_fault("mp_anyone_can_pickup_c4", "readback".into()).scope,
            Scope::Mechanism(Mechanism::CarryBomb)
        );
        for (name, _) in BUNNYHOP {
            assert_eq!(parameter_fault(name, "readback".into()).scope, Scope::Mechanism(Mechanism::Bunnyhop));
        }
        assert_eq!(
            parameter_fault("mp_buytime", "readback".into()).scope,
            Scope::Mechanism(Mechanism::BuyTime)
        );
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
        assert!(settings(&mode).contains(&("view_punch_decay", "2".into())));
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
        assert!(!settings(&mode).contains(&("mp_anyone_can_pickup_c4", "1".into())));
        mode.bomb = Some(funrounds_core::BombSettings {
            kind: funrounds_core::BombKind::Carry,
            ..Default::default()
        });
        assert!(settings(&mode).contains(&("mp_anyone_can_pickup_c4", "1".into())));
    }

    #[test]
    fn captured_convars_remain_pending_until_their_snapshot_is_released() {
        assert!(Parameters::default().is_restored());
        assert!(!Parameters(vec![owned("mp_teamname_1", "original name", "Fun Rounds")]).is_restored());
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
    fn generic_mode_leaves_server_settings_untouched_by_default() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        assert!(settings(&mode).is_empty());
        mode.buy_time = Some(0.0);
        assert_eq!(settings(&mode), [("mp_buytime", "0".into())]);
    }

    #[test]
    fn reserve_keeps_reload_and_no_spread_is_replicated_without_missing_recoil_cvars() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        mode.ammo = Ammo::Reserve;
        mode.no_spread = Some(true);
        let settings = settings(&mode);
        assert!(settings.contains(&("sv_infinite_ammo", "2".into())));
        assert!(settings.contains(&("weapon_accuracy_nospread", "1".into())));
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
        mode.ammo = Ammo::Clip;
        let clip = settings(&mode);
        assert!(clip.contains(&("sv_infinite_ammo", "1".into())));
        assert!(!clip.contains(&("sv_infinite_ammo", "2".into())));

        mode.ammo = Ammo::Reserve;
        mode.self_damage = Some(SelfDamage::MissPenalty);
        mode.c4_count = Some(1);
        mode.win_condition = WinCondition::TeamDeathmatch;
        let effects = settings(&mode);
        assert!(effects.contains(&("sv_infinite_ammo", "2".into())));
        assert!(effects.contains(&("mp_weapon_self_inflict_amount", "1".into())));
        assert!(effects.contains(&("mp_give_player_c4", "0".into())));
        assert!(effects.contains(&("mp_ignore_round_win_conditions", "1".into())));
    }
}
