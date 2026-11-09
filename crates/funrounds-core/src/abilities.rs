// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::AttributeValue;
use crate::GrenadeKind;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HealingAmount {
    Fixed(i32),
    Percent(f32),
}

impl HealingAmount {
    /// Converts configured healing to whole HP using the current life's spawn health.
    /// Percentage values use human-readable percentage points (e.g. 5.0 means 5%).
    pub fn amount(self, base_health: i32) -> Option<i32> {
        match self {
            Self::Fixed(amount) => (amount > 0).then_some(amount),
            Self::Percent(percent) if base_health > 0 && finite_positive(percent) => {
                let amount = (f64::from(base_health) * f64::from(percent) / 100.0).floor();
                (amount >= 1.0).then_some(amount.min(f64::from(i32::MAX)) as i32)
            }
            Self::Percent(_) => None,
        }
    }

    fn validate(self) -> Result<(), &'static str> {
        match self {
            Self::Fixed(amount) if amount > 0 => Ok(()),
            Self::Percent(percent) if finite_positive(percent) => Ok(()),
            Self::Fixed(_) => Err("heal must be a positive integer"),
            Self::Percent(_) => Err("heal_percent must be a positive finite percentage"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExplosiveBulletsSettings {
    pub cooldown_seconds: f32,
    pub damage: AttributeValue,
    pub radius: AttributeValue,
}

impl Default for ExplosiveBulletsSettings {
    fn default() -> Self {
        Self {
            cooldown_seconds: 1.0,
            damage: AttributeValue::Range(crate::AttributeRange { min: 75, max: 180 }),
            radius: AttributeValue::Range(crate::AttributeRange { min: 225, max: 450 }),
        }
    }
}

impl ExplosiveBulletsSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.cooldown_seconds.is_finite() || self.cooldown_seconds < 0.0 {
            return Err("explosive_bullets needs a finite nonnegative cooldown_seconds");
        }
        if !self.damage.validate(1, u16::MAX) || !self.radius.validate(1, u16::MAX) {
            return Err("explosive_bullets damage and radius must be positive fixed values or ordered ranges");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeGrenadeImpulseSettings {
    pub impulse: f32,
}

impl Default for HeGrenadeImpulseSettings {
    fn default() -> Self {
        Self { impulse: 300.0 }
    }
}

impl HeGrenadeImpulseSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.impulse.is_finite() || self.impulse < 0.0 {
            return Err("hegrenade_impulse impulse must be finite and nonnegative");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProximityFuseSettings {
    pub radius: f32,
    pub delay_seconds: f32,
    pub grenades: Vec<GrenadeKind>,
}

impl Default for ProximityFuseSettings {
    fn default() -> Self {
        Self {
            radius: 200.0,
            delay_seconds: 0.0,
            grenades: vec![
                GrenadeKind::He,
                GrenadeKind::Flash,
                GrenadeKind::Smoke,
                GrenadeKind::Molotov,
                GrenadeKind::Incendiary,
            ],
        }
    }
}

impl ProximityFuseSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !finite_positive(self.radius) {
            return Err("proximity_fuse radius must be positive and finite");
        }
        if !self.delay_seconds.is_finite() || self.delay_seconds < 0.0 {
            return Err("proximity_fuse delay_seconds must be finite and non-negative");
        }
        if self.grenades.is_empty() {
            return Err("proximity_fuse grenades must contain at least one supported grenade type");
        }
        if self.grenades.contains(&GrenadeKind::Decoy) {
            return Err("proximity_fuse does not support decoy grenades");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrappleSettings {
    pub max_distance: f32,
    pub max_speed: f32,
    pub pull_gain: f32,
}

impl Default for GrappleSettings {
    fn default() -> Self {
        Self {
            max_distance: 32768.0,
            max_speed: 800.0,
            pull_gain: 6.0,
        }
    }
}

impl GrappleSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !finite_positive(self.max_distance) || !finite_positive(self.max_speed) || !finite_positive(self.pull_gain) {
            return Err("grapple max_distance, max_speed and pull_gain must be positive and finite");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeGuidanceSettings {
    pub range: f32,
    pub turn_rate_degrees_per_second: f32,
    pub max_speed: f32,
    pub max_guidance_seconds: f32,
}

impl Default for HeGuidanceSettings {
    fn default() -> Self {
        Self {
            range: 2400.0,
            turn_rate_degrees_per_second: 120.0,
            max_speed: 1200.0,
            max_guidance_seconds: 8.0,
        }
    }
}

impl HeGuidanceSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !finite_positive(self.range)
            || !finite_positive(self.turn_rate_degrees_per_second)
            || !finite_positive(self.max_speed)
            || !finite_positive(self.max_guidance_seconds)
        {
            return Err("he_guidance range, turn_rate_degrees_per_second, max_speed and max_guidance_seconds must be positive and finite");
        }
        Ok(())
    }
}

/// Whether a trace result qualifies as an airborne wall contact for wall hang.
/// Trace reach, entity identity and the movement hull are checked by the adapter.
pub fn wall_hang_contact_eligible(
    is_walk: bool,
    airborne: bool,
    jump_held: bool,
    hit_world: bool,
    start_solid: bool,
    gap: f32,
    normal_z: f32,
) -> bool {
    is_walk
        && airborne
        && jump_held
        && hit_world
        && !start_solid
        && gap.is_finite()
        && (0.0..=0.75).contains(&gap)
        && normal_z.is_finite()
        && normal_z.abs() <= 0.25
}

/// Turns an existing projectile velocity toward a target direction without adding speed.
/// The adapter supplies game-time delta; a nonpositive delta permits speed capping but no turn.
pub fn turn_projectile_velocity(
    current_velocity: [f32; 3],
    target_direction: [f32; 3],
    delta_seconds: f32,
    settings: HeGuidanceSettings,
) -> Option<[f32; 3]> {
    settings.validate().ok()?;
    if current_velocity.iter().chain(target_direction.iter()).any(|value| !value.is_finite()) || !delta_seconds.is_finite() {
        return None;
    }
    let current_speed = length(current_velocity);
    let target_length = length(target_direction);
    if !current_speed.is_finite() || !target_length.is_finite() || target_length <= f32::EPSILON {
        return None;
    }
    let speed = current_speed.min(settings.max_speed);
    if speed <= f32::EPSILON {
        return Some([0.0; 3]);
    }
    let direction = scale(current_velocity, 1.0 / current_speed);
    let target = scale(target_direction, 1.0 / target_length);
    let angle = dot(direction, target).clamp(-1.0, 1.0).acos();
    let max_turn = if delta_seconds > 0.0 {
        (degrees_to_radians(settings.turn_rate_degrees_per_second) * delta_seconds).min(std::f32::consts::PI)
    } else {
        0.0
    };
    let turned = if angle <= max_turn || angle <= f32::EPSILON {
        target
    } else if max_turn <= 0.0 {
        direction
    } else {
        let axis = cross(direction, target);
        let axis_length = length(axis);
        let axis = if axis_length > f32::EPSILON {
            scale(axis, 1.0 / axis_length)
        } else {
            // Antiparallel vectors have no unique turn axis; choose a stable perpendicular.
            let reference = if direction[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            let perpendicular = cross(direction, reference);
            scale(perpendicular, 1.0 / length(perpendicular))
        };
        let radians = max_turn.min(angle);
        let (sin, cos) = radians.sin_cos();
        let axis_cross_direction = cross(axis, direction);
        let projection = dot(axis, direction);
        [
            direction[0] * cos + axis_cross_direction[0] * sin + axis[0] * projection * (1.0 - cos),
            direction[1] * cos + axis_cross_direction[1] * sin + axis[1] * projection * (1.0 - cos),
            direction[2] * cos + axis_cross_direction[2] * sin + axis[2] * projection * (1.0 - cos),
        ]
    };
    Some(scale(turned, speed))
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn length(value: [f32; 3]) -> f32 {
    dot(value, value).sqrt()
}

fn scale(value: [f32; 3], scalar: f32) -> [f32; 3] {
    [value[0] * scalar, value[1] * scalar, value[2] * scalar]
}

fn degrees_to_radians(degrees: f32) -> f32 {
    degrees * std::f32::consts::PI / 180.0
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FriendlyFireHealSettings {
    pub kind: FriendlyFireHealKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FriendlyFireHealKind {
    Fixed { amount: u16 },
    DamageRatio { ratio: f32 },
}

impl Default for FriendlyFireHealSettings {
    fn default() -> Self {
        Self {
            kind: FriendlyFireHealKind::Fixed { amount: 10 },
        }
    }
}

impl FriendlyFireHealSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        match self.kind {
            FriendlyFireHealKind::Fixed { amount: 0 } => Err("friendly_fire_heal fixed amount must be positive"),
            FriendlyFireHealKind::Fixed { .. } => Ok(()),
            FriendlyFireHealKind::DamageRatio { ratio } if finite_positive(ratio) => Ok(()),
            FriendlyFireHealKind::DamageRatio { .. } => Err("friendly_fire_heal ratio must be positive and finite"),
        }
    }

    /// Returns whole HP: fixed amount or floor(ratio × accumulated direct damage).
    pub fn healing(self, damage_sum: f64) -> i32 {
        match self.kind {
            FriendlyFireHealKind::Fixed { amount } => i32::from(amount),
            FriendlyFireHealKind::DamageRatio { ratio } if damage_sum.is_finite() && damage_sum > 0.0 && finite_positive(ratio) => {
                (damage_sum * f64::from(ratio)).floor().clamp(0.0, f64::from(i32::MAX)) as i32
            }
            FriendlyFireHealKind::DamageRatio { .. } => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegenerationSettings {
    pub delay_seconds: f32,
    pub interval_seconds: f32,
    pub heal: HealingAmount,
}

impl Default for RegenerationSettings {
    fn default() -> Self {
        Self {
            delay_seconds: 5.0,
            interval_seconds: 1.0,
            heal: HealingAmount::Fixed(2),
        }
    }
}

impl RegenerationSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !finite_positive(self.delay_seconds) || !finite_positive(self.interval_seconds) {
            return Err("regeneration needs positive finite delays");
        }
        self.heal.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChickenAuraSettings {
    pub radius: f32,
    pub interval_seconds: f32,
    pub heal: HealingAmount,
}

impl Default for ChickenAuraSettings {
    fn default() -> Self {
        Self {
            radius: 200.0,
            interval_seconds: 1.0,
            heal: HealingAmount::Fixed(3),
        }
    }
}

impl ChickenAuraSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !finite_positive(self.radius) || !finite_positive(self.interval_seconds) {
            return Err("chicken_aura needs positive finite radius and interval");
        }
        self.heal.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DontStopSettings {
    pub minimum_horizontal_speed: f32,
    pub grace_seconds: f32,
    pub damage_interval_seconds: f32,
    pub damage: i32,
}

impl Default for DontStopSettings {
    fn default() -> Self {
        Self {
            minimum_horizontal_speed: 20.0,
            grace_seconds: 1.5,
            damage_interval_seconds: 1.0,
            damage: 5,
        }
    }
}

impl DontStopSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !finite_nonnegative(self.minimum_horizontal_speed)
            || !finite_nonnegative(self.grace_seconds)
            || !finite_positive(self.damage_interval_seconds)
            || self.damage <= 0
        {
            return Err("dont_stop needs finite nonnegative speed/grace, a positive interval and positive damage");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HotFloorSettings {
    pub grounded_seconds: f32,
    pub damage_interval_seconds: f32,
    pub damage: i32,
    pub ignite_after_seconds: f32,
    pub burn_duration_seconds: f32,
    pub burn_damage_fraction: f32,
    pub burn_minimum_damage: f32,
}

impl Default for HotFloorSettings {
    fn default() -> Self {
        Self {
            grounded_seconds: 2.0,
            damage_interval_seconds: 0.5,
            damage: 3,
            ignite_after_seconds: 4.0,
            burn_duration_seconds: 5.0,
            burn_damage_fraction: 0.01,
            burn_minimum_damage: 1.0,
        }
    }
}

impl HotFloorSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !finite_positive(self.grounded_seconds)
            || !finite_positive(self.damage_interval_seconds)
            || self.damage <= 0
            || !finite_positive(self.ignite_after_seconds)
            || !finite_positive(self.burn_duration_seconds)
            || !finite_nonnegative(self.burn_damage_fraction)
            || !finite_nonnegative(self.burn_minimum_damage)
        {
            return Err("hot_floor needs positive finite ground/damage/ignite/burn times, positive damage, and finite nonnegative burn damage");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponOnFireSettings {
    pub chance: f32,
    pub switch_weight: u32,
    pub drop_weight: u32,
    pub cooldown_seconds: f32,
}

impl Default for WeaponOnFireSettings {
    fn default() -> Self {
        Self {
            chance: 0.1,
            switch_weight: 3,
            drop_weight: 1,
            cooldown_seconds: 1.0,
        }
    }
}

impl WeaponOnFireSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.chance.is_finite()
            || !(0.0..=1.0).contains(&self.chance)
            || (self.switch_weight == 0 && self.drop_weight == 0)
            || !finite_nonnegative(self.cooldown_seconds)
        {
            return Err("weapon_on_fire needs chance between 0 and 1, at least one positive action weight, and a finite nonnegative cooldown");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LongFlashSettings {
    pub duration_multiplier: f32,
    pub duration_cap_seconds: f32,
}

impl Default for LongFlashSettings {
    fn default() -> Self {
        Self {
            duration_multiplier: 3.0,
            duration_cap_seconds: 12.0,
        }
    }
}

impl LongFlashSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.duration_multiplier.is_finite() || self.duration_multiplier < 1.0 || !finite_nonnegative(self.duration_cap_seconds) {
            return Err("long_flash needs a finite duration_multiplier >= 1 and finite nonnegative extension cap");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BigHeadSettings {
    pub min_scale: f32,
    pub max_scale: f32,
}

impl Default for BigHeadSettings {
    fn default() -> Self {
        Self {
            min_scale: 0.5,
            max_scale: 2.0,
        }
    }
}

impl BigHeadSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !finite_positive(self.min_scale) || !finite_positive(self.max_scale) || self.min_scale > self.max_scale {
            return Err("big_head needs positive finite scales with min_scale <= max_scale");
        }
        Ok(())
    }
}

fn finite_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn finite_nonnegative(value: f32) -> bool {
    value.is_finite() && value >= 0.0
}

/// Returns the next health value, or None when the player should not be healed.
/// Existing health above the configured ceiling is never reduced.
pub fn healing_target(base_health: i32, current_health: i32, amount: i32, cap_base_multiple: f32) -> Option<i32> {
    if base_health <= 0 || !finite_positive(cap_base_multiple) || amount <= 0 || current_health <= 0 {
        return None;
    }
    let cap = (f64::from(base_health) * f64::from(cap_base_multiple)).floor();
    if !cap.is_finite() {
        return None;
    }
    let cap = cap.min(f64::from(i32::MAX)) as i32;
    let healed = current_health.saturating_add(amount).min(cap);
    (healed > current_health).then_some(healed)
}

/// Uses the player's real match kills/deaths. Clamp bounds are validated in configuration.
pub fn big_head_scale(kills: u32, deaths: u32, min_scale: f32, max_scale: f32) -> f32 {
    let ratio = (f64::from(kills) + 1.0) / (f64::from(deaths) + 1.0);
    (ratio as f32).clamp(min_scale, max_scale)
}

/// Multiplies the native duration up to `duration_cap_seconds`; an already later
/// native or plugin deadline is preserved.
pub fn extended_flash_deadline(
    now: f64,
    native_deadline: f64,
    current_deadline: f64,
    duration_multiplier: f32,
    duration_cap_seconds: f32,
) -> Option<f64> {
    if !now.is_finite()
        || !native_deadline.is_finite()
        || !current_deadline.is_finite()
        || !duration_multiplier.is_finite()
        || duration_multiplier < 1.0
        || !duration_cap_seconds.is_finite()
        || duration_cap_seconds < 0.0
        || native_deadline <= now
    {
        return None;
    }
    let duration = ((native_deadline - now) * f64::from(duration_multiplier)).min(f64::from(duration_cap_seconds));
    let target = now + duration;
    target.is_finite().then_some(current_deadline.max(native_deadline).max(target))
}

macro_rules! settings_input {
    ($name:ident, $settings:ident, { $($field:ident : $kind:ty),+ $(,)? }) => {
        #[derive(Clone, Debug, Default, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub(super) struct $name { $(pub(super) $field: Option<$kind>,)+ }

        impl $name {
            pub(super) fn resolve(self, base: Option<$settings>) -> $settings {
                let base = base.unwrap_or_default();
                $settings { $($field: self.$field.unwrap_or(base.$field),)+ }
            }
        }
    };
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct RegenerationInput {
    pub(super) delay_seconds: Option<f32>,
    pub(super) interval_seconds: Option<f32>,
    pub(super) heal: Option<i32>,
    pub(super) heal_percent: Option<f32>,
}

impl RegenerationInput {
    pub(super) fn resolve(self, base: Option<RegenerationSettings>) -> Result<RegenerationSettings, String> {
        let base = base.unwrap_or_default();
        let heal = resolve_healing_amount(self.heal, self.heal_percent, base.heal, "regeneration")?;
        Ok(RegenerationSettings {
            delay_seconds: self.delay_seconds.unwrap_or(base.delay_seconds),
            interval_seconds: self.interval_seconds.unwrap_or(base.interval_seconds),
            heal,
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ChickenAuraInput {
    pub(super) radius: Option<f32>,
    pub(super) interval_seconds: Option<f32>,
    pub(super) heal: Option<i32>,
    pub(super) heal_percent: Option<f32>,
}

impl ChickenAuraInput {
    pub(super) fn resolve(self, base: Option<ChickenAuraSettings>) -> Result<ChickenAuraSettings, String> {
        let base = base.unwrap_or_default();
        let heal = resolve_healing_amount(self.heal, self.heal_percent, base.heal, "chicken_aura")?;
        Ok(ChickenAuraSettings {
            radius: self.radius.unwrap_or(base.radius),
            interval_seconds: self.interval_seconds.unwrap_or(base.interval_seconds),
            heal,
        })
    }
}

fn resolve_healing_amount(heal: Option<i32>, heal_percent: Option<f32>, inherited: HealingAmount, ability: &str) -> Result<HealingAmount, String> {
    match (heal, heal_percent) {
        (Some(_), Some(_)) => Err(format!("{ability}: configure either heal or heal_percent, not both")),
        (Some(amount), None) => Ok(HealingAmount::Fixed(amount)),
        (None, Some(percent)) => Ok(HealingAmount::Percent(percent)),
        (None, None) => Ok(inherited),
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum FriendlyFireHealKindInput {
    Fixed,
    DamageRatio,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct FriendlyFireHealInput {
    pub(super) kind: Option<FriendlyFireHealKindInput>,
    pub(super) amount: Option<u16>,
    pub(super) ratio: Option<f32>,
}

impl FriendlyFireHealInput {
    pub(super) fn resolve(self, base: Option<FriendlyFireHealSettings>) -> Result<FriendlyFireHealSettings, String> {
        if self.amount.is_some() && self.ratio.is_some() {
            return Err("configure either amount or ratio, not both".into());
        }
        let base = base.unwrap_or_default();
        let inherited_kind = match base.kind {
            FriendlyFireHealKind::Fixed { .. } => FriendlyFireHealKindInput::Fixed,
            FriendlyFireHealKind::DamageRatio { .. } => FriendlyFireHealKindInput::DamageRatio,
        };
        let kind = self.kind.unwrap_or(inherited_kind);
        let changed_kind = self.kind.is_some_and(|kind| kind != inherited_kind);
        let settings = match kind {
            FriendlyFireHealKindInput::Fixed => {
                if self.ratio.is_some() {
                    return Err("kind = 'fixed' does not accept ratio".into());
                }
                let inherited = match base.kind {
                    FriendlyFireHealKind::Fixed { amount } if !changed_kind => Some(amount),
                    _ => None,
                };
                FriendlyFireHealSettings {
                    kind: FriendlyFireHealKind::Fixed {
                        amount: self.amount.or(inherited).unwrap_or(10),
                    },
                }
            }
            FriendlyFireHealKindInput::DamageRatio => {
                if self.amount.is_some() {
                    return Err("kind = 'damage_ratio' does not accept amount".into());
                }
                let inherited = match base.kind {
                    FriendlyFireHealKind::DamageRatio { ratio } if !changed_kind => Some(ratio),
                    _ => None,
                };
                FriendlyFireHealSettings {
                    kind: FriendlyFireHealKind::DamageRatio {
                        ratio: self.ratio.or(inherited).ok_or("kind = 'damage_ratio' requires ratio")?,
                    },
                }
            }
        };
        settings.validate()?;
        Ok(settings)
    }
}

settings_input!(ExplosiveBulletsInput, ExplosiveBulletsSettings, {
    cooldown_seconds: f32,
    damage: AttributeValue,
    radius: AttributeValue,
});

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct HeGrenadeImpulseInput {
    pub(super) impulse: Option<f32>,
}

impl HeGrenadeImpulseInput {
    pub(super) fn resolve(self, base: Option<HeGrenadeImpulseSettings>) -> HeGrenadeImpulseSettings {
        let base = base.unwrap_or_default();
        HeGrenadeImpulseSettings {
            impulse: self.impulse.unwrap_or(base.impulse),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ProximityFuseInput {
    pub(super) radius: Option<f32>,
    pub(super) delay_seconds: Option<f32>,
    pub(super) grenades: Option<Vec<String>>,
}

impl ProximityFuseInput {
    pub(super) fn resolve(self, base: Option<ProximityFuseSettings>) -> Result<ProximityFuseSettings, String> {
        let base = base.unwrap_or_default();
        let grenades = match self.grenades {
            Some(items) => items
                .into_iter()
                .map(|item| GrenadeKind::from_config_id(&item).map_err(|error| format!("grenades: {error}")))
                .collect::<Result<Vec<_>, _>>()?,
            None => base.grenades,
        };
        let settings = ProximityFuseSettings {
            radius: self.radius.unwrap_or(base.radius),
            delay_seconds: self.delay_seconds.unwrap_or(base.delay_seconds),
            grenades,
        };
        settings.validate().map_err(str::to_owned)?;
        Ok(settings)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct GrappleInput {
    pub(super) max_distance: Option<f32>,
    pub(super) max_speed: Option<f32>,
    pub(super) pull_gain: Option<f32>,
}

impl GrappleInput {
    pub(super) fn resolve(self, base: Option<GrappleSettings>) -> GrappleSettings {
        let base = base.unwrap_or_default();
        GrappleSettings {
            max_distance: self.max_distance.unwrap_or(base.max_distance),
            max_speed: self.max_speed.unwrap_or(base.max_speed),
            pull_gain: self.pull_gain.unwrap_or(base.pull_gain),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct HeGuidanceInput {
    pub(super) range: Option<f32>,
    pub(super) turn_rate_degrees_per_second: Option<f32>,
    pub(super) max_speed: Option<f32>,
    pub(super) max_guidance_seconds: Option<f32>,
}

impl HeGuidanceInput {
    pub(super) fn resolve(self, base: Option<HeGuidanceSettings>) -> HeGuidanceSettings {
        let base = base.unwrap_or_default();
        HeGuidanceSettings {
            range: self.range.unwrap_or(base.range),
            turn_rate_degrees_per_second: self.turn_rate_degrees_per_second.unwrap_or(base.turn_rate_degrees_per_second),
            max_speed: self.max_speed.unwrap_or(base.max_speed),
            max_guidance_seconds: self.max_guidance_seconds.unwrap_or(base.max_guidance_seconds),
        }
    }
}

settings_input!(DontStopInput, DontStopSettings, {
    minimum_horizontal_speed: f32,
    grace_seconds: f32,
    damage_interval_seconds: f32,
    damage: i32,
});
settings_input!(HotFloorInput, HotFloorSettings, {
    grounded_seconds: f32,
    damage_interval_seconds: f32,
    damage: i32,
    ignite_after_seconds: f32,
    burn_duration_seconds: f32,
    burn_damage_fraction: f32,
    burn_minimum_damage: f32,
});
settings_input!(WeaponOnFireInput, WeaponOnFireSettings, {
    chance: f32,
    switch_weight: u32,
    drop_weight: u32,
    cooldown_seconds: f32,
});
settings_input!(LongFlashInput, LongFlashSettings, {
    duration_multiplier: f32,
    duration_cap_seconds: f32,
});
settings_input!(BigHeadInput, BigHeadSettings, {
    min_scale: f32,
    max_scale: f32,
});
