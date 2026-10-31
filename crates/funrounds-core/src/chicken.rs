// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::PlayerTask;
use rand::{Rng, RngExt};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChickenHealthBasis {
    Current,
    Base,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChickenPercentRange {
    pub min: f32,
    pub max: f32,
}

impl ChickenPercentRange {
    fn validate(self, name: &str) -> Result<(), String> {
        if !self.min.is_finite() || !self.max.is_finite() || self.min < 0.0 || self.max < self.min {
            return Err(format!("{name} must be a finite, non-negative increasing range"));
        }
        Ok(())
    }

    fn sample(self, rng: &mut impl Rng) -> f32 {
        if self.min == self.max {
            self.min
        } else {
            rng.random_range(self.min..=self.max)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChickenSpawnConfig {
    pub per_player: u32,
    pub minimum: u32,
}

impl Default for ChickenSpawnConfig {
    fn default() -> Self {
        Self { per_player: 2, minimum: 10 }
    }
}

impl ChickenSpawnConfig {
    /// `effective_players` is counted by the adapter from eligible players, not slots or observers.
    pub fn target_count(self, effective_players: u32) -> u32 {
        effective_players.saturating_mul(self.per_player).max(self.minimum)
    }

    /// Replenish by a full batch only below ceil(1.5 × target), preserving an
    /// unchanged population at the exact threshold.
    pub fn refill_threshold(self, target: u32) -> u32 {
        target.saturating_add(target / 2 + target % 2)
    }

    /// Populations above three batches are trimmed back to this limit.
    pub fn maximum_population(self, target: u32) -> u32 {
        target.saturating_mul(3)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChickenHealConfig {
    pub basis: ChickenHealthBasis,
    pub percent: ChickenPercentRange,
    pub minimum_health: i32,
}

impl Default for ChickenHealConfig {
    fn default() -> Self {
        Self {
            basis: ChickenHealthBasis::Current,
            percent: ChickenPercentRange { min: 0.70, max: 2.75 },
            minimum_health: 25,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChickenDamageRule {
    Percent {
        basis: ChickenHealthBasis,
        percent: ChickenPercentRange,
    },
    Fixed {
        amount: u32,
    },
}

impl Default for ChickenDamageRule {
    fn default() -> Self {
        Self::Percent {
            basis: ChickenHealthBasis::Current,
            percent: ChickenPercentRange { min: 0.10, max: 0.25 },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChickenEatingConfig {
    pub enabled: bool,
    pub digestion_seconds: f32,
    pub risk_offset: i32,
    pub risk_per_undigested: u32,
    pub heal: ChickenHealConfig,
    pub damage: ChickenDamageRule,
}

impl Default for ChickenEatingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            digestion_seconds: 20.0,
            risk_offset: 0,
            risk_per_undigested: 7,
            heal: ChickenHealConfig::default(),
            damage: ChickenDamageRule::default(),
        }
    }
}

impl ChickenEatingConfig {
    /// The global setting can disable eating; a mode can additionally disable it, never re-enable it.
    pub const fn enabled_for_mode(self, mode_enabled: Option<bool>) -> bool {
        self.enabled && !matches!(mode_enabled, Some(false))
    }

    fn validate(self) -> Result<(), String> {
        if !self.digestion_seconds.is_finite() || self.digestion_seconds <= 0.0 {
            return Err("chicken.eating.digestion_seconds must be finite and positive".into());
        }
        self.heal.percent.validate("chicken.eating.heal percent")?;
        if self.heal.minimum_health < 0 {
            return Err("chicken.eating.heal.minimum_health must be non-negative".into());
        }
        if let ChickenDamageRule::Percent { percent, .. } = self.damage {
            percent.validate("chicken.eating.damage percent")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChickenBlastConfig {
    pub damage: f32,
    pub radius: f32,
}

impl Default for ChickenBlastConfig {
    fn default() -> Self {
        Self {
            damage: 2000.0,
            radius: 1000.0,
        }
    }
}

impl ChickenBlastConfig {
    fn validate(self, name: &str) -> Result<(), String> {
        if !self.damage.is_finite() || self.damage < 0.0 || !self.radius.is_finite() || self.radius < 0.0 {
            return Err(format!("{name}.damage and radius must be finite and non-negative"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChickenDamageChainConfig {
    pub allow_fire: bool,
    pub allow_explosion: bool,
    pub allow_chicken_explosion: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChickenConfig {
    pub spawn: ChickenSpawnConfig,
    pub eating: ChickenEatingConfig,
    pub kill_blast: ChickenBlastConfig,
    pub risk_blast: ChickenBlastConfig,
    pub damage_chain: ChickenDamageChainConfig,
}

impl ChickenConfig {
    pub fn validate(self) -> Result<(), String> {
        self.eating.validate()?;
        self.kill_blast.validate("chicken.kill_blast")?;
        self.risk_blast.validate("chicken.risk_blast")?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct HealthshotConfig {
    pub heal_base_fraction: f32,
    pub cap_base_multiple: u32,
}

impl Default for HealthshotConfig {
    fn default() -> Self {
        Self {
            heal_base_fraction: 0.5,
            cap_base_multiple: 3,
        }
    }
}

impl HealthshotConfig {
    pub fn validate(self) -> Result<(), String> {
        if !self.heal_base_fraction.is_finite() || self.heal_base_fraction < 0.0 || self.cap_base_multiple == 0 {
            return Err("healthshot.heal_base_fraction must be finite and non-negative and cap_base_multiple must be positive".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthshotOutcome {
    NotConsumed,
    Applied { health: i32, healed: i32 },
}

/// Returns NotConsumed at or above the B-relative cap, and never lowers health above that cap.
pub fn use_healthshot(config: HealthshotConfig, current_health: i32, base_health: i32) -> HealthshotOutcome {
    if config.validate().is_err() || current_health <= 0 || base_health <= 0 {
        return HealthshotOutcome::NotConsumed;
    }
    let cap = i64::from(base_health) * i64::from(config.cap_base_multiple);
    if i64::from(current_health) >= cap {
        return HealthshotOutcome::NotConsumed;
    }
    // Engine health is integer-valued; retain the project's floor rounding for healing amounts.
    let amount = (f64::from(base_health) * f64::from(config.heal_base_fraction)).floor() as i64;
    let health = i64::from(current_health).saturating_add(amount).min(cap).min(i64::from(i32::MAX)) as i32;
    if health > current_health {
        HealthshotOutcome::Applied {
            health,
            healed: health - current_health,
        }
    } else {
        HealthshotOutcome::NotConsumed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChickenDamageCause {
    Fire,
    Explosion,
    ChickenExplosion,
    Other,
}

/// Reports whether a damage source is blocked from creating a chicken blast chain.
pub const fn filters_chicken_damage_chain(config: ChickenDamageChainConfig, cause: ChickenDamageCause) -> bool {
    match cause {
        ChickenDamageCause::Fire => !config.allow_fire,
        ChickenDamageCause::Explosion => !config.allow_explosion,
        ChickenDamageCause::ChickenExplosion => !config.allow_chicken_explosion,
        ChickenDamageCause::Other => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChickenEatOutcome {
    Heal { health: i32, healed: i32 },
    Damage { amount: i32 },
    Death,
    RiskBlast(ChickenBlastConfig),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChickenEatError {
    Disabled,
    StaleTask,
    InvalidHealth,
    InvalidTime,
    InvalidConfig,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChickenEatingState {
    task: PlayerTask,
    undigested: u32,
    next_digestion_at: Option<f64>,
}

impl ChickenEatingState {
    pub const fn new(task: PlayerTask) -> Self {
        Self {
            task,
            undigested: 0,
            next_digestion_at: None,
        }
    }

    /// Rebind this life-bound stomach state after a confirmed controller takeover of the same pawn.
    /// The adapter must resolve and validate the new controller against the live engine pawn first.
    pub fn rebind_takeover(&mut self, new_task: PlayerTask) -> bool {
        let old_pawn = self.task.player.pawn_handle;
        if self.task.round != new_task.round || old_pawn == u32::MAX || new_task.player.pawn_handle != old_pawn {
            return false;
        }
        self.task = new_task;
        true
    }

    pub const fn undigested(&self) -> u32 {
        self.undigested
    }

    pub const fn next_digestion_at(&self) -> Option<f64> {
        self.next_digestion_at
    }

    /// Clear this life's stomach state when chicken use is disabled or the player dies.
    pub fn reset(&mut self) {
        self.undigested = 0;
        self.next_digestion_at = None;
    }

    /// Digests every chicken whose fixed 20-second cadence (or configured interval) elapsed.
    /// The deadline advances from the old deadline, not from the latest feed time.
    pub fn digest_due(&mut self, task: PlayerTask, now: f64, interval_seconds: f32) -> Result<u32, ChickenEatError> {
        if task != self.task {
            return Err(ChickenEatError::StaleTask);
        }
        if !now.is_finite() || now < 0.0 {
            return Err(ChickenEatError::InvalidTime);
        }
        if !interval_seconds.is_finite() || interval_seconds <= 0.0 {
            return Err(ChickenEatError::InvalidConfig);
        }
        if self.undigested == 0 {
            self.next_digestion_at = None;
            return Ok(0);
        }
        let Some(next_digestion_at) = self.next_digestion_at else {
            return Ok(0);
        };
        if now < next_digestion_at {
            return Ok(0);
        }
        let interval = f64::from(interval_seconds);
        let elapsed_intervals = (((now - next_digestion_at) / interval).floor() as u64).saturating_add(1);
        let digested = elapsed_intervals.min(u64::from(self.undigested)) as u32;
        self.undigested -= digested;
        self.next_digestion_at = (self.undigested > 0).then_some(next_digestion_at + f64::from(digested) * interval);
        Ok(digested)
    }

    /// Applies one accepted chicken interaction; the caller supplies the same round/life task
    /// used to create this state and the actual spawn-health base captured by the adapter.
    pub fn consume(
        &mut self,
        task: PlayerTask,
        current_health: i32,
        base_health: i32,
        now: f64,
        config: ChickenConfig,
        rng: &mut impl Rng,
    ) -> Result<ChickenEatOutcome, ChickenEatError> {
        if task != self.task {
            return Err(ChickenEatError::StaleTask);
        }
        if !config.eating.enabled {
            return Err(ChickenEatError::Disabled);
        }
        if config.validate().is_err() {
            return Err(ChickenEatError::InvalidConfig);
        }
        if current_health <= 0 || base_health <= 0 {
            return Err(ChickenEatError::InvalidHealth);
        }
        if !now.is_finite() || now < 0.0 {
            return Err(ChickenEatError::InvalidTime);
        }
        self.digest_due(task, now, config.eating.digestion_seconds)?;

        let risk = if self.undigested == 0 {
            ChickenRisk::Heal
        } else {
            classify_chicken_risk(
                rng.random_range(1..=100),
                self.undigested,
                config.eating.risk_per_undigested,
                config.eating.risk_offset,
            )
        };
        Ok(self.finish_consumption(risk, current_health, base_health, now, config, rng))
    }

    fn finish_consumption(
        &mut self,
        risk: ChickenRisk,
        current_health: i32,
        base_health: i32,
        now: f64,
        config: ChickenConfig,
        rng: &mut impl Rng,
    ) -> ChickenEatOutcome {
        let eaten_count = self.undigested.saturating_add(1);
        self.undigested = eaten_count;
        if self.next_digestion_at.is_none() {
            self.next_digestion_at = Some(now + f64::from(config.eating.digestion_seconds));
        }
        match risk {
            ChickenRisk::Heal => {
                let basis = match config.eating.heal.basis {
                    ChickenHealthBasis::Current => current_health,
                    ChickenHealthBasis::Base => base_health,
                };
                let percent = config.eating.heal.percent.sample(rng);
                let bonus = (basis as f32 * percent / eaten_count as f32).floor() as i32;
                let mut health = current_health.saturating_add(bonus);
                if current_health < base_health && health < base_health {
                    health = base_health;
                }
                if current_health < config.eating.heal.minimum_health && health < config.eating.heal.minimum_health {
                    health = config.eating.heal.minimum_health;
                }
                ChickenEatOutcome::Heal {
                    health,
                    healed: health.saturating_sub(current_health),
                }
            }
            ChickenRisk::Damage => ChickenEatOutcome::Damage {
                amount: chicken_damage_amount(config.eating.damage, current_health, base_health, eaten_count, rng),
            },
            ChickenRisk::Death => ChickenEatOutcome::Death,
            ChickenRisk::Blast => ChickenEatOutcome::RiskBlast(config.risk_blast),
        }
    }
}

fn chicken_damage_amount(rule: ChickenDamageRule, current_health: i32, base_health: i32, eaten_count: u32, rng: &mut impl Rng) -> i32 {
    match rule {
        ChickenDamageRule::Percent { basis, percent } => {
            let basis = match basis {
                ChickenHealthBasis::Current => current_health,
                ChickenHealthBasis::Base => base_health,
            };
            let per_chicken = (basis as f32 * percent.sample(rng)).floor() as i32;
            per_chicken.saturating_mul(eaten_count.min(i32::MAX as u32) as i32).max(1)
        }
        ChickenDamageRule::Fixed { amount } => i32::try_from(amount).unwrap_or(i32::MAX),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChickenRisk {
    Heal,
    Damage,
    Death,
    Blast,
}

fn classify_chicken_risk(roll: u32, undigested: u32, per_undigested: u32, offset: i32) -> ChickenRisk {
    let risk = i64::from(roll)
        .saturating_add(i64::from(undigested).saturating_mul(i64::from(per_undigested)))
        .saturating_add(i64::from(offset));
    match risk {
        ..=50 => ChickenRisk::Heal,
        51..=85 => ChickenRisk::Damage,
        86..=94 => ChickenRisk::Death,
        _ => ChickenRisk::Blast,
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ChickenInput {
    spawn: Option<ChickenSpawnInput>,
    eating: Option<ChickenEatingInput>,
    kill_blast: Option<ChickenBlastInput>,
    risk_blast: Option<ChickenBlastInput>,
    damage_chain: Option<ChickenDamageChainInput>,
}

impl ChickenInput {
    pub(crate) fn resolve(self) -> Result<ChickenConfig, String> {
        let defaults = ChickenConfig::default();
        let config = ChickenConfig {
            spawn: self.spawn.map_or(defaults.spawn, |input| input.resolve(defaults.spawn)),
            eating: self.eating.map_or(Ok(defaults.eating), |input| input.resolve(defaults.eating))?,
            kill_blast: self.kill_blast.map_or(defaults.kill_blast, |input| input.resolve(defaults.kill_blast)),
            risk_blast: self.risk_blast.map_or(defaults.risk_blast, |input| input.resolve(defaults.risk_blast)),
            damage_chain: self
                .damage_chain
                .map_or(defaults.damage_chain, |input| input.resolve(defaults.damage_chain)),
        };
        config.validate()?;
        Ok(config)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ChickenDamageChainInput {
    allow_fire: Option<bool>,
    allow_explosion: Option<bool>,
    allow_chicken_explosion: Option<bool>,
}

impl ChickenDamageChainInput {
    fn resolve(self, defaults: ChickenDamageChainConfig) -> ChickenDamageChainConfig {
        ChickenDamageChainConfig {
            allow_fire: self.allow_fire.unwrap_or(defaults.allow_fire),
            allow_explosion: self.allow_explosion.unwrap_or(defaults.allow_explosion),
            allow_chicken_explosion: self.allow_chicken_explosion.unwrap_or(defaults.allow_chicken_explosion),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ChickenSpawnInput {
    per_player: Option<u32>,
    minimum: Option<u32>,
}

impl ChickenSpawnInput {
    fn resolve(self, defaults: ChickenSpawnConfig) -> ChickenSpawnConfig {
        ChickenSpawnConfig {
            per_player: self.per_player.unwrap_or(defaults.per_player),
            minimum: self.minimum.unwrap_or(defaults.minimum),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ChickenEatingInput {
    enabled: Option<bool>,
    digestion_seconds: Option<f32>,
    risk_offset: Option<i32>,
    risk_per_undigested: Option<u32>,
    heal: Option<ChickenHealInput>,
    damage: Option<ChickenDamageInput>,
}

impl ChickenEatingInput {
    fn resolve(self, defaults: ChickenEatingConfig) -> Result<ChickenEatingConfig, String> {
        let config = ChickenEatingConfig {
            enabled: self.enabled.unwrap_or(defaults.enabled),
            digestion_seconds: self.digestion_seconds.unwrap_or(defaults.digestion_seconds),
            risk_offset: self.risk_offset.unwrap_or(defaults.risk_offset),
            risk_per_undigested: self.risk_per_undigested.unwrap_or(defaults.risk_per_undigested),
            heal: self.heal.map_or(defaults.heal, |input| input.resolve(defaults.heal)),
            damage: self.damage.map_or(Ok(defaults.damage), |input| input.resolve(defaults.damage))?,
        };
        config.validate()?;
        Ok(config)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ChickenHealInput {
    basis: Option<ChickenHealthBasis>,
    min: Option<f32>,
    max: Option<f32>,
    minimum_health: Option<i32>,
}

impl ChickenHealInput {
    fn resolve(self, defaults: ChickenHealConfig) -> ChickenHealConfig {
        ChickenHealConfig {
            basis: self.basis.unwrap_or(defaults.basis),
            percent: ChickenPercentRange {
                min: self.min.unwrap_or(defaults.percent.min),
                max: self.max.unwrap_or(defaults.percent.max),
            },
            minimum_health: self.minimum_health.unwrap_or(defaults.minimum_health),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ChickenDamageInput {
    Percent {
        basis: Option<ChickenHealthBasis>,
        min: Option<f32>,
        max: Option<f32>,
    },
    Fixed {
        amount: Option<u32>,
    },
}

impl ChickenDamageInput {
    fn resolve(self, defaults: ChickenDamageRule) -> Result<ChickenDamageRule, String> {
        match self {
            Self::Percent { basis, min, max } => {
                let base = match defaults {
                    ChickenDamageRule::Percent { basis, percent } => (basis, percent),
                    ChickenDamageRule::Fixed { .. } => match ChickenDamageRule::default() {
                        ChickenDamageRule::Percent { basis, percent } => (basis, percent),
                        ChickenDamageRule::Fixed { .. } => unreachable!(),
                    },
                };
                Ok(ChickenDamageRule::Percent {
                    basis: basis.unwrap_or(base.0),
                    percent: ChickenPercentRange {
                        min: min.unwrap_or(base.1.min),
                        max: max.unwrap_or(base.1.max),
                    },
                })
            }
            Self::Fixed { amount } => match (amount, defaults) {
                (Some(amount), _) => Ok(ChickenDamageRule::Fixed { amount }),
                (None, ChickenDamageRule::Fixed { amount }) => Ok(ChickenDamageRule::Fixed { amount }),
                (None, ChickenDamageRule::Percent { .. }) => Err("chicken.eating.damage.amount is required for fixed damage".into()),
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ChickenBlastInput {
    damage: Option<f32>,
    radius: Option<f32>,
}

impl ChickenBlastInput {
    fn resolve(self, defaults: ChickenBlastConfig) -> ChickenBlastConfig {
        ChickenBlastConfig {
            damage: self.damage.unwrap_or(defaults.damage),
            radius: self.radius.unwrap_or(defaults.radius),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HealthshotInput {
    heal_base_fraction: Option<f32>,
    cap_base_multiple: Option<u32>,
}

impl HealthshotInput {
    pub(crate) fn resolve(self) -> Result<HealthshotConfig, String> {
        let defaults = HealthshotConfig::default();
        let config = HealthshotConfig {
            heal_base_fraction: self.heal_base_fraction.unwrap_or(defaults.heal_base_fraction),
            cap_base_multiple: self.cap_base_multiple.unwrap_or(defaults.cap_base_multiple),
        };
        config.validate()?;
        Ok(config)
    }
}
