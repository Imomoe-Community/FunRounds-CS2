// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{Ammo, Config, Headshots, Mode, RoundRespawn, TeamFilter, WeaponDamage, WinCondition};
use rand::{Rng, seq::SliceRandom};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub equipment: bool,
    pub health: bool,
    pub armor: bool,
    pub gravity: bool,
    pub speed: bool,
    pub helmet: bool,
    pub clear_weapons: bool,
    pub give_weapons: bool,
    pub give_knife: bool,
    pub ammo: bool,
    pub taser_charge: bool,
    pub bunnyhop: bool,
    pub friction: bool,
    pub time_scale: bool,
    pub ground_acceleration: bool,
    pub ground_reverse: bool,
    pub no_spread: bool,
    pub no_recoil: bool,
    pub recoil_view: bool,
    pub color: bool,
    pub single_shot: bool,
    pub fast_switch: bool,
    pub weapon_damage: bool,
    pub grenade_replenishment: bool,
    pub random_loadout: bool,
    pub suicide: bool,
    pub random_damage: bool,
    pub kill_loadout: bool,
    pub lifesteal: bool,
    pub headshots: bool,
    pub self_damage: bool,
    pub native_self_damage: bool,
    pub reflect_damage: bool,
    pub reflection_lifesteal_frame: bool,
    pub energy_shield: bool,
    pub combat_protection: bool,
    pub translucent: bool,
    pub ghost_visibility: bool,
    pub ghost_breath: bool,
    pub afterburn: bool,
    pub third_person: bool,
    pub delayed_damage: bool,
    pub win_condition: bool,
    pub ct_bomb_no_defuse: bool,
    pub bomb_timing: bool,
    pub chicken_bomb: bool,
    pub carry_bomb: bool,
    pub bomb_bounce: bool,
    pub dodgeball: bool,
    pub throwing_knives: bool,
    pub view_punch_decay: bool,
    pub no_scope: bool,
    pub fov: bool,
    pub hide_crosshair: bool,
    pub health_damage: bool,
    pub health_speed: bool,
    pub health_decay: bool,
    pub self_push: bool,
    pub periodic_speed: bool,
    pub periodic_gravity: bool,
    pub c4_assignment: bool,
    pub c4_purchase: bool,
    pub round_respawn: bool,
    pub round_respawn_all: bool,
    pub hide_radar: bool,
    pub hide_player_identification: bool,
    pub respawn: bool,
    pub buy_time: bool,
    pub buy_limit: bool,
    pub money: bool,
    pub buy_access: bool,
    pub buy_anywhere: bool,
    pub friendly_fire_scale: bool,
    pub teammates_as_enemies: bool,
    pub random_spawn: bool,
    pub drop_weapons: bool,
    pub drop_grenades: bool,
    pub shoot_dropped_grenades: bool,
    pub fall_damage_scale: bool,
    pub healthshot: bool,
    pub ordinary_chickens: bool,
}

impl Capabilities {
    pub fn supports_mode(self, mode: &Mode) -> bool {
        self.missing_for(mode).is_empty()
    }

    pub fn missing_for(self, mode: &Mode) -> Vec<&'static str> {
        self.missing_for_stage(mode, true)
    }

    pub fn supports_mode_with_chicken_config(self, mode: &Mode, chicken_configured: bool) -> bool {
        self.missing_for_with_chicken_config(mode, chicken_configured).is_empty()
    }

    pub fn missing_for_with_chicken_config(self, mode: &Mode, chicken_configured: bool) -> Vec<&'static str> {
        let mut missing = self.missing_for(mode);
        if chicken_configured && mode.ordinary_chickens && !self.ordinary_chickens {
            missing.push("ordinary_chickens");
        }
        missing
    }

    pub fn supports_warmup(self, mode: &Mode) -> bool {
        self.missing_for_stage(mode, false).is_empty()
    }

    pub fn supports_warmup_with_chicken_config(self, mode: &Mode, chicken_configured: bool) -> bool {
        self.supports_warmup(mode) && (!chicken_configured || !mode.ordinary_chickens || self.ordinary_chickens)
    }

    fn missing_for_stage(self, mode: &Mode, require_ct_bomb_no_defuse: bool) -> Vec<&'static str> {
        [
            (mode.clear_weapons != TeamFilter::None, self.clear_weapons, "clear_weapons"),
            (
                mode.give_weapons != TeamFilter::None && !mode.weapons.is_empty(),
                self.give_weapons,
                "give_weapons",
            ),
            (mode.give_knife != TeamFilter::None, self.give_knife, "give_knife"),
            (
                mode.give_weapons != TeamFilter::None && !mode.weapons.is_empty(),
                self.equipment,
                "equipment",
            ),
            (true, self.health, "health"),
            (true, self.armor, "armor"),
            (true, self.helmet, "helmet"),
            (true, self.gravity, "gravity"),
            (true, self.speed, "speed"),
            (matches!(mode.ammo, Ammo::Reserve | Ammo::Clip), self.ammo, "ammo"),
            (mode.ammo == Ammo::SingleShot, self.single_shot, "single_shot"),
            (mode.taser_charge.is_some(), self.taser_charge, "taser_charge"),
            (mode.bunnyhop.is_some(), self.bunnyhop, "bunnyhop"),
            (mode.friction.is_some(), self.friction, "friction"),
            (mode.time_scale.is_some(), self.time_scale, "time_scale"),
            (mode.ground_acceleration.is_some(), self.ground_acceleration, "ground_acceleration"),
            (mode.ground_reverse.is_some(), self.ground_reverse, "ground_reverse"),
            (mode.no_spread.is_some(), self.no_spread, "no_spread"),
            (mode.no_recoil, self.no_recoil, "no_recoil"),
            (mode.recoil_view.is_some(), self.recoil_view, "recoil_view"),
            (mode.color.is_some(), self.color, "color"),
            (mode.fast_switch, self.fast_switch, "fast_switch"),
            (mode.weapon_damage != WeaponDamage::default(), self.weapon_damage, "weapon_damage"),
            (!mode.replenish.is_empty(), self.grenade_replenishment, "grenade_replenishment"),
            (mode.random_loadout, self.random_loadout, "random_loadout"),
            (mode.suicide, self.suicide, "suicide"),
            (mode.random_damage, self.random_damage, "random_damage"),
            (mode.kill_loadout, self.kill_loadout, "kill_loadout"),
            (mode.lifesteal, self.lifesteal, "lifesteal"),
            (mode.headshots != Headshots::Normal, self.headshots, "headshots"),
            (mode.self_damage.is_some(), self.self_damage, "self_damage"),
            (mode.native_self_damage_enabled(), self.native_self_damage, "native_self_damage"),
            (mode.reflect_damage.is_some(), self.reflect_damage, "reflect_damage"),
            (
                mode.reflect_damage.is_some() && mode.lifesteal,
                self.reflection_lifesteal_frame,
                "reflection_lifesteal_frame",
            ),
            (mode.energy_shield, self.energy_shield, "energy_shield"),
            (mode.spawn_protection || mode.kill_protection, self.combat_protection, "combat_protection"),
            (mode.translucent, self.translucent, "translucent"),
            (mode.ghost.is_some(), self.translucent, "translucent"),
            (mode.ghost.is_some(), self.ghost_visibility, "ghost_visibility"),
            (mode.afterburn.is_some(), self.afterburn, "afterburn"),
            (mode.third_person.is_some(), self.third_person, "third_person"),
            (
                mode.ghost.is_some_and(|ghost| ghost.breath_interval > 0.0),
                self.ghost_breath,
                "ghost_breath",
            ),
            (mode.delayed_damage, self.delayed_damage, "delayed_damage"),
            (mode.win_condition == WinCondition::TeamDeathmatch, self.win_condition, "win_condition"),
            (require_ct_bomb_no_defuse, self.ct_bomb_no_defuse, "ct_bomb_no_defuse"),
            (mode.bomb.is_some(), self.bomb_timing, "bomb_timing"),
            (
                mode.bomb.as_ref().is_some_and(|bomb| bomb.kind == crate::BombKind::Chicken),
                self.chicken_bomb,
                "chicken_bomb",
            ),
            (
                mode.bomb.as_ref().is_some_and(|bomb| bomb.kind == crate::BombKind::Carry),
                self.carry_bomb,
                "carry_bomb",
            ),
            (mode.bomb.as_ref().is_some_and(|bomb| bomb.bounce), self.bomb_bounce, "bomb_bounce"),
            (mode.dodgeball, self.dodgeball, "dodgeball"),
            (mode.throwing_knives, self.throwing_knives, "throwing_knives"),
            (mode.view_punch_decay.is_some(), self.view_punch_decay, "view_punch_decay"),
            (mode.no_scope, self.no_scope, "no_scope"),
            (mode.fov.is_some(), self.fov, "fov"),
            (mode.hide_crosshair_after.is_some(), self.hide_crosshair, "hide_crosshair"),
            (mode.health_damage, self.health_damage, "health_damage"),
            (mode.health_speed, self.health_speed, "health_speed"),
            (mode.health_decay, self.health_decay, "health_decay"),
            (mode.self_push, self.self_push, "self_push"),
            (mode.periodic_speed, self.periodic_speed, "periodic_speed"),
            (mode.periodic_gravity, self.periodic_gravity, "periodic_gravity"),
            (mode.c4_count.is_some(), self.c4_assignment, "c4_assignment"),
            (mode.c4_buy, self.c4_purchase, "c4_purchase"),
            (mode.round_respawn == RoundRespawn::Dead, self.round_respawn, "round_respawn"),
            (mode.round_respawn == RoundRespawn::All, self.round_respawn_all, "round_respawn_all"),
            (mode.hide_radar.is_some(), self.hide_radar, "hide_radar"),
            (
                mode.hide_player_identification.is_some(),
                self.hide_player_identification,
                "hide_player_identification",
            ),
            (mode.respawn.is_some(), self.respawn, "respawn"),
            (mode.buy_time.is_some(), self.buy_time, "buy_time"),
            (mode.buy_limit.is_some(), self.buy_limit, "buy_limit"),
            (mode.round_money_floor.is_some() || mode.money_income.is_some(), self.money, "money"),
            (mode.buy_access.is_some(), self.buy_access, "buy_access"),
            (mode.buy_anywhere.is_some(), self.buy_anywhere, "buy_anywhere"),
            (mode.friendly_fire_scale.is_some(), self.friendly_fire_scale, "friendly_fire_scale"),
            (mode.teammates_as_enemies.is_some(), self.teammates_as_enemies, "teammates_as_enemies"),
            (mode.random_spawn.is_some(), self.random_spawn, "random_spawn"),
            (mode.drop_weapons.is_some(), self.drop_weapons, "drop_weapons"),
            (mode.drop_grenades.is_some(), self.drop_grenades, "drop_grenades"),
            (
                mode.shoot_dropped_grenades.is_some(),
                self.shoot_dropped_grenades,
                "shoot_dropped_grenades",
            ),
            (mode.fall_damage_scale.is_some(), self.fall_damage_scale, "fall_damage_scale"),
        ]
        .into_iter()
        .filter(|(required, available, _)| *required && !*available)
        .map(|(_, _, name)| name)
        .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundGeneration(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundKey {
    map: u64,
    sequence: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Preparing,
    Running,
    Warmup,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextMode {
    Random,
    Id(String),
}

pub struct RoundController {
    config: Option<Config>,
    pending_config: Option<Config>,
    capabilities: Capabilities,
    enabled: bool,
    remaining: Vec<usize>,
    last_mode: Option<String>,
    forced_next: Option<String>,
    current: Option<usize>,
    last_start: Option<RoundKey>,
    map: u64,
    generation: RoundGeneration,
    phase: Phase,
}

impl RoundController {
    pub fn new(config: Option<Config>, capabilities: Capabilities) -> Result<Self, String> {
        if let Some(config) = &config {
            config.validate()?;
        }
        Ok(Self {
            config,
            pending_config: None,
            capabilities,
            enabled: true,
            remaining: Vec::new(),
            last_mode: None,
            forced_next: None,
            current: None,
            last_start: None,
            map: 0,
            generation: RoundGeneration(0),
            phase: Phase::Idle,
        })
    }

    pub fn config(&self) -> Option<&Config> {
        self.config.as_ref()
    }

    pub fn hostage_rescue_compat_enabled(&self) -> bool {
        self.config.as_ref().is_some_and(|config| config.hostage_rescue_compat)
    }

    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    pub fn pending_config(&self) -> Option<&Config> {
        self.pending_config.as_ref()
    }

    pub fn current_mode(&self) -> Option<&Mode> {
        if self.phase == Phase::Warmup {
            return self
                .config
                .as_ref()?
                .warmup
                .as_ref()
                .filter(|warmup| warmup.enabled)
                .map(|warmup| &warmup.mode);
        }
        self.current.and_then(|index| self.config.as_ref()?.modes.get(index))
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn generation(&self) -> RoundGeneration {
        self.generation
    }

    pub fn round_key(&self, sequence: u64) -> RoundKey {
        RoundKey { map: self.map, sequence }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn forced_next(&self) -> Option<&str> {
        self.forced_next.as_deref()
    }

    pub fn queue_reload(&mut self, source: &str) -> Result<(), String> {
        let config = Config::parse(source)?;
        self.pending_config = Some(config);
        Ok(())
    }

    pub fn force_next(&mut self, next: NextMode) -> Result<(), String> {
        match next {
            NextMode::Random => self.forced_next = None,
            NextMode::Id(id) => {
                let config = self.pending_config.as_ref().or(self.config.as_ref());
                if !config.is_some_and(|config| {
                    config.modes.iter().any(|mode| {
                        mode.id == id && mode.enabled && self.capabilities.supports_mode_with_chicken_config(mode, config.chicken.is_some())
                    })
                }) {
                    return Err(format!("mode is disabled, unavailable or unknown: {id}"));
                }
                self.forced_next = Some(id);
            }
        }
        Ok(())
    }

    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        if self.enabled == enabled {
            return false;
        }
        self.enabled = enabled;
        self.clear_round();
        true
    }

    pub fn start_warmup(&mut self) -> bool {
        self.clear_round();
        let Some(config) = self.config.as_ref() else {
            return false;
        };
        let Some(warmup) = config.warmup.as_ref() else {
            return false;
        };
        if !self.enabled
            || !warmup.enabled
            || !self
                .capabilities
                .supports_warmup_with_chicken_config(&warmup.mode, config.chicken.is_some())
        {
            return false;
        }
        self.phase = Phase::Warmup;
        true
    }

    pub fn stop_warmup(&mut self) -> bool {
        if self.phase != Phase::Warmup {
            return false;
        }
        self.clear_round();
        true
    }

    pub fn clear_active_round(&mut self) {
        self.clear_round();
    }

    pub fn set_capabilities(&mut self, capabilities: Capabilities) -> bool {
        if self.capabilities == capabilities {
            return false;
        }
        self.capabilities = capabilities;
        self.remaining.clear();
        self.clear_round();
        true
    }

    pub fn is_new_round(&self, key: RoundKey) -> bool {
        key.map == self.map && self.last_start.is_none_or(|previous| key.sequence > previous.sequence)
    }

    // The adapter supplies a monotonically increasing engine round identity within each map.
    pub fn start_round(&mut self, key: RoundKey, warmup: bool, rng: &mut impl Rng) -> bool {
        if key.map != self.map || self.last_start.is_some_and(|previous| key.sequence < previous.sequence) {
            return false;
        }
        if self.last_start == Some(key) {
            if warmup && self.phase != Phase::Idle {
                self.clear_round();
                return true;
            }
            return false;
        }
        self.clear_round();
        self.last_start = Some(key);
        if let Some(config) = self.pending_config.take() {
            self.config = Some(config);
            self.remaining.clear();
        }
        if warmup || !self.enabled {
            return true;
        }
        let Some(config) = &self.config else {
            return true;
        };
        let chicken_configured = config.chicken.is_some();
        if self.remaining.is_empty() {
            self.remaining = config
                .modes
                .iter()
                .enumerate()
                .filter_map(|(index, mode)| {
                    (mode.enabled && self.capabilities.supports_mode_with_chicken_config(mode, chicken_configured)).then_some(index)
                })
                .collect();
            self.remaining.shuffle(rng);
            if self.remaining.len() > 1
                && self
                    .remaining
                    .last()
                    .is_some_and(|&index| self.last_mode.as_deref() == Some(&config.modes[index].id))
            {
                let last = self.remaining.len() - 1;
                self.remaining.swap(0, last);
            }
        }
        let forced = self.forced_next.take().and_then(|id| {
            config
                .modes
                .iter()
                .position(|mode| mode.id == id && mode.enabled && self.capabilities.supports_mode_with_chicken_config(mode, chicken_configured))
        });
        let selected = if let Some(index) = forced {
            if let Some(position) = self.remaining.iter().position(|&remaining| remaining == index) {
                self.remaining.remove(position);
            }
            Some(index)
        } else {
            self.remaining.pop()
        };
        if let Some(index) = selected {
            self.last_mode = Some(config.modes[index].id.clone());
            self.current = Some(index);
            self.phase = Phase::Preparing;
        }
        true
    }

    pub fn freeze_end(&mut self, key: RoundKey) -> bool {
        if self.last_start != Some(key) || self.phase != Phase::Preparing {
            return false;
        }
        self.phase = Phase::Running;
        true
    }

    pub fn end_round(&mut self, key: RoundKey) -> bool {
        if self.last_start != Some(key) || self.phase == Phase::Idle {
            return false;
        }
        self.clear_round();
        true
    }

    pub fn reset(&mut self) {
        self.clear_round();
        self.last_start = None;
        self.map = self.map.checked_add(1).expect("map generation exhausted");
    }

    fn clear_round(&mut self) {
        self.generation.0 = self.generation.0.checked_add(1).expect("round generation exhausted");
        self.current = None;
        self.phase = Phase::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    use std::collections::HashSet;

    const EXAMPLE: &str = include_str!("../../../config/funrounds.example.toml");
    fn available() -> Capabilities {
        Capabilities {
            equipment: true,
            health: true,
            armor: true,
            gravity: true,
            speed: true,
            helmet: true,
            clear_weapons: true,
            give_weapons: true,
            give_knife: true,
            ct_bomb_no_defuse: true,
            ..Capabilities::default()
        }
    }

    fn fully_available() -> Capabilities {
        Capabilities {
            ammo: true,
            taser_charge: true,
            bunnyhop: true,
            friction: true,
            no_spread: true,
            no_recoil: true,
            recoil_view: true,
            color: true,
            single_shot: true,
            fast_switch: true,
            weapon_damage: true,
            grenade_replenishment: true,
            random_loadout: true,
            suicide: true,
            random_damage: true,
            kill_loadout: true,
            lifesteal: true,
            headshots: true,
            self_damage: true,
            native_self_damage: true,
            reflect_damage: true,
            reflection_lifesteal_frame: true,
            energy_shield: true,
            combat_protection: true,
            translucent: true,
            delayed_damage: true,
            win_condition: true,
            bomb_timing: true,
            chicken_bomb: true,
            carry_bomb: true,
            bomb_bounce: true,
            dodgeball: true,
            throwing_knives: true,
            view_punch_decay: true,
            no_scope: true,
            fov: true,
            hide_crosshair: true,
            health_damage: true,
            health_speed: true,
            health_decay: true,
            self_push: true,
            periodic_speed: true,
            periodic_gravity: true,
            c4_assignment: true,
            c4_purchase: true,
            round_respawn: true,
            respawn: true,
            buy_time: true,
            buy_limit: true,
            friendly_fire_scale: true,
            teammates_as_enemies: true,
            random_spawn: true,
            drop_weapons: true,
            drop_grenades: true,
            fall_damage_scale: true,
            ..available()
        }
    }

    #[test]
    fn equipment_capability_gates_the_public_example() {
        let config = Config::parse(EXAMPLE).unwrap();
        let missing_equipment = Capabilities {
            equipment: false,
            ..fully_available()
        };
        let mut controller = RoundController::new(Some(config), missing_equipment).unwrap();
        let mut rng = StdRng::seed_from_u64(1);
        assert!(controller.start_round(controller.round_key(1), false, &mut rng));
        assert_eq!(controller.phase(), Phase::Idle);
        assert!(controller.force_next(NextMode::Id("example".into())).is_err());
        assert!(controller.queue_reload(EXAMPLE).is_ok());
        assert!(controller.pending_config().is_some());
        controller.set_capabilities(fully_available());
        assert!(controller.force_next(NextMode::Id("example".into())).is_ok());
        assert!(controller.start_round(controller.round_key(2), false, &mut rng));
        assert_eq!(controller.phase(), Phase::Preparing);
    }

    fn controller() -> RoundController {
        RoundController::new(Some(Config::parse(EXAMPLE).unwrap()), available()).unwrap()
    }

    fn generic_pool(count: usize) -> Config {
        let mut config = Config::parse(EXAMPLE).unwrap();
        let template = config.modes[0].clone();
        config.modes = (0..count)
            .map(|index| {
                let mut mode = template.clone();
                mode.id = format!("example_{index}");
                mode.name = format!("Example {index}");
                mode
            })
            .collect();
        config
    }

    #[test]
    fn cycles_cover_each_mode_and_avoid_boundary_repetition() {
        for seed in 0..64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut controller = RoundController::new(Some(generic_pool(3)), available()).unwrap();
            let mut previous = None;
            for cycle in 0..20 {
                let mut ids = HashSet::new();
                for offset in 0..3 {
                    assert!(controller.start_round(controller.round_key(cycle * 3 + offset), false, &mut rng));
                    let id = controller.current_mode().unwrap().id.clone();
                    assert_ne!(previous.as_ref(), Some(&id));
                    assert!(ids.insert(id.clone()));
                    previous = Some(id);
                }
                assert_eq!(ids.len(), 3);
            }
        }
    }

    #[test]
    fn disabled_modes_are_excluded_and_a_single_mode_repeats() {
        let mut config = generic_pool(3);
        config.modes[0].enabled = false;
        config.modes[1].enabled = false;
        let mut controller = RoundController::new(Some(config), available()).unwrap();
        let mut rng = StdRng::seed_from_u64(0);
        for round in 0..10 {
            controller.start_round(controller.round_key(round), false, &mut rng);
            assert_eq!(controller.current_mode().unwrap().id, "example_2");
        }
    }

    #[test]
    fn forced_selection_consumes_the_pending_cycle_and_random_cancels_it() {
        let mut controller = RoundController::new(Some(generic_pool(3)), available()).unwrap();
        let mut rng = StdRng::seed_from_u64(1);
        controller.force_next(NextMode::Id("example_0".into())).unwrap();
        controller.start_round(controller.round_key(1), false, &mut rng);
        assert_eq!(controller.current_mode().unwrap().id, "example_0");
        let mut ids = HashSet::from(["example_0".to_string()]);
        for round in 2..=3 {
            controller.start_round(controller.round_key(round), false, &mut rng);
            assert!(ids.insert(controller.current_mode().unwrap().id.clone()));
        }
        controller.force_next(NextMode::Id("example_0".into())).unwrap();
        assert!(controller.force_next(NextMode::Id("unknown".into())).is_err());
        assert_eq!(controller.forced_next(), Some("example_0"));
        controller.force_next(NextMode::Random).unwrap();
        assert_eq!(controller.forced_next(), None);
    }

    #[test]
    fn invalid_reload_retains_active_and_pending_configuration() {
        let mut controller = controller();
        let mut rng = StdRng::seed_from_u64(0);
        controller.force_next(NextMode::Id("example".into())).unwrap();
        controller.start_round(controller.round_key(1), false, &mut rng);
        controller.queue_reload(&EXAMPLE.replace("health = 120", "health = 140")).unwrap();
        let active = controller.config().cloned();
        let pending = controller.pending_config().cloned();
        for value in ["false", "true"] {
            assert!(controller.queue_reload(&format!("unknown = {value}\n{EXAMPLE}")).is_err());
            assert_eq!(controller.config(), active.as_ref());
            assert_eq!(controller.pending_config(), pending.as_ref());
        }
        assert!(controller.queue_reload("diagnostics = false\nmodes = []").is_err());
        assert_eq!(controller.current_mode().unwrap().health, crate::AttributeValue::Fixed(120));
        assert_eq!(controller.pending_config().unwrap().modes[0].health, crate::AttributeValue::Fixed(140));
        controller.force_next(NextMode::Id("example".into())).unwrap();
        controller.start_round(controller.round_key(2), false, &mut rng);
        assert_eq!(controller.current_mode().unwrap().health, crate::AttributeValue::Fixed(140));
        assert!(controller.pending_config().is_none());
    }

    #[test]
    fn diagnostics_reload_waits_for_the_next_distinct_round() {
        let mut controller = controller();
        let mut rng = StdRng::seed_from_u64(0);
        let first = controller.round_key(1);
        controller.start_round(first, false, &mut rng);
        assert!(!controller.config().unwrap().diagnostics);
        controller
            .queue_reload(&format!("diagnostics = true\n{}", EXAMPLE.replace("diagnostics = false", "")))
            .unwrap();
        assert!(!controller.config().unwrap().diagnostics);
        assert!(controller.pending_config().unwrap().diagnostics);
        assert!(!controller.start_round(first, false, &mut rng));
        assert!(!controller.config().unwrap().diagnostics);
        controller.start_round(controller.round_key(2), false, &mut rng);
        assert!(controller.config().unwrap().diagnostics);
    }

    #[test]
    fn lifecycle_ignores_duplicates_and_old_round_events() {
        let mut controller = controller();
        let mut rng = StdRng::seed_from_u64(0);
        controller.start_round(controller.round_key(1), false, &mut rng);
        let first = controller.generation();
        let mode = controller.current_mode().unwrap().id.clone();
        assert!(!controller.start_round(controller.round_key(1), false, &mut rng));
        assert_eq!(controller.generation(), first);
        assert_eq!(controller.current_mode().unwrap().id, mode);
        assert!(controller.freeze_end(controller.round_key(1)));
        assert!(!controller.freeze_end(controller.round_key(1)));
        controller.start_round(controller.round_key(2), false, &mut rng);
        assert_ne!(controller.generation(), first);
        assert!(!controller.start_round(controller.round_key(1), false, &mut rng));
        assert!(!controller.freeze_end(controller.round_key(1)));
        assert!(!controller.end_round(controller.round_key(1)));
        assert_eq!(controller.phase(), Phase::Preparing);
        assert!(controller.end_round(controller.round_key(2)));
        assert!(!controller.end_round(controller.round_key(2)));
        assert!(!controller.start_round(controller.round_key(2), false, &mut rng));
        assert!(controller.current_mode().is_none());
    }

    #[test]
    fn warmup_disable_and_missing_capabilities_do_not_consume_modes() {
        let mut controller = controller();
        let mut rng = StdRng::seed_from_u64(0);
        controller.force_next(NextMode::Id("example".into())).unwrap();
        controller.start_round(controller.round_key(1), true, &mut rng);
        assert_eq!(controller.forced_next(), Some("example"));
        assert!(controller.current_mode().is_none());
        assert!(!controller.freeze_end(controller.round_key(1)));
        assert!(controller.set_enabled(false));
        controller.start_round(controller.round_key(2), false, &mut rng);
        assert!(controller.current_mode().is_none());
        controller.set_enabled(true);
        assert!(!controller.start_round(controller.round_key(2), false, &mut rng));
        controller.start_round(controller.round_key(3), false, &mut rng);
        assert_eq!(controller.current_mode().unwrap().id, "example");
        let active = controller.generation();
        controller.set_capabilities(Capabilities::default());
        assert_ne!(controller.generation(), active);
        assert_eq!(controller.phase(), Phase::Idle);
        controller.start_round(controller.round_key(4), false, &mut rng);
        assert!(controller.current_mode().is_none());
        assert!(controller.force_next(NextMode::Id("example".into())).is_err());
    }

    #[test]
    fn missing_configuration_can_be_recovered_on_a_later_round() {
        let mut controller = RoundController::new(None, available()).unwrap();
        let mut rng = StdRng::seed_from_u64(0);
        controller.start_round(controller.round_key(1), false, &mut rng);
        assert!(controller.current_mode().is_none());
        assert!(controller.queue_reload("diagnostics = false\nmodes = []").is_err());
        assert!(controller.config().is_none());
        controller.queue_reload(EXAMPLE).unwrap();
        controller.start_round(controller.round_key(2), false, &mut rng);
        assert!(controller.current_mode().is_some());
        let before_map = controller.generation();
        controller.reset();
        assert_ne!(controller.generation(), before_map);
        assert!(controller.current_mode().is_none());
        assert!(controller.start_round(controller.round_key(2), false, &mut rng));
    }

    #[test]
    fn constructor_rejects_unvalidated_public_configuration() {
        let mut config = Config::parse(EXAMPLE).unwrap();
        config.modes[0].health = crate::AttributeValue::Fixed(0);
        assert!(RoundController::new(Some(config), available()).is_err());
    }

    #[test]
    fn warmup_transitions_cannot_replay_an_ended_round() {
        let mut controller = controller();
        let mut rng = StdRng::seed_from_u64(0);
        let key = controller.round_key(7);
        assert!(controller.start_round(key, false, &mut rng));
        assert!(controller.end_round(key));
        let ended = controller.generation();
        assert!(!controller.start_round(key, true, &mut rng));
        assert!(!controller.start_round(key, false, &mut rng));
        assert!(!controller.freeze_end(key));
        assert_eq!(controller.generation(), ended);
        assert!(controller.current_mode().is_none());
    }

    #[test]
    fn entering_warmup_cleans_an_active_round_without_reselecting_it() {
        let mut controller = controller();
        let mut rng = StdRng::seed_from_u64(0);
        let key = controller.round_key(1);
        controller.start_round(key, false, &mut rng);
        let active = controller.generation();
        assert!(controller.start_round(key, true, &mut rng));
        assert_ne!(controller.generation(), active);
        assert!(controller.current_mode().is_none());
        assert!(!controller.start_round(key, false, &mut rng));
        assert!(!controller.freeze_end(key));
        assert!(controller.start_round(controller.round_key(2), false, &mut rng));
    }

    #[test]
    fn old_map_callbacks_cannot_touch_a_reused_round_sequence() {
        let mut controller = controller();
        let mut rng = StdRng::seed_from_u64(0);
        let old = controller.round_key(1);
        controller.start_round(old, false, &mut rng);
        controller.reset();
        assert!(!controller.start_round(old, false, &mut rng));
        let new = controller.round_key(1);
        controller.start_round(new, false, &mut rng);
        assert!(!controller.freeze_end(old));
        assert!(!controller.end_round(old));
        assert_eq!(controller.phase(), Phase::Preparing);
        assert!(controller.freeze_end(new));
    }

    #[test]
    fn unavailable_capability_filters_only_the_dependent_mode() {
        let mut config = generic_pool(2);
        config.modes[1].bunnyhop = Some(true);
        let mut capabilities = fully_available();
        capabilities.bunnyhop = false;
        assert!(capabilities.supports_mode(&config.modes[0]));
        assert!(!capabilities.supports_mode(&config.modes[1]));
        assert_eq!(capabilities.missing_for(&config.modes[1]), ["bunnyhop"]);

        let mut controller = RoundController::new(Some(config), capabilities).unwrap();
        assert!(controller.force_next(NextMode::Id("example_1".into())).is_err());
        controller.force_next(NextMode::Id("example_0".into())).unwrap();
    }
}
