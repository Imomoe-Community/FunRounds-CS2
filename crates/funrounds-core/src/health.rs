// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HealthBand {
    pub below_percent: u32,
    pub damage: f32,
    pub speed: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HealthCurve {
    pub bands: Vec<HealthBand>,
    pub growth_step_percent: u32,
    pub high_damage: f32,
    pub high_speed: f32,
    pub damage_increment: f32,
    pub speed_increment: f32,
    pub damage_cap: f32,
    pub speed_cap: f32,
}

impl Default for HealthCurve {
    fn default() -> Self {
        Self {
            bands: vec![
                HealthBand {
                    below_percent: 10,
                    damage: 0.6925,
                    speed: 0.385,
                },
                HealthBand {
                    below_percent: 55,
                    damage: 0.86,
                    speed: 0.68,
                },
                HealthBand {
                    below_percent: 133,
                    damage: 1.0,
                    speed: 1.0,
                },
            ],
            growth_step_percent: 33,
            high_damage: 1.0,
            high_speed: 1.0,
            damage_increment: 0.02,
            speed_increment: 0.01,
            damage_cap: 6.0,
            speed_cap: 5.0,
        }
    }
}

impl HealthCurve {
    pub fn damage_scale(&self, attacker_health: Option<(i32, u16)>, self_damage: bool, fall: bool, self_exception: bool) -> Option<f32> {
        if fall {
            return Some(1.0);
        }
        if self_damage {
            return self_exception.then_some(1.0);
        }
        Some(attacker_health.map_or(1.0, |(health, base)| self.scaling(health, base).0))
    }

    pub fn validate(&self) -> Result<(), String> {
        let positive = |value: f32| value.is_finite() && value > 0.0;
        if self.bands.is_empty()
            || self
                .bands
                .iter()
                .any(|band| band.below_percent == 0 || !positive(band.damage) || !positive(band.speed))
            || self.bands.windows(2).any(|pair| pair[0].below_percent >= pair[1].below_percent)
            || self.growth_step_percent == 0
            || !positive(self.high_damage)
            || !positive(self.high_speed)
            || !positive(self.damage_cap)
            || !positive(self.speed_cap)
            || self.damage_cap < self.high_damage
            || self.speed_cap < self.high_speed
            || !self.damage_increment.is_finite()
            || self.damage_increment < 0.0
            || !self.speed_increment.is_finite()
            || self.speed_increment < 0.0
        {
            return Err("health_curve needs increasing positive percentage bands, positive finite multipliers/caps and growth step, and non-negative finite increments".into());
        }
        Ok(())
    }

    pub fn scaling(&self, health: i32, spawn_health: u16) -> (f32, f32) {
        assert!(spawn_health > 0, "spawn health must be validated before computing scaling");
        let numerator = i64::from(health.max(0)) * 100;
        let base = i64::from(spawn_health);
        for band in &self.bands {
            if numerator < base * i64::from(band.below_percent) {
                return (band.damage, band.speed);
            }
        }
        let start = i64::from(self.bands.last().expect("validated health curve has bands").below_percent);
        let step = i64::from(self.growth_step_percent);
        // Keep the legacy integer percentage buckets, including its initial high-health plateau.
        let growth = (numerator / (base * step) - start / step) as f32;
        (
            (self.high_damage + growth * self.damage_increment).min(self.damage_cap),
            (self.high_speed + growth * self.speed_increment).min(self.speed_cap),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HealthDecay {
    pub interval: f32,
    pub percent: u8,
    pub floor_percent: u16,
}

impl Default for HealthDecay {
    fn default() -> Self {
        Self {
            interval: 2.0,
            percent: 5,
            floor_percent: 100,
        }
    }
}

impl HealthDecay {
    pub fn validate(self) -> Result<(), String> {
        if !self.interval.is_finite() || self.interval <= 0.0 || self.percent > 100 || self.floor_percent == 0 {
            return Err("health_decay_settings needs a positive finite interval, percent between 0 and 100, and a positive floor_percent".into());
        }
        Ok(())
    }

    pub fn decay(self, health: i32, spawn_health: u16) -> i32 {
        let floor = (i64::from(spawn_health) * i64::from(self.floor_percent) + 99) / 100;
        if i64::from(health) <= floor {
            return health;
        }
        ((i64::from(health) * i64::from(100 - self.percent) + 99) / 100).max(floor) as i32
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HealthCurveInput {
    bands: Option<Vec<HealthBand>>,
    growth_step_percent: Option<u32>,
    high_damage: Option<f32>,
    high_speed: Option<f32>,
    damage_increment: Option<f32>,
    speed_increment: Option<f32>,
    damage_cap: Option<f32>,
    speed_cap: Option<f32>,
}

impl HealthCurveInput {
    pub(crate) fn resolve(self, defaults: Option<&Self>) -> HealthCurve {
        let fallback = HealthCurve::default();
        HealthCurve {
            bands: self
                .bands
                .or_else(|| defaults.and_then(|input| input.bands.clone()))
                .unwrap_or(fallback.bands),
            growth_step_percent: self
                .growth_step_percent
                .or_else(|| defaults.and_then(|input| input.growth_step_percent))
                .unwrap_or(fallback.growth_step_percent),
            high_damage: self
                .high_damage
                .or_else(|| defaults.and_then(|input| input.high_damage))
                .unwrap_or(fallback.high_damage),
            high_speed: self
                .high_speed
                .or_else(|| defaults.and_then(|input| input.high_speed))
                .unwrap_or(fallback.high_speed),
            damage_increment: self
                .damage_increment
                .or_else(|| defaults.and_then(|input| input.damage_increment))
                .unwrap_or(fallback.damage_increment),
            speed_increment: self
                .speed_increment
                .or_else(|| defaults.and_then(|input| input.speed_increment))
                .unwrap_or(fallback.speed_increment),
            damage_cap: self
                .damage_cap
                .or_else(|| defaults.and_then(|input| input.damage_cap))
                .unwrap_or(fallback.damage_cap),
            speed_cap: self
                .speed_cap
                .or_else(|| defaults.and_then(|input| input.speed_cap))
                .unwrap_or(fallback.speed_cap),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HealthDecayInput {
    interval: Option<f32>,
    percent: Option<u8>,
    floor_percent: Option<u16>,
}

impl HealthDecayInput {
    pub(crate) fn resolve(self, defaults: Option<Self>) -> HealthDecay {
        let fallback = HealthDecay::default();
        HealthDecay {
            interval: self
                .interval
                .or_else(|| defaults.and_then(|input| input.interval))
                .unwrap_or(fallback.interval),
            percent: self
                .percent
                .or_else(|| defaults.and_then(|input| input.percent))
                .unwrap_or(fallback.percent),
            floor_percent: self
                .floor_percent
                .or_else(|| defaults.and_then(|input| input.floor_percent))
                .unwrap_or(fallback.floor_percent),
        }
    }
}
