// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AfterburnSettings {
    pub duration_seconds: f32,
    pub damage_fraction: f32,
    pub minimum_damage: f32,
}

impl Default for AfterburnSettings {
    fn default() -> Self {
        Self {
            duration_seconds: 15.0,
            damage_fraction: 0.01,
            minimum_damage: 1.0,
        }
    }
}

impl AfterburnSettings {
    pub(crate) fn validate(self) -> Result<(), String> {
        if !self.duration_seconds.is_finite() || self.duration_seconds <= 0.0 {
            return Err("afterburn duration_seconds must be finite and positive".into());
        }
        if !self.damage_fraction.is_finite() || self.damage_fraction < 0.0 {
            return Err("afterburn damage_fraction must be finite and non-negative".into());
        }
        if !self.minimum_damage.is_finite() || self.minimum_damage < 0.0 {
            return Err("afterburn minimum_damage must be finite and non-negative".into());
        }
        Ok(())
    }

    /// Only the captured, plugin-owned entityflame uses this rule; inferno ticks stay native.
    pub fn tick_damage(self, current_health: i32) -> f32 {
        if current_health <= 0 {
            return 0.0;
        }
        (f64::from(current_health) * f64::from(self.damage_fraction))
            .max(f64::from(self.minimum_damage))
            .min(f64::from(f32::MAX)) as f32
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AfterburnInput {
    duration_seconds: Option<f32>,
    damage_fraction: Option<f32>,
    minimum_damage: Option<f32>,
}

impl AfterburnInput {
    pub(crate) fn resolve(self, base: AfterburnSettings) -> Result<AfterburnSettings, String> {
        let settings = AfterburnSettings {
            duration_seconds: self.duration_seconds.unwrap_or(base.duration_seconds),
            damage_fraction: self.damage_fraction.unwrap_or(base.damage_fraction),
            minimum_damage: self.minimum_damage.unwrap_or(base.minimum_damage),
        };
        settings.validate()?;
        Ok(settings)
    }
}
