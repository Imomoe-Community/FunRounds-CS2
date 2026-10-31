// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use funrounds_core::Capabilities;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mechanism {
    Ammo,
    TaserCharge,
    Bunnyhop,
    Friction,
    TimeScale,
    GroundAcceleration,
    GroundReverse,
    NoSpread,
    NoRecoil,
    BuyTime,
    BuyLimit,
    BuyAccess,
    BuyAnywhere,
    Money,
    FriendlyFireScale,
    TeammatesAsEnemies,
    RandomSpawn,
    HideRadar,
    HidePlayerIdentification,
    DropWeapons,
    DropGrenades,
    ShootDroppedGrenades,
    FallDamageScale,
    RecoilView,
    Color,
    SingleShot,
    FastSwitch,
    GrenadeRefill,
    KillLoadout,
    SelfDamage,
    NativeSelfDamage,
    EnergyShield,
    Translucent,
    GhostVisibility,
    GhostBreath,
    DelayedDamage,
    Reflection,
    ReflectionLifestealFrame,
    TeamDeathmatch,
    BombTiming,
    ChickenBomb,
    CarryBomb,
    MovingBlast,
    ProjectileDamage,
    Dodgeball,
    ThrowingKnives,
    ViewPunchDecay,
    NoScope,
    Fov,
    ThirdPerson,
    Crosshair,
    PeriodicMovement,
    NoC4,
    Classic,
    HealthSpeed,
    HealthDecay,
    PeriodicSpeed,
    PeriodicGravity,
    WinCondition,
    CtBombNoDefuse,
    C4Assignment,
    RandomDamage,
    Suicide,
    Healthshot,
    ChickenFollowBlocking,
    ChickenEating,
    Afterburn,
}

impl Mechanism {
    fn exclude(self, capabilities: &mut Capabilities) {
        match self {
            Self::Ammo => {
                capabilities.ammo = false;
                capabilities.single_shot = false;
            }
            Self::TaserCharge => capabilities.taser_charge = false,
            Self::Bunnyhop => capabilities.bunnyhop = false,
            Self::Friction => capabilities.friction = false,
            Self::TimeScale => capabilities.time_scale = false,
            Self::GroundAcceleration => {
                capabilities.ground_acceleration = false;
                capabilities.ground_reverse = false;
            }
            Self::GroundReverse => capabilities.ground_reverse = false,
            Self::NoSpread => capabilities.no_spread = false,
            Self::NoRecoil => capabilities.no_recoil = false,
            Self::BuyTime => capabilities.buy_time = false,
            Self::BuyLimit => capabilities.buy_limit = false,
            Self::BuyAccess => capabilities.buy_access = false,
            Self::BuyAnywhere => capabilities.buy_anywhere = false,
            Self::Money => capabilities.money = false,
            Self::FriendlyFireScale => capabilities.friendly_fire_scale = false,
            Self::TeammatesAsEnemies => capabilities.teammates_as_enemies = false,
            Self::RandomSpawn => capabilities.random_spawn = false,
            Self::HideRadar => capabilities.hide_radar = false,
            Self::HidePlayerIdentification => capabilities.hide_player_identification = false,
            Self::DropWeapons => capabilities.drop_weapons = false,
            Self::DropGrenades => capabilities.drop_grenades = false,
            Self::ShootDroppedGrenades => capabilities.shoot_dropped_grenades = false,
            Self::FallDamageScale => capabilities.fall_damage_scale = false,
            Self::RecoilView => capabilities.recoil_view = false,
            Self::Color => capabilities.color = false,
            Self::SingleShot => capabilities.single_shot = false,
            Self::FastSwitch => capabilities.fast_switch = false,
            Self::GrenadeRefill => {
                capabilities.grenade_replenishment = false;
            }
            Self::KillLoadout => capabilities.kill_loadout = false,
            Self::SelfDamage => capabilities.self_damage = false,
            Self::NativeSelfDamage => capabilities.native_self_damage = false,
            Self::EnergyShield => capabilities.energy_shield = false,
            Self::Translucent => capabilities.translucent = false,
            Self::GhostVisibility => capabilities.ghost_visibility = false,
            Self::GhostBreath => capabilities.ghost_breath = false,
            Self::DelayedDamage => capabilities.delayed_damage = false,
            Self::Reflection => capabilities.reflect_damage = false,
            Self::ReflectionLifestealFrame => capabilities.reflection_lifesteal_frame = false,
            Self::TeamDeathmatch | Self::WinCondition => capabilities.win_condition = false,
            Self::CtBombNoDefuse => capabilities.ct_bomb_no_defuse = false,
            Self::BombTiming => capabilities.bomb_timing = false,
            Self::ChickenBomb => capabilities.chicken_bomb = false,
            Self::CarryBomb => capabilities.carry_bomb = false,
            Self::MovingBlast => {
                capabilities.chicken_bomb = false;
                capabilities.carry_bomb = false;
            }
            Self::ProjectileDamage => {
                capabilities.dodgeball = false;
                capabilities.throwing_knives = false;
            }
            Self::Dodgeball => capabilities.dodgeball = false,
            Self::ThrowingKnives => capabilities.throwing_knives = false,
            Self::ViewPunchDecay => capabilities.view_punch_decay = false,
            Self::NoScope => capabilities.no_scope = false,
            Self::Fov => capabilities.fov = false,
            Self::ThirdPerson => capabilities.third_person = false,
            Self::Crosshair => capabilities.hide_crosshair = false,
            Self::PeriodicMovement => {
                capabilities.periodic_speed = false;
                capabilities.periodic_gravity = false;
            }
            Self::PeriodicSpeed => capabilities.periodic_speed = false,
            Self::PeriodicGravity => capabilities.periodic_gravity = false,
            Self::NoC4 | Self::C4Assignment => capabilities.c4_assignment = false,
            Self::Classic => capabilities.speed = false,
            Self::HealthSpeed => capabilities.health_speed = false,
            Self::HealthDecay => capabilities.health_decay = false,
            Self::RandomDamage => capabilities.random_damage = false,
            Self::Suicide => capabilities.suicide = false,
            Self::Healthshot => capabilities.healthshot = false,
            Self::ChickenFollowBlocking => capabilities.ordinary_chickens = false,
            Self::ChickenEating => {}
            Self::Afterburn => capabilities.afterburn = false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Cancelled,
    Mechanism(Mechanism),
    Global,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub scope: Scope,
    message: String,
}

impl Fault {
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self {
            scope: Scope::Cancelled,
            message: message.into(),
        }
    }

    pub fn mechanism(mechanism: Mechanism, message: impl Into<String>) -> Self {
        Self {
            scope: Scope::Mechanism(mechanism),
            message: message.into(),
        }
    }

    pub fn global(message: impl Into<String>) -> Self {
        Self {
            scope: Scope::Global,
            message: message.into(),
        }
    }

    pub fn context(mut self, context: impl fmt::Display) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl From<String> for Fault {
    fn from(message: String) -> Self {
        Self::global(message)
    }
}

impl From<&str> for Fault {
    fn from(message: &str) -> Self {
        Self::global(message)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    Ready,
    Recovering,
    Stopped,
}

pub struct Faults {
    available: Capabilities,
    pub recovery: Recovery,
    reasons: Vec<(Scope, String)>,
}

impl Faults {
    pub fn new(available: Capabilities) -> Self {
        Self {
            available,
            recovery: Recovery::Ready,
            reasons: Vec::new(),
        }
    }

    pub fn record(&mut self, fault: &Fault) -> bool {
        match fault.scope {
            Scope::Cancelled => return false,
            Scope::Mechanism(mechanism) => {
                mechanism.exclude(&mut self.available);
                if self.recovery != Recovery::Stopped {
                    self.recovery = Recovery::Recovering;
                }
            }
            Scope::Global => self.recovery = Recovery::Stopped,
        }
        let reason = (fault.scope, fault.to_string());
        if self.reasons.contains(&reason) {
            return false;
        }
        self.reasons.push(reason);
        true
    }

    /// Excludes one optional capability without putting unrelated gameplay into recovery.
    pub fn exclude(&mut self, mechanism: Mechanism, message: impl Into<String>) -> bool {
        mechanism.exclude(&mut self.available);
        let reason = (Scope::Mechanism(mechanism), message.into());
        if self.reasons.contains(&reason) {
            return false;
        }
        self.reasons.push(reason);
        true
    }

    pub fn capabilities(&self) -> Capabilities {
        if self.recovery == Recovery::Stopped {
            Capabilities::default()
        } else {
            self.available
        }
    }

    pub fn finish_cleanup(&mut self, complete: bool) -> bool {
        if self.recovery != Recovery::Recovering || !complete {
            return false;
        }
        self.recovery = Recovery::Ready;
        true
    }

    pub fn reasons(&self) -> String {
        self.reasons
            .iter()
            .map(|(scope, reason)| format!("{scope:?}:[{reason}]"))
            .collect::<Vec<_>>()
            .join(";")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use funrounds_core::{Config, NextMode, Phase, RoundController, TeamFilter};
    use rand::{SeedableRng, rngs::StdRng};

    fn available() -> Capabilities {
        Capabilities {
            ammo: true,
            single_shot: true,
            equipment: true,
            health: true,
            armor: true,
            gravity: true,
            speed: true,
            helmet: true,
            clear_weapons: true,
            give_weapons: true,
            give_knife: true,
            grenade_replenishment: true,
            bomb_timing: true,
            chicken_bomb: true,
            carry_bomb: true,
            bomb_bounce: true,
            dodgeball: true,
            throwing_knives: true,
            win_condition: true,
            ct_bomb_no_defuse: true,
            ..Capabilities::default()
        }
    }

    #[test]
    fn mechanism_failures_disable_only_their_capabilities() {
        let original = available();
        for (mechanism, expected) in [
            (
                Mechanism::Ammo,
                Capabilities {
                    ammo: false,
                    single_shot: false,
                    ..original
                },
            ),
            (
                Mechanism::GrenadeRefill,
                Capabilities {
                    grenade_replenishment: false,
                    ..original
                },
            ),
            (
                Mechanism::BombTiming,
                Capabilities {
                    bomb_timing: false,
                    ..original
                },
            ),
            (
                Mechanism::MovingBlast,
                Capabilities {
                    chicken_bomb: false,
                    carry_bomb: false,
                    ..original
                },
            ),
            (
                Mechanism::ProjectileDamage,
                Capabilities {
                    dodgeball: false,
                    throwing_knives: false,
                    ..original
                },
            ),
            (
                Mechanism::ThrowingKnives,
                Capabilities {
                    throwing_knives: false,
                    ..original
                },
            ),
        ] {
            let mut faults = Faults::new(original);
            let fault = Fault::mechanism(mechanism, "native readback mismatch");
            assert!(faults.record(&fault));
            assert!(!faults.record(&fault));
            assert_eq!(faults.capabilities(), expected);
            assert_eq!(faults.recovery, Recovery::Recovering);
            assert!(!faults.finish_cleanup(false));
            assert!(faults.finish_cleanup(true));
            assert_eq!(faults.recovery, Recovery::Ready);
            assert_eq!(faults.capabilities(), expected);
            assert_eq!(faults.reasons.len(), 1);
        }
    }

    #[test]
    fn cancellation_does_not_quarantine_or_interrupt_a_healthy_round() {
        let original = available();
        let mut faults = Faults::new(original);
        for message in [
            "new life on the same pawn",
            "new connection in the same slot",
            "original item no longer owned",
        ] {
            assert!(!faults.record(&Fault::cancelled(message)));
        }
        assert_eq!(faults.capabilities(), original);
        assert_eq!(faults.recovery, Recovery::Ready);
        assert!(faults.reasons.is_empty());
    }

    #[test]
    fn restore_failure_stays_stopped_after_successful_cleanup_and_later_local_faults() {
        let mut faults = Faults::new(available());
        faults.record(&Fault::mechanism(Mechanism::ThrowingKnives, "knife creation"));
        faults.record(&Fault::global("mp_buytime restoration mismatch"));
        assert_eq!(faults.capabilities(), Capabilities::default());
        assert!(!faults.finish_cleanup(false));
        assert!(!faults.finish_cleanup(true));
        faults.record(&Fault::mechanism(Mechanism::Ammo, "ammo mismatch"));
        assert_eq!(faults.recovery, Recovery::Stopped);
        assert_eq!(faults.capabilities(), Capabilities::default());
        assert!(faults.reasons().contains("mp_buytime restoration mismatch"));
    }

    #[test]
    fn pending_recovery_consumes_the_round_key_and_reload_enable_map_reset_keep_quarantine() {
        let source = include_str!("../../../config/funrounds.example.toml");
        let mut config = Config::parse(source).unwrap();
        let mut feature_mode = config.modes[0].clone();
        config.modes[0].id = "basic".into();
        config.modes[0].name = "Basic".into();
        feature_mode.id = "feature".into();
        feature_mode.name = "Feature".into();
        feature_mode.throwing_knives = true;
        feature_mode.give_knife = TeamFilter::All;
        config.modes.push(feature_mode);
        let mut faults = Faults::new(available());
        let mut controller = RoundController::new(Some(config), faults.capabilities()).unwrap();
        let mut rng = StdRng::seed_from_u64(7);
        controller.force_next(NextMode::Id("feature".into())).unwrap();
        let first = controller.round_key(1);
        assert!(controller.is_new_round(first));
        assert!(controller.start_round(first, false, &mut rng));
        assert!(controller.freeze_end(first));
        let previous_generation = controller.generation();
        faults.record(&Fault::mechanism(Mechanism::ThrowingKnives, "knife Touch unavailable"));
        controller.set_capabilities(faults.capabilities());
        assert_ne!(controller.generation(), previous_generation);
        assert_eq!(controller.phase(), Phase::Idle);
        let blocked = controller.round_key(2);
        assert!(controller.start_round(blocked, faults.recovery != Recovery::Ready, &mut rng));
        assert!(!faults.finish_cleanup(false));
        assert!(faults.finish_cleanup(true));
        assert!(!controller.is_new_round(blocked));
        assert!(!controller.start_round(blocked, false, &mut rng));
        assert!(!controller.is_new_round(first));
        assert_eq!(controller.phase(), Phase::Idle);
        let (header, body) = source.split_once("[[modes]]").unwrap();
        let base = format!("[[modes]]{body}");
        let basic_source = base
            .replace("id = \"example\"", "id = \"basic\"")
            .replace("name_key = \"mode.example.name\"", "name = \"Basic\"");
        let feature_source = base
            .replace("id = \"example\"", "id = \"feature\"")
            .replace("name_key = \"mode.example.name\"", "name = \"Feature\"")
            .replace(
                "weapons = [\"weapon_famas\", \"weapon_p250\"]",
                "weapons = [\"weapon_famas\", \"weapon_p250\"]\ngive_knife = \"all\"\nthrowing_knives = true",
            );
        let reloaded_source = format!("{header}\n{basic_source}\n{feature_source}");
        controller.queue_reload(&reloaded_source).unwrap();
        controller.set_enabled(false);
        controller.set_enabled(true);
        assert!(controller.pending_config().unwrap().modes.iter().any(|mode| mode.id == "feature"));
        assert!(controller.force_next(NextMode::Id("feature".into())).is_err());
        controller.reset();
        assert_eq!(controller.capabilities(), faults.capabilities());
        controller.force_next(NextMode::Id("basic".into())).unwrap();
        let next = controller.round_key(3);
        assert!(controller.start_round(next, false, &mut rng));
        assert_eq!(controller.current_mode().unwrap().id, "basic");
        assert!(controller.freeze_end(next));
        assert_eq!(controller.phase(), Phase::Running);
    }
}
