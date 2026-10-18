// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{Config, Mode};
use rand::{Rng, seq::SliceRandom};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub classic: bool,
    pub equipment: bool,
    pub health: bool,
    pub armor: bool,
    pub gravity: bool,
    pub unlimited_reserve: bool,
    pub taser_charge: bool,
    pub bunnyhop: bool,
    pub friction: bool,
    pub no_spread_recoil: bool,
    pub recoil_view: bool,
    pub blue: bool,
    pub replenish_he: bool,
    pub random_loadout: bool,
    pub kill_loadout: bool,
    pub lifesteal: bool,
    pub unlimited_clip: bool,
    pub headshots_only: bool,
    pub miss_damage: bool,
    pub team_deathmatch: bool,
    pub bomb_timing: bool,
    pub chicken_bomb: bool,
    pub carry_bomb: bool,
    pub bomb_bounce: bool,
    pub dodgeball: bool,
    pub throwing_knives: bool,
    pub no_headshots: bool,
    pub replenish_decoy: bool,
    pub view_punch_decay: bool,
}

impl Capabilities {
    pub fn supports_basic_effects(self) -> bool {
        self.equipment && self.health && self.armor && self.gravity
    }

    pub fn supports_mode(self, mode: &Mode) -> bool {
        self.missing_for(mode).is_empty()
    }

    pub fn missing_for(self, mode: &Mode) -> Vec<&'static str> {
        [
            (true, self.equipment, "equipment"),
            (true, self.health, "health"),
            (true, self.armor, "armor"),
            (true, self.gravity, "gravity"),
            (mode.unlimited_reserve, self.unlimited_reserve, "unlimited_reserve"),
            (mode.taser_charge.is_some(), self.taser_charge, "taser_charge"),
            (mode.bunnyhop, self.bunnyhop, "bunnyhop"),
            (mode.friction.is_some(), self.friction, "friction"),
            (mode.no_spread_recoil, self.no_spread_recoil, "no_spread_recoil"),
            (mode.recoil_view.is_some(), self.recoil_view, "recoil_view"),
            (mode.blue, self.blue, "blue"),
            (mode.replenish_he, self.replenish_he, "replenish_he"),
            (mode.random_loadout, self.random_loadout, "random_loadout"),
            (mode.kill_loadout, self.kill_loadout, "kill_loadout"),
            (mode.lifesteal, self.lifesteal, "lifesteal"),
            (mode.unlimited_clip, self.unlimited_clip, "unlimited_clip"),
            (mode.headshots_only, self.headshots_only, "headshots_only"),
            (mode.miss_damage, self.miss_damage, "miss_damage"),
            (mode.team_deathmatch, self.team_deathmatch, "team_deathmatch"),
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
            (mode.no_headshots, self.no_headshots, "no_headshots"),
            (mode.replenish_decoy, self.replenish_decoy, "replenish_decoy"),
            (mode.view_punch_decay.is_some(), self.view_punch_decay, "view_punch_decay"),
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

    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    pub fn pending_config(&self) -> Option<&Config> {
        self.pending_config.as_ref()
    }

    pub fn current_mode(&self) -> Option<&Mode> {
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
        if !self.capabilities.classic {
            return Err("public behavior requires all classic capabilities".into());
        }
        self.pending_config = Some(config);
        Ok(())
    }

    pub fn force_next(&mut self, next: NextMode) -> Result<(), String> {
        match next {
            NextMode::Random => self.forced_next = None,
            NextMode::Id(id) => {
                let config = self.pending_config.as_ref().or(self.config.as_ref());
                if !self.capabilities.classic
                    || !config.is_some_and(|config| {
                        config
                            .modes
                            .iter()
                            .any(|mode| mode.id == id && mode.enabled && self.capabilities.supports_mode(mode))
                    })
                {
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
        if warmup || !self.enabled || !self.capabilities.supports_basic_effects() {
            return true;
        }
        let Some(config) = &self.config else {
            return true;
        };
        if !self.capabilities.classic {
            return true;
        }
        if self.remaining.is_empty() {
            self.remaining = config
                .modes
                .iter()
                .enumerate()
                .filter_map(|(index, mode)| (mode.enabled && self.capabilities.supports_mode(mode)).then_some(index))
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
                .position(|mode| mode.id == id && mode.enabled && self.capabilities.supports_mode(mode))
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
    const AVAILABLE: Capabilities = Capabilities {
        classic: true,
        equipment: true,
        health: true,
        armor: true,
        gravity: true,
        unlimited_reserve: false,
        taser_charge: false,
        bunnyhop: false,
        friction: false,
        no_spread_recoil: false,
        recoil_view: false,
        blue: false,
        replenish_he: false,
        random_loadout: false,
        kill_loadout: false,
        lifesteal: false,
        unlimited_clip: false,
        headshots_only: false,
        miss_damage: false,
        team_deathmatch: false,
        bomb_timing: false,
        chicken_bomb: false,
        carry_bomb: false,
        bomb_bounce: false,
        dodgeball: false,
        throwing_knives: false,
        no_headshots: false,
        replenish_decoy: false,
        view_punch_decay: false,
    };
    const FULLY_AVAILABLE: Capabilities = Capabilities {
        classic: true,
        unlimited_reserve: true,
        taser_charge: true,
        bunnyhop: true,
        friction: true,
        no_spread_recoil: true,
        recoil_view: true,
        blue: true,
        replenish_he: true,
        random_loadout: true,
        kill_loadout: true,
        lifesteal: true,
        unlimited_clip: true,
        headshots_only: true,
        miss_damage: true,
        team_deathmatch: true,
        bomb_timing: true,
        chicken_bomb: true,
        carry_bomb: true,
        bomb_bounce: true,
        dodgeball: true,
        throwing_knives: true,
        no_headshots: true,
        replenish_decoy: true,
        view_punch_decay: true,
        ..AVAILABLE
    };

    #[test]
    fn shared_behavior_capability_gates_every_config() {
        let config = Config::parse(EXAMPLE).unwrap();
        let missing_classic = Capabilities {
            classic: false,
            ..FULLY_AVAILABLE
        };
        let mut controller = RoundController::new(Some(config), missing_classic).unwrap();
        let mut rng = StdRng::seed_from_u64(1);
        assert!(controller.start_round(controller.round_key(1), false, &mut rng));
        assert_eq!(controller.phase(), Phase::Idle);
        assert!(controller.force_next(NextMode::Id("example".into())).is_err());
        assert!(controller.queue_reload(EXAMPLE).is_err());
        assert!(controller.pending_config().is_none());
        controller.set_capabilities(FULLY_AVAILABLE);
        assert!(controller.force_next(NextMode::Id("example".into())).is_ok());
        assert!(controller.start_round(controller.round_key(2), false, &mut rng));
        assert_eq!(controller.phase(), Phase::Preparing);
    }

    fn controller() -> RoundController {
        RoundController::new(Some(Config::parse(EXAMPLE).unwrap()), AVAILABLE).unwrap()
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
            let mut controller = RoundController::new(Some(generic_pool(3)), AVAILABLE).unwrap();
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
        let mut controller = RoundController::new(Some(config), AVAILABLE).unwrap();
        let mut rng = StdRng::seed_from_u64(0);
        for round in 0..10 {
            controller.start_round(controller.round_key(round), false, &mut rng);
            assert_eq!(controller.current_mode().unwrap().id, "example_2");
        }
    }

    #[test]
    fn forced_selection_consumes_the_pending_cycle_and_random_cancels_it() {
        let mut controller = RoundController::new(Some(generic_pool(3)), AVAILABLE).unwrap();
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
        assert!(controller.queue_reload("modes = []").is_err());
        assert_eq!(controller.current_mode().unwrap().health, 120);
        assert_eq!(controller.pending_config().unwrap().modes[0].health, 140);
        controller.force_next(NextMode::Id("example".into())).unwrap();
        controller.start_round(controller.round_key(2), false, &mut rng);
        assert_eq!(controller.current_mode().unwrap().health, 140);
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
        let mut controller = RoundController::new(None, AVAILABLE).unwrap();
        let mut rng = StdRng::seed_from_u64(0);
        controller.start_round(controller.round_key(1), false, &mut rng);
        assert!(controller.current_mode().is_none());
        assert!(controller.queue_reload("modes = []").is_err());
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
        config.modes[0].health = 0;
        assert!(RoundController::new(Some(config), AVAILABLE).is_err());
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
        config.modes[1].bunnyhop = true;
        let mut capabilities = FULLY_AVAILABLE;
        capabilities.bunnyhop = false;
        assert!(capabilities.supports_mode(&config.modes[0]));
        assert!(!capabilities.supports_mode(&config.modes[1]));
        assert_eq!(capabilities.missing_for(&config.modes[1]), ["bunnyhop"]);

        let mut controller = RoundController::new(Some(config), capabilities).unwrap();
        assert!(controller.force_next(NextMode::Id("example_1".into())).is_err());
        controller.force_next(NextMode::Id("example_0".into())).unwrap();
    }
}
