// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

mod abilities;
use abilities::{
    BigHeadInput, ChickenAuraInput, DontStopInput, ExplosiveBulletsInput, FriendlyFireHealInput, GrappleInput, HeGrenadeImpulseInput,
    HeGuidanceInput, HotFloorInput, LongFlashInput, ProximityFuseInput, RegenerationInput, WeaponOnFireInput,
};
pub use abilities::{
    BigHeadSettings, ChickenAuraSettings, DontStopSettings, ExplosiveBulletsSettings, FriendlyFireHealKind, FriendlyFireHealSettings,
    GrappleSettings, HeGrenadeImpulseSettings, HeGuidanceSettings, HealingAmount, HotFloorSettings, LongFlashSettings, ProximityFuseSettings,
    RegenerationSettings, WeaponOnFireSettings, big_head_scale, extended_flash_deadline, healing_target, turn_projectile_velocity,
    wall_hang_contact_eligible,
};

mod attributes;
pub use attributes::{AttributeRange, AttributeValue, HelmetValue, LifeAttributes, RandomHelmet};
mod chicken;
pub use chicken::{
    ChickenBlastConfig, ChickenConfig, ChickenDamageCause, ChickenDamageChainConfig, ChickenDamageRule, ChickenEatError, ChickenEatOutcome,
    ChickenEatingConfig, ChickenEatingState, ChickenHealConfig, ChickenHealthBasis, ChickenPercentRange, ChickenSpawnConfig, HealthshotConfig,
    HealthshotOutcome, filters_chicken_damage_chain, use_healthshot,
};
use chicken::{ChickenInput, HealthshotInput};
mod ghost;
use ghost::GhostInput;
pub use ghost::{GhostFrame, GhostPose, GhostSettings, GhostState, ghost_alpha};
mod afterburn;
use afterburn::AfterburnInput;
pub use afterburn::AfterburnSettings;
mod third_person;
use third_person::ThirdPersonInput;
pub use third_person::ThirdPersonSettings;

mod rounds;
pub use rounds::{Capabilities, NextMode, Phase, RoundController, RoundGeneration, RoundKey};
mod players;
pub use players::{PlayerIdentity, PlayerTask, PlayerTracker, TeamkillCounter};
mod rules;
pub use rules::{
    DODGEBALL_DAMAGE_MULTIPLIER, LIFESTEAL_MAX_HEALTH, MovementScale, PeriodicDistribution, PeriodicMovement, PeriodicSetting,
    THROWING_KNIFE_BODY_DAMAGE, THROWING_KNIFE_ELASTICITY, THROWING_KNIFE_GRAVITY, THROWING_KNIFE_HEAD_DAMAGE, THROWING_KNIFE_HEADSHOT_DISTANCE,
    THROWING_KNIFE_LIFETIME, THROWING_KNIFE_SPEED, TeamDeathmatch, TeamResult, dodgeball_damage, lifesteal_recovery, low_health_finish,
    shockwave_impulse,
};
mod health;
pub mod regions;
use health::{HealthCurveInput, HealthDecayInput};
mod text;
pub use text::{TextBundle, TextCatalog};
mod combat;
pub use combat::{CombatLife, combat_hit, weapon_damage_scale};
mod random_damage;
pub use random_damage::{RandomDamage, random_damage};
mod autostrafe;
mod suicide;
pub use autostrafe::air_strafe_input;
pub use suicide::{DeathExplosion, SuicideSequence, suicide_blast_damage};
mod equipment;
mod money;
pub use equipment::{WeaponSlot, self_push_force, weapon_slot};
pub use equipment::{item_classname, item_id};
pub use health::{HealthBand, HealthCurve, HealthDecay};
pub use money::{money_floor, money_income_credit, next_money_income_due};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Headshots {
    #[default]
    Normal,
    Only,
    Disabled,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Ammo {
    #[default]
    Inherit,
    Reserve,
    Clip,
    SingleShot,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SelfDamage {
    MissPenalty,
    Native { amount: f32 },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SelfDamageKind {
    MissPenalty,
    Native,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SelfDamageInput {
    kind: Option<SelfDamageKind>,
    amount: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct WeaponDamage(BTreeMap<String, f32>);

impl WeaponDamage {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn configured(&self, id: &str) -> Option<f32> {
        self.0.get(id).copied()
    }

    pub fn multiplier(&self, id: &str) -> f32 {
        self.configured(id).unwrap_or(1.0)
    }

    fn validate(&self) -> Result<(), String> {
        for (id, multiplier) in &self.0 {
            if let Some(bare) = id.strip_prefix("weapon_") {
                return Err(format!("weapon_damage.{id}: use weapon_damage.{bare}"));
            }
            if equipment::weapon_slot(id).is_none() {
                return Err(format!("weapon_damage.{id}: unknown weapon ID"));
            }
            if !multiplier.is_finite() || *multiplier < 0.0 {
                return Err(format!("weapon_damage.{id}: multiplier must be finite and non-negative"));
            }
        }
        Ok(())
    }
}

pub const VERSION: &str = "0.4-Alpha Test 3";

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

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct BombInput {
    kind: Option<BombKind>,
    plant_time: Option<f32>,
    explode_time: Option<f32>,
    defuse_kit_time: Option<f32>,
    defuse_time: Option<f32>,
    bounce: Option<bool>,
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

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub default_language: String,
    pub diagnostics: bool,
    pub hostage_rescue_compat: bool,
    pub prices: CashPrices,
    pub warmup: Option<WarmupConfig>,
    pub chicken: Option<ChickenConfig>,
    /// Presence of `[healthshot]` opts in to replacing native healthshot healing.
    pub healthshot: Option<HealthshotConfig>,
    pub modes: Vec<Mode>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CashPrices {
    pub c4_cash: u32,
    pub healthshot_cash: u32,
    pub chicken_cash: u32,
}

impl Default for CashPrices {
    fn default() -> Self {
        Self {
            c4_cash: 10_000,
            healthshot_cash: 4_000,
            chicken_cash: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TeamFilter {
    #[default]
    None,
    All,
    Terrorists,
    CounterTerrorists,
}

impl TeamFilter {
    pub const fn matches(self, team: i32) -> bool {
        match self {
            Self::None => false,
            Self::All => matches!(team, 2 | 3),
            Self::Terrorists => team == 2,
            Self::CounterTerrorists => team == 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GrenadeKind {
    He,
    Flash,
    Smoke,
    Decoy,
    Molotov,
    Incendiary,
}

impl GrenadeKind {
    pub(crate) fn from_config_id(item: &str) -> Result<Self, String> {
        match item {
            "hegrenade" => Ok(Self::He),
            "flashbang" => Ok(Self::Flash),
            "smokegrenade" => Ok(Self::Smoke),
            "decoy" => Ok(Self::Decoy),
            "molotov" => Ok(Self::Molotov),
            "incgrenade" => Ok(Self::Incendiary),
            "he" => Err("use canonical item ID 'hegrenade' instead of legacy alias 'he'".into()),
            "flash" => Err("use canonical item ID 'flashbang' instead of legacy alias 'flash'".into()),
            "smoke" => Err("use canonical item ID 'smokegrenade' instead of legacy alias 'smoke'".into()),
            "incendiary" => Err("use canonical item ID 'incgrenade' instead of legacy alias 'incendiary'".into()),
            value if value.starts_with("weapon_") => {
                let canonical = value.strip_prefix("weapon_").unwrap_or(value);
                if matches!(canonical, "hegrenade" | "flashbang" | "smokegrenade" | "decoy" | "molotov" | "incgrenade") {
                    Err(format!("use canonical item ID '{canonical}' without the 'weapon_' prefix"))
                } else {
                    Err(format!("unsupported grenade item ID '{item}'"))
                }
            }
            _ => Err(format!("unsupported grenade item ID '{item}'")),
        }
    }

    pub const fn item_id(self) -> &'static str {
        match self {
            Self::He => "hegrenade",
            Self::Flash => "flashbang",
            Self::Smoke => "smokegrenade",
            Self::Decoy => "decoy",
            Self::Molotov => "molotov",
            Self::Incendiary => "incgrenade",
        }
    }

    pub const fn classname(self) -> &'static str {
        match self {
            Self::He => "weapon_hegrenade",
            Self::Flash => "weapon_flashbang",
            Self::Smoke => "weapon_smokegrenade",
            Self::Decoy => "weapon_decoy",
            Self::Molotov => "weapon_molotov",
            Self::Incendiary => "weapon_incgrenade",
        }
    }
}

impl<'de> Deserialize<'de> for GrenadeKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let item = String::deserialize(deserializer)?;
        Self::from_config_id(&item).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WinCondition {
    #[default]
    Native,
    TeamDeathmatch,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KillLoadoutStart {
    Glock,
    Random,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RoundRespawn {
    #[default]
    Disabled,
    Dead,
    All,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NamedRoundRespawn {
    All,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(untagged)]
enum RoundRespawnInput {
    Legacy(bool),
    Named(NamedRoundRespawn),
}

impl RoundRespawnInput {
    fn resolve(self) -> RoundRespawn {
        match self {
            Self::Legacy(false) => RoundRespawn::Disabled,
            Self::Legacy(true) => RoundRespawn::Dead,
            Self::Named(NamedRoundRespawn::All) => RoundRespawn::All,
        }
    }
}

impl std::fmt::Display for RoundRespawn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Disabled => "false",
            Self::Dead => "true",
            Self::All => "all",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RespawnSettings {
    pub team: TeamFilter,
    #[serde(default = "default_respawn_delay")]
    pub delay_seconds: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MoneyIncome {
    pub interval: f32,
    pub amount: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RespawnInput {
    team: Option<TeamFilter>,
    delay_seconds: Option<f32>,
}

fn default_respawn_delay() -> f32 {
    3.0
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub description: String,
    pub name_key: Option<String>,
    pub description_key: Option<String>,
    pub weapons: Vec<String>,
    pub health: AttributeValue,
    pub armor: AttributeValue,
    pub gravity: f32,
    pub speed: f32,
    pub helmet: HelmetValue,
    pub clear_weapons: TeamFilter,
    pub give_weapons: TeamFilter,
    pub give_knife: TeamFilter,
    pub color: Option<[u8; 3]>,
    pub headshots: Headshots,
    pub ammo: Ammo,
    pub self_damage: Option<SelfDamage>,
    pub weapon_damage: WeaponDamage,
    pub taser_charge: Option<f32>,
    pub bunnyhop: Option<bool>,
    pub autostrafe: bool,
    pub friction: Option<f32>,
    pub time_scale: Option<f32>,
    pub ground_acceleration: Option<f32>,
    pub ground_reverse: Option<f32>,
    pub no_spread: Option<bool>,
    pub no_recoil: bool,
    pub recoil_view: Option<f32>,
    pub fast_switch: bool,
    pub replenish: Vec<GrenadeKind>,
    pub random_loadout: bool,
    pub suicide: bool,
    pub death_explosion: DeathExplosion,
    pub random_damage: bool,
    pub kill_loadout: bool,
    pub kill_loadout_start: Option<KillLoadoutStart>,
    pub lifesteal: bool,
    pub reflect_damage: Option<f32>,
    pub energy_shield: bool,
    pub spawn_protection: bool,
    pub kill_protection: bool,
    pub translucent: bool,
    pub ghost: Option<GhostSettings>,
    pub afterburn: Option<AfterburnSettings>,
    pub third_person: Option<ThirdPersonSettings>,
    pub delayed_damage: bool,
    pub win_condition: WinCondition,
    pub bomb: Option<BombSettings>,
    pub dodgeball: bool,
    pub throwing_knives: bool,
    pub view_punch_decay: Option<f32>,
    pub no_scope: bool,
    pub fov: Option<i32>,
    pub hide_crosshair_after: Option<f32>,
    pub health_damage: bool,
    pub health_speed: bool,
    pub health_decay: bool,
    pub health_curve: HealthCurve,
    pub health_decay_settings: HealthDecay,
    pub self_push: bool,
    pub periodic_speed: Option<PeriodicSetting>,
    pub periodic_gravity: Option<PeriodicSetting>,
    pub c4_count: Option<u8>,
    pub c4_buy: bool,
    pub round_respawn: RoundRespawn,
    pub hide_all_hud: bool,
    pub hide_radar: Option<bool>,
    pub hide_player_identification: Option<bool>,
    pub respawn: Option<RespawnSettings>,
    pub buy_time: Option<f32>,
    pub buy_limit: Option<u32>,
    pub round_money_floor: Option<u32>,
    pub money_income: Option<MoneyIncome>,
    pub buy_access: Option<TeamFilter>,
    pub buy_anywhere: Option<TeamFilter>,
    pub friendly_fire_scale: Option<f32>,
    pub damage_multiplier: f32,
    pub explosive_bullets: Option<ExplosiveBulletsSettings>,
    pub friendly_fire_heal: Option<FriendlyFireHealSettings>,
    pub inferno_damage: Option<f32>,
    pub teamkill_limit: Option<u32>,
    pub hegrenade_impulse: Option<HeGrenadeImpulseSettings>,
    pub proximity_fuse: Option<ProximityFuseSettings>,
    pub wall_hang: bool,
    pub grapple: Option<GrappleSettings>,
    pub he_guidance: Option<HeGuidanceSettings>,
    pub teammates_as_enemies: Option<bool>,
    pub random_spawn: Option<TeamFilter>,
    pub drop_weapons: Option<bool>,
    pub drop_grenades: Option<bool>,
    pub shoot_dropped_grenades: Option<bool>,
    pub fall_damage_scale: Option<f32>,
    pub ordinary_chickens: bool,
    pub chicken_eating: Option<bool>,
    pub regeneration: Option<RegenerationSettings>,
    pub chicken_aura: Option<ChickenAuraSettings>,
    pub medical_smoke: bool,
    pub trampoline: bool,
    pub healing_cap_base_multiple: f32,
    pub dont_stop: Option<DontStopSettings>,
    pub hot_floor: Option<HotFloorSettings>,
    pub weapon_on_fire: Option<WeaponOnFireSettings>,
    pub long_flash: Option<LongFlashSettings>,
    pub big_head: Option<BigHeadSettings>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WarmupConfig {
    pub enabled: bool,
    pub mode: Mode,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WarmupInput {
    respawn_delay: Option<f32>,
    #[serde(flatten)]
    mode: ModeInput,
}

impl WarmupInput {
    fn into_config(mut self, weapon_groups: &BTreeMap<String, Vec<String>>) -> Result<WarmupConfig, String> {
        if self.mode.id.is_some()
            || self.mode.name.is_some()
            || self.mode.description.is_some()
            || self.mode.name_key.is_some()
            || self.mode.description_key.is_some()
        {
            return Err("[warmup] may contain capability settings and enabled only".into());
        }
        if self.respawn_delay.is_some() && self.mode.respawn.is_some() {
            return Err("warmup: use respawn_delay or respawn, not both".into());
        }
        if self.mode.death_explosion.is_some_and(DeathExplosion::enabled) {
            return Err("warmup: death_explosion requires a competitive round".into());
        }
        let enabled = self.mode.enabled.unwrap_or(true);
        self.mode.resolve_weapons(weapon_groups, "warmup")?;
        self.mode.id = Some("warmup".into());
        self.mode.enabled = Some(true);
        self.mode.name_key = Some("warmup.name".into());
        self.mode.description_key = Some("warmup.description".into());
        // Preserve the original warmup defaults, independently of [defaults].
        let defaults = ModeInput {
            health: Some(AttributeValue::Fixed(100)),
            armor: Some(AttributeValue::Fixed(100)),
            clear_weapons: Some(TeamFilter::All),
            give_weapons: Some(TeamFilter::All),
            give_knife: Some(TeamFilter::All),
            ammo: Some(Ammo::Reserve),
            fast_switch: Some(true),
            self_push: Some(true),
            respawn: Some(OptionalOverride::Value(RespawnInput {
                team: Some(TeamFilter::All),
                delay_seconds: Some(self.respawn_delay.unwrap_or(1.0)),
            })),
            ..ModeInput::default()
        };
        Ok(WarmupConfig {
            enabled,
            mode: self.mode.into_mode(&defaults)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum OptionalOverride<T> {
    Clear(bool),
    Value(T),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum WeaponSelection {
    List(Vec<String>),
    Group(String),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum PeriodicSettingInput {
    Enabled(bool),
    Custom(PeriodicSettingFields),
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PeriodicSettingFields {
    interval_seconds: Option<f32>,
    values: Option<Vec<f32>>,
    min: Option<f32>,
    max: Option<f32>,
}

impl PeriodicSettingInput {
    fn resolve(self, name: &str) -> Result<Option<PeriodicSetting>, String> {
        match self {
            Self::Enabled(false) => Ok(None),
            Self::Enabled(true) => Ok(Some(PeriodicSetting {
                interval_seconds: 10.0,
                distribution: PeriodicDistribution::Legacy,
            })),
            Self::Custom(fields) => {
                let interval_seconds = fields.interval_seconds.ok_or_else(|| format!("{name}.interval_seconds is required"))?;
                let distribution = match (fields.values, fields.min, fields.max) {
                    (Some(values), None, None) => PeriodicDistribution::Tiers(values),
                    (None, Some(min), Some(max)) => PeriodicDistribution::Range { min, max },
                    (None, None, None) => return Err(format!("{name} requires either values or both min and max")),
                    _ => return Err(format!("{name} requires either values or both min and max, not both")),
                };
                let setting = PeriodicSetting {
                    interval_seconds,
                    distribution,
                };
                setting.validate().map_err(|error| format!("{name}: {error}"))?;
                Ok(Some(setting))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HudElement {
    All,
    Radar,
    Crosshair,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ModeInput {
    id: Option<String>,
    enabled: Option<bool>,
    name: Option<String>,
    description: Option<String>,
    name_key: Option<String>,
    description_key: Option<String>,
    weapons: Option<WeaponSelection>,
    health: Option<AttributeValue>,
    armor: Option<AttributeValue>,
    gravity: Option<f32>,
    speed: Option<f32>,
    helmet: Option<HelmetValue>,
    clear_weapons: Option<TeamFilter>,
    give_weapons: Option<TeamFilter>,
    give_knife: Option<TeamFilter>,
    color: Option<OptionalOverride<[u8; 3]>>,
    headshots: Option<Headshots>,
    ammo: Option<Ammo>,
    self_damage: Option<OptionalOverride<SelfDamageInput>>,
    weapon_damage: Option<WeaponDamage>,
    taser_charge: Option<OptionalOverride<f32>>,
    bunnyhop: Option<bool>,
    autostrafe: Option<bool>,
    friction: Option<OptionalOverride<f32>>,
    time_scale: Option<OptionalOverride<f32>>,
    ground_acceleration: Option<OptionalOverride<f32>>,
    ground_reverse: Option<OptionalOverride<f32>>,
    no_spread: Option<bool>,
    no_recoil: Option<bool>,
    recoil_view: Option<OptionalOverride<f32>>,
    fast_switch: Option<bool>,
    replenish: Option<Vec<GrenadeKind>>,
    random_loadout: Option<bool>,
    suicide: Option<bool>,
    death_explosion: Option<DeathExplosion>,
    random_damage: Option<bool>,
    kill_loadout: Option<bool>,
    kill_loadout_start: Option<KillLoadoutStart>,
    lifesteal: Option<bool>,
    reflect_damage: Option<OptionalOverride<f32>>,
    energy_shield: Option<bool>,
    spawn_protection: Option<bool>,
    kill_protection: Option<bool>,
    combat_protection: Option<bool>,
    translucent: Option<bool>,
    delayed_damage: Option<bool>,
    win_condition: Option<WinCondition>,
    bomb: Option<OptionalOverride<BombInput>>,
    dodgeball: Option<bool>,
    throwing_knives: Option<bool>,
    view_punch_decay: Option<OptionalOverride<f32>>,
    no_scope: Option<bool>,
    fov: Option<OptionalOverride<i32>>,
    hide_crosshair_after: Option<OptionalOverride<f32>>,
    hide_hud: Option<Vec<HudElement>>,
    health_damage: Option<bool>,
    health_speed: Option<bool>,
    health_decay: Option<bool>,
    health_curve: Option<HealthCurveInput>,
    health_decay_settings: Option<HealthDecayInput>,
    self_push: Option<bool>,
    periodic_speed: Option<PeriodicSettingInput>,
    periodic_gravity: Option<PeriodicSettingInput>,
    c4_count: Option<OptionalOverride<u8>>,
    c4_buy: Option<bool>,
    round_respawn: Option<RoundRespawnInput>,
    hide_radar: Option<bool>,
    hide_player_identification: Option<bool>,
    respawn: Option<OptionalOverride<RespawnInput>>,
    buy_time: Option<f32>,
    buy_limit: Option<u32>,
    round_money_floor: Option<OptionalOverride<u32>>,
    money_income: Option<OptionalOverride<MoneyIncome>>,
    buy_access: Option<OptionalOverride<TeamFilter>>,
    buy_anywhere: Option<OptionalOverride<TeamFilter>>,
    friendly_fire_scale: Option<f32>,
    damage_multiplier: Option<OptionalOverride<f32>>,
    explosive_bullets: Option<OptionalOverride<ExplosiveBulletsInput>>,
    friendly_fire_heal: Option<OptionalOverride<FriendlyFireHealInput>>,
    inferno_damage: Option<OptionalOverride<f32>>,
    teamkill_limit: Option<OptionalOverride<u32>>,
    hegrenade_impulse: Option<OptionalOverride<HeGrenadeImpulseInput>>,
    proximity_fuse: Option<OptionalOverride<ProximityFuseInput>>,
    wall_hang: Option<bool>,
    grapple: Option<OptionalOverride<GrappleInput>>,
    he_guidance: Option<OptionalOverride<HeGuidanceInput>>,
    teammates_as_enemies: Option<bool>,
    random_spawn: Option<TeamFilter>,
    drop_weapons: Option<bool>,
    drop_grenades: Option<bool>,
    shoot_dropped_grenades: Option<bool>,
    fall_damage_scale: Option<f32>,
    ordinary_chickens: Option<bool>,
    chicken_eating: Option<bool>,
    regeneration: Option<OptionalOverride<RegenerationInput>>,
    chicken_aura: Option<OptionalOverride<ChickenAuraInput>>,
    medical_smoke: Option<bool>,
    trampoline: Option<bool>,
    healing_cap_base_multiple: Option<f32>,
    dont_stop: Option<OptionalOverride<DontStopInput>>,
    hot_floor: Option<OptionalOverride<HotFloorInput>>,
    weapon_on_fire: Option<OptionalOverride<WeaponOnFireInput>>,
    long_flash: Option<OptionalOverride<LongFlashInput>>,
    big_head: Option<OptionalOverride<BigHeadInput>>,
    ghost: Option<OptionalOverride<GhostInput>>,
    afterburn: Option<OptionalOverride<AfterburnInput>>,
    third_person: Option<OptionalOverride<ThirdPersonInput>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    diagnostics: bool,
    #[serde(default)]
    hostage_rescue_compat: bool,
    #[serde(default)]
    prices: CashPrices,
    chicken: Option<ChickenInput>,
    healthshot: Option<HealthshotInput>,
    #[serde(default)]
    defaults: ModeInput,
    #[serde(default)]
    weapon_groups: BTreeMap<String, Vec<String>>,
    warmup: Option<WarmupInput>,
    modes: Vec<ModeInput>,
    #[serde(default)]
    i18n: I18nInput,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct I18nInput {
    default_language: String,
}

impl Default for I18nInput {
    fn default() -> Self {
        Self {
            default_language: "zh-CN".into(),
        }
    }
}

fn resolved_weapon_list(selection: Option<WeaponSelection>, owner: &str) -> Result<Option<Vec<String>>, String> {
    match selection {
        None => Ok(None),
        Some(WeaponSelection::List(weapons)) => Ok(Some(weapons)),
        Some(WeaponSelection::Group(_)) => Err(format!("{owner}.weapons weapon group reference was not resolved")),
    }
}

fn choose_periodic(
    mode: Option<PeriodicSettingInput>,
    defaults: Option<PeriodicSettingInput>,
    name: &str,
) -> Result<Option<PeriodicSetting>, String> {
    mode.or(defaults).map(|setting| setting.resolve(name)).transpose().map(Option::flatten)
}

fn equipment_input_error(owner: &str, item: &str) -> String {
    if item == "knife" || item == "weapon_knife" {
        return format!("{owner}: knife is configured separately from equipment; use give_knife");
    }
    if let Some(id) = item.strip_prefix("weapon_")
        && item_classname(id).is_some()
    {
        return format!("{owner}: use canonical item ID '{id}' without the 'weapon_' prefix");
    }
    format!("{owner}: equipment item '{item}' is not in the supported CS2 item whitelist")
}

fn supported_equipment_id(item: &str) -> bool {
    weapon_slot(item).is_some() && item != "knife"
}

fn choose<T>(mode: Option<T>, defaults: Option<T>, fallback: T) -> T {
    mode.or(defaults).unwrap_or(fallback)
}

fn choose_protection(mode: Option<bool>, legacy: Option<bool>, defaults: Option<bool>, default_legacy: Option<bool>) -> bool {
    mode.or(legacy).or(defaults).or(default_legacy).unwrap_or(false)
}

fn choose_optional<T>(mode: Option<OptionalOverride<T>>, defaults: Option<OptionalOverride<T>>, name: &str) -> Result<Option<T>, String> {
    fn apply<T>(setting: OptionalOverride<T>, name: &str) -> Result<Option<T>, String> {
        match setting {
            OptionalOverride::Value(value) => Ok(Some(value)),
            OptionalOverride::Clear(false) => Ok(None),
            OptionalOverride::Clear(true) => Err(format!("{name}: only false clears an optional setting")),
        }
    }
    match mode {
        Some(setting) => apply(setting, name),
        None => defaults.map_or(Ok(None), |setting| apply(setting, name)),
    }
}

fn merge_ghost(mode: Option<OptionalOverride<GhostInput>>, defaults: Option<OptionalOverride<GhostInput>>) -> Result<Option<GhostSettings>, String> {
    fn apply(setting: Option<OptionalOverride<GhostInput>>, base: Option<GhostSettings>) -> Result<Option<GhostSettings>, String> {
        match setting {
            None => Ok(base),
            Some(OptionalOverride::Clear(false)) => Ok(None),
            Some(OptionalOverride::Clear(true)) => Err("ghost: only false clears an optional setting".into()),
            Some(OptionalOverride::Value(input)) => Ok(Some(input.resolve(base.unwrap_or_default()))),
        }
    }

    let base = apply(defaults, None)?;
    apply(mode, base)
}

fn merge_afterburn(
    mode: Option<OptionalOverride<AfterburnInput>>,
    defaults: Option<OptionalOverride<AfterburnInput>>,
) -> Result<Option<AfterburnSettings>, String> {
    fn apply(setting: Option<OptionalOverride<AfterburnInput>>, base: Option<AfterburnSettings>) -> Result<Option<AfterburnSettings>, String> {
        match setting {
            None => Ok(base),
            Some(OptionalOverride::Clear(false)) => Ok(None),
            Some(OptionalOverride::Clear(true)) => Err("afterburn: only false clears an optional setting".into()),
            Some(OptionalOverride::Value(input)) => Ok(Some(input.resolve(base.unwrap_or_default())?)),
        }
    }
    apply(mode, apply(defaults, None)?)
}

fn merge_ability<T, S>(
    mode: Option<OptionalOverride<T>>,
    defaults: Option<OptionalOverride<T>>,
    name: &str,
    resolve: impl Fn(T, Option<S>) -> S,
) -> Result<Option<S>, String> {
    fn resolve_default<T, S>(setting: Option<OptionalOverride<T>>, name: &str, resolve: &impl Fn(T, Option<S>) -> S) -> Result<Option<S>, String> {
        match setting {
            None | Some(OptionalOverride::Clear(false)) => Ok(None),
            Some(OptionalOverride::Clear(true)) => Err(format!("{name}: only false clears an optional setting")),
            Some(OptionalOverride::Value(input)) => Ok(Some(resolve(input, None))),
        }
    }

    let inherited = resolve_default(defaults, name, &resolve)?;
    match mode {
        None => Ok(inherited),
        Some(OptionalOverride::Clear(false)) => Ok(None),
        Some(OptionalOverride::Clear(true)) => Err(format!("{name}: only false clears an optional setting")),
        Some(OptionalOverride::Value(input)) => Ok(Some(resolve(input, inherited))),
    }
}

fn merge_ability_result<T, S>(
    mode: Option<OptionalOverride<T>>,
    defaults: Option<OptionalOverride<T>>,
    name: &str,
    resolve: impl Fn(T, Option<S>) -> Result<S, String>,
) -> Result<Option<S>, String> {
    fn resolve_input<T, S>(input: T, base: Option<S>, name: &str, resolve: &impl Fn(T, Option<S>) -> Result<S, String>) -> Result<Option<S>, String> {
        resolve(input, base).map(Some).map_err(|error| format!("{name}: {error}"))
    }
    let inherited = match defaults {
        None | Some(OptionalOverride::Clear(false)) => None,
        Some(OptionalOverride::Clear(true)) => return Err(format!("{name}: only false clears an optional setting")),
        Some(OptionalOverride::Value(input)) => resolve_input(input, None, name, &resolve)?,
    };
    match mode {
        None => Ok(inherited),
        Some(OptionalOverride::Clear(false)) => Ok(None),
        Some(OptionalOverride::Clear(true)) => Err(format!("{name}: only false clears an optional setting")),
        Some(OptionalOverride::Value(input)) => resolve_input(input, inherited, name, &resolve),
    }
}

fn merge_friendly_fire_heal(
    mode: Option<OptionalOverride<FriendlyFireHealInput>>,
    defaults: Option<OptionalOverride<FriendlyFireHealInput>>,
) -> Result<Option<FriendlyFireHealSettings>, String> {
    let inherited = match defaults {
        None | Some(OptionalOverride::Clear(false)) => None,
        Some(OptionalOverride::Clear(true)) => return Err("friendly_fire_heal: only false clears an optional setting".into()),
        Some(OptionalOverride::Value(input)) => Some(input.resolve(None).map_err(|error| format!("friendly_fire_heal: {error}"))?),
    };
    match mode {
        None => Ok(inherited),
        Some(OptionalOverride::Clear(false)) => Ok(None),
        Some(OptionalOverride::Clear(true)) => Err("friendly_fire_heal: only false clears an optional setting".into()),
        Some(OptionalOverride::Value(input)) => input.resolve(inherited).map(Some).map_err(|error| format!("friendly_fire_heal: {error}")),
    }
}

fn merge_third_person(
    mode: Option<OptionalOverride<ThirdPersonInput>>,
    defaults: Option<OptionalOverride<ThirdPersonInput>>,
) -> Result<Option<ThirdPersonSettings>, String> {
    fn apply(setting: Option<OptionalOverride<ThirdPersonInput>>, base: Option<ThirdPersonSettings>) -> Result<Option<ThirdPersonSettings>, String> {
        match setting {
            None => Ok(base),
            Some(OptionalOverride::Clear(false)) => Ok(None),
            Some(OptionalOverride::Clear(true)) => Err("third_person: only false clears an optional setting".into()),
            Some(OptionalOverride::Value(input)) => Ok(Some(input.resolve(base.unwrap_or_default()))),
        }
    }

    apply(mode, apply(defaults, None)?)
}

fn merge_bomb_patch(patch: BombInput, base: Option<BombSettings>) -> BombSettings {
    let kind = patch
        .kind
        .or_else(|| base.as_ref().map(|settings| settings.kind))
        .unwrap_or(BombKind::Native);
    let base = base.filter(|settings| settings.kind == kind).unwrap_or_default();
    BombSettings {
        kind,
        plant_time: patch.plant_time.unwrap_or(base.plant_time),
        explode_time: patch.explode_time.unwrap_or(base.explode_time),
        defuse_kit_time: patch.defuse_kit_time.unwrap_or(base.defuse_kit_time),
        defuse_time: patch.defuse_time.unwrap_or(base.defuse_time),
        bounce: patch.bounce.unwrap_or(base.bounce),
    }
}

fn merge_bomb(mode: Option<OptionalOverride<BombInput>>, defaults: Option<OptionalOverride<BombInput>>) -> Result<Option<BombSettings>, String> {
    fn default_value(setting: Option<OptionalOverride<BombInput>>) -> Result<Option<BombSettings>, String> {
        match setting {
            None | Some(OptionalOverride::Clear(false)) => Ok(None),
            Some(OptionalOverride::Clear(true)) => Err("bomb: only false clears an optional setting".into()),
            Some(OptionalOverride::Value(patch)) => Ok(Some(merge_bomb_patch(patch, None))),
        }
    }

    match mode {
        None => default_value(defaults),
        Some(OptionalOverride::Clear(false)) => Ok(None),
        Some(OptionalOverride::Clear(true)) => Err("bomb: only false clears an optional setting".into()),
        Some(OptionalOverride::Value(patch)) => {
            let base = default_value(defaults)?;
            Ok(Some(merge_bomb_patch(patch, base)))
        }
    }
}

fn merge_respawn_patch(patch: RespawnInput, base: Option<&RespawnInput>) -> Result<RespawnSettings, String> {
    let team = patch
        .team
        .or_else(|| base.and_then(|settings| settings.team))
        .ok_or("respawn.team is required")?;
    let delay_seconds = patch
        .delay_seconds
        .or_else(|| base.and_then(|settings| settings.delay_seconds))
        .unwrap_or_else(default_respawn_delay);
    Ok(RespawnSettings { team, delay_seconds })
}

fn merge_respawn(
    mode: Option<OptionalOverride<RespawnInput>>,
    defaults: Option<OptionalOverride<RespawnInput>>,
) -> Result<Option<RespawnSettings>, String> {
    fn default_patch(setting: Option<OptionalOverride<RespawnInput>>) -> Result<Option<RespawnInput>, String> {
        match setting {
            None | Some(OptionalOverride::Clear(false)) => Ok(None),
            Some(OptionalOverride::Clear(true)) => Err("respawn: only false clears an optional setting".into()),
            Some(OptionalOverride::Value(patch)) => Ok(Some(patch)),
        }
    }

    match mode {
        None => default_patch(defaults)?.map(|patch| merge_respawn_patch(patch, None)).transpose(),
        Some(OptionalOverride::Clear(false)) => Ok(None),
        Some(OptionalOverride::Clear(true)) => Err("respawn: only false clears an optional setting".into()),
        Some(OptionalOverride::Value(patch)) => {
            let base = default_patch(defaults)?;
            Ok(Some(merge_respawn_patch(patch, base.as_ref())?))
        }
    }
}

fn merge_self_damage_patch(patch: SelfDamageInput, base: Option<SelfDamageInput>) -> Result<SelfDamage, String> {
    let kind = patch.kind.or_else(|| base.and_then(|defaults| defaults.kind));
    let changed_kind = patch
        .kind
        .zip(base.and_then(|defaults| defaults.kind))
        .is_some_and(|(kind, default_kind)| kind != default_kind);
    let amount = if changed_kind {
        patch.amount
    } else {
        patch.amount.or_else(|| base.and_then(|defaults| defaults.amount))
    };

    match kind.ok_or("self_damage.kind is required unless inherited from defaults")? {
        SelfDamageKind::MissPenalty => {
            if amount.is_some() {
                return Err("self_damage.miss_penalty does not accept amount".into());
            }
            Ok(SelfDamage::MissPenalty)
        }
        SelfDamageKind::Native => amount
            .map(|amount| SelfDamage::Native { amount })
            .ok_or_else(|| "self_damage.native requires amount unless inherited from native defaults".into()),
    }
}

fn merge_self_damage(
    mode: Option<OptionalOverride<SelfDamageInput>>,
    defaults: Option<OptionalOverride<SelfDamageInput>>,
) -> Result<Option<SelfDamage>, String> {
    fn default_patch(setting: Option<OptionalOverride<SelfDamageInput>>) -> Result<Option<SelfDamageInput>, String> {
        match setting {
            None | Some(OptionalOverride::Clear(false)) => Ok(None),
            Some(OptionalOverride::Clear(true)) => Err("self_damage: only false clears an optional setting".into()),
            Some(OptionalOverride::Value(patch)) => Ok(Some(patch)),
        }
    }

    match mode {
        None => default_patch(defaults)?.map(|patch| merge_self_damage_patch(patch, None)).transpose(),
        Some(OptionalOverride::Clear(false)) => Ok(None),
        Some(OptionalOverride::Clear(true)) => Err("self_damage: only false clears an optional setting".into()),
        Some(OptionalOverride::Value(patch)) => merge_self_damage_patch(patch, default_patch(defaults)?).map(Some),
    }
}

impl ModeInput {
    fn resolve_weapons(&mut self, weapon_groups: &BTreeMap<String, Vec<String>>, owner: &str) -> Result<(), String> {
        let Some(selection) = self.weapons.take() else { return Ok(()) };
        self.weapons = Some(WeaponSelection::List(match selection {
            WeaponSelection::List(weapons) => weapons,
            WeaponSelection::Group(reference) => {
                let Some(name) = reference.strip_prefix('@') else {
                    return Err(format!("{owner}.weapons must be an array or an '@group' reference"));
                };
                weapon_groups
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("{owner}.weapons references unknown weapon group '{name}'"))?
            }
        }));
        Ok(())
    }

    fn validate_native_effect_inputs(&self) -> Result<(), String> {
        if let Some(setting) = &self.periodic_speed {
            setting.clone().resolve("periodic_speed")?;
        }
        if let Some(setting) = &self.periodic_gravity {
            setting.clone().resolve("periodic_gravity")?;
        }
        if let Some(damage) = &self.weapon_damage {
            damage.validate()?;
        }
        if matches!(&self.inferno_damage, Some(OptionalOverride::Clear(true))) {
            return Err("inferno_damage: only false clears an optional setting".into());
        }
        if matches!(&self.inferno_damage, Some(OptionalOverride::Value(value)) if !value.is_finite() || *value < 0.0) {
            return Err("inferno_damage must be finite and nonnegative".into());
        }
        if matches!(&self.teamkill_limit, Some(OptionalOverride::Clear(true))) {
            return Err("teamkill_limit: only false clears an optional setting".into());
        }
        if matches!(&self.teamkill_limit, Some(OptionalOverride::Value(0))) {
            return Err("teamkill_limit must be a positive integer".into());
        }
        if matches!(&self.hegrenade_impulse, Some(OptionalOverride::Clear(true))) {
            return Err("hegrenade_impulse: only false clears an optional setting".into());
        }
        if let Some(OptionalOverride::Value(input)) = &self.hegrenade_impulse {
            HeGrenadeImpulseInput { impulse: input.impulse }
                .resolve(None)
                .validate()
                .map_err(str::to_owned)?;
        }
        if matches!(&self.grapple, Some(OptionalOverride::Clear(true))) {
            return Err("grapple: only false clears an optional setting".into());
        }
        if matches!(&self.he_guidance, Some(OptionalOverride::Clear(true))) {
            return Err("he_guidance: only false clears an optional setting".into());
        }
        if let Some(OptionalOverride::Value(input)) = &self.grapple {
            input.clone().resolve(None).validate().map_err(str::to_owned)?;
        }
        if let Some(OptionalOverride::Value(input)) = &self.he_guidance {
            input.clone().resolve(None).validate().map_err(str::to_owned)?;
        }
        Ok(())
    }

    fn validate_hud(&self) -> Result<(), String> {
        let Some(hud) = &self.hide_hud else { return Ok(()) };
        if self.hide_radar.is_some() {
            return Err("hide_hud and legacy hide_radar cannot be set in the same section".into());
        }
        if let Some(delay) = &self.hide_crosshair_after {
            if !hud.contains(&HudElement::Crosshair) {
                return Err("hide_crosshair_after requires crosshair in hide_hud".into());
            }
            if !matches!(delay, OptionalOverride::Value(value) if value.is_finite() && *value >= 0.0) {
                return Err("hide_crosshair_after must be a finite nonnegative delay with hide_hud".into());
            }
        }
        Ok(())
    }

    fn into_mode(self, defaults: &Self) -> Result<Mode, String> {
        self.validate_hud()?;
        let id = self.id.ok_or("mode id is required")?;
        let enabled = self.enabled.ok_or_else(|| format!("{id}: enabled is required"))?;
        let name = resolve_mode_label(self.name, self.name_key.as_deref(), &id, "name")?;
        let description = resolve_mode_label(self.description, self.description_key.as_deref(), &id, "description")?;
        let kill_loadout = choose(self.kill_loadout, defaults.kill_loadout, false);
        let kill_loadout_start = self.kill_loadout_start.or(if kill_loadout { defaults.kill_loadout_start } else { None });
        let mut weapon_damage = defaults.weapon_damage.clone().unwrap_or_default();
        if let Some(overrides) = self.weapon_damage {
            weapon_damage.0.extend(overrides.0);
        }
        let armor = choose(self.armor, defaults.armor, AttributeValue::Fixed(0));
        let helmet = self.helmet.or(defaults.helmet).unwrap_or(match armor {
            AttributeValue::Fixed(value) => HelmetValue::Fixed(value > 0),
            AttributeValue::Range(_) => HelmetValue::FromArmor,
        });
        let spawn_protection = choose_protection(
            self.spawn_protection,
            self.combat_protection,
            defaults.spawn_protection,
            defaults.combat_protection,
        );
        let kill_protection = choose_protection(
            self.kill_protection,
            self.combat_protection,
            defaults.kill_protection,
            defaults.combat_protection,
        );
        let weapons = resolved_weapon_list(self.weapons, &id)?;
        let default_weapons = resolved_weapon_list(defaults.weapons.clone(), &id)?;
        Ok(Mode {
            id,
            enabled,
            name,
            description,
            name_key: self.name_key,
            description_key: self.description_key,
            weapons: choose(weapons, default_weapons, Vec::new()),
            health: choose(self.health, defaults.health, AttributeValue::Fixed(100)),
            armor,
            gravity: choose(self.gravity, defaults.gravity, 1.0),
            speed: choose(self.speed, defaults.speed, 1.0),
            helmet,
            clear_weapons: choose(self.clear_weapons, defaults.clear_weapons, TeamFilter::None),
            give_weapons: choose(self.give_weapons, defaults.give_weapons, TeamFilter::None),
            give_knife: choose(self.give_knife, defaults.give_knife, TeamFilter::None),
            color: choose_optional(self.color, defaults.color.clone(), "color")?,
            headshots: choose(self.headshots, defaults.headshots, Headshots::Normal),
            ammo: choose(self.ammo, defaults.ammo, Ammo::Inherit),
            self_damage: merge_self_damage(self.self_damage, defaults.self_damage.clone())?,
            weapon_damage,
            taser_charge: choose_optional(self.taser_charge, defaults.taser_charge.clone(), "taser_charge")?,
            bunnyhop: self.bunnyhop.or(defaults.bunnyhop),
            autostrafe: choose(self.autostrafe, defaults.autostrafe, false),
            friction: choose_optional(self.friction, defaults.friction.clone(), "friction")?,
            time_scale: choose_optional(self.time_scale, defaults.time_scale.clone(), "time_scale")?,
            ground_acceleration: choose_optional(self.ground_acceleration, defaults.ground_acceleration.clone(), "ground_acceleration")?,
            ground_reverse: choose_optional(self.ground_reverse, defaults.ground_reverse.clone(), "ground_reverse")?,
            no_spread: self.no_spread.or(defaults.no_spread),
            no_recoil: choose(self.no_recoil, defaults.no_recoil, false),
            recoil_view: choose_optional(self.recoil_view, defaults.recoil_view.clone(), "recoil_view")?,
            fast_switch: choose(self.fast_switch, defaults.fast_switch, false),
            replenish: choose(self.replenish, defaults.replenish.clone(), Vec::new()),
            random_loadout: choose(self.random_loadout, defaults.random_loadout, false),
            suicide: choose(self.suicide, defaults.suicide, false),
            death_explosion: choose(self.death_explosion, defaults.death_explosion, DeathExplosion::default()),
            random_damage: choose(self.random_damage, defaults.random_damage, false),
            kill_loadout,
            kill_loadout_start,
            lifesteal: choose(self.lifesteal, defaults.lifesteal, false),
            reflect_damage: choose_optional(self.reflect_damage, defaults.reflect_damage.clone(), "reflect_damage")?,
            energy_shield: choose(self.energy_shield, defaults.energy_shield, false),
            spawn_protection,
            kill_protection,
            translucent: choose(self.translucent, defaults.translucent, false),
            ghost: merge_ghost(self.ghost, defaults.ghost.clone())?,
            afterburn: merge_afterburn(self.afterburn, defaults.afterburn.clone())?,
            third_person: merge_third_person(self.third_person, defaults.third_person.clone())?,
            delayed_damage: choose(self.delayed_damage, defaults.delayed_damage, false),
            win_condition: choose(self.win_condition, defaults.win_condition, WinCondition::Native),
            bomb: merge_bomb(self.bomb, defaults.bomb.clone())?,
            dodgeball: choose(self.dodgeball, defaults.dodgeball, false),
            throwing_knives: choose(self.throwing_knives, defaults.throwing_knives, false),
            view_punch_decay: choose_optional(self.view_punch_decay, defaults.view_punch_decay.clone(), "view_punch_decay")?,
            no_scope: choose(self.no_scope, defaults.no_scope, false),
            fov: choose_optional(self.fov, defaults.fov.clone(), "fov")?,
            hide_crosshair_after: {
                let explicit_delay = self.hide_crosshair_after.is_some();
                let delay = choose_optional(self.hide_crosshair_after, defaults.hide_crosshair_after.clone(), "hide_crosshair_after")?;
                let hud = self.hide_hud.as_ref().or(if explicit_delay { None } else { defaults.hide_hud.as_ref() });
                hud.map_or(delay, |selection| {
                    selection.contains(&HudElement::Crosshair).then_some(delay.unwrap_or(1.0))
                })
            },
            health_damage: choose(self.health_damage, defaults.health_damage, false),
            health_speed: choose(self.health_speed, defaults.health_speed, false),
            health_decay: choose(self.health_decay, defaults.health_decay, false),
            health_curve: self.health_curve.unwrap_or_default().resolve(defaults.health_curve.as_ref()),
            health_decay_settings: self.health_decay_settings.unwrap_or_default().resolve(defaults.health_decay_settings),
            self_push: choose(self.self_push, defaults.self_push, false),
            periodic_speed: choose_periodic(self.periodic_speed, defaults.periodic_speed.clone(), "periodic_speed")?,
            periodic_gravity: choose_periodic(self.periodic_gravity, defaults.periodic_gravity.clone(), "periodic_gravity")?,
            c4_count: choose_optional(self.c4_count, defaults.c4_count.clone(), "c4_count")?,
            c4_buy: choose(self.c4_buy, defaults.c4_buy, false),
            round_respawn: self
                .round_respawn
                .or(defaults.round_respawn)
                .map(RoundRespawnInput::resolve)
                .unwrap_or_default(),
            hide_all_hud: self
                .hide_hud
                .as_ref()
                .or(defaults.hide_hud.as_ref())
                .is_some_and(|selection| selection.contains(&HudElement::All)),
            hide_radar: self.hide_hud.as_ref().map_or_else(
                || {
                    self.hide_radar.or_else(|| {
                        defaults
                            .hide_hud
                            .as_ref()
                            .map_or(defaults.hide_radar, |selection| selection.contains(&HudElement::Radar).then_some(true))
                    })
                },
                |selection| selection.contains(&HudElement::Radar).then_some(true),
            ),
            hide_player_identification: self.hide_player_identification.or(defaults.hide_player_identification),
            respawn: merge_respawn(self.respawn, defaults.respawn.clone())?,
            buy_time: self.buy_time.or(defaults.buy_time),
            buy_limit: self.buy_limit.or(defaults.buy_limit),
            round_money_floor: choose_optional(self.round_money_floor, defaults.round_money_floor.clone(), "round_money_floor")?,
            money_income: choose_optional(self.money_income, defaults.money_income.clone(), "money_income")?,
            buy_access: choose_optional(self.buy_access, defaults.buy_access.clone(), "buy_access")?,
            buy_anywhere: choose_optional(self.buy_anywhere, defaults.buy_anywhere.clone(), "buy_anywhere")?,
            friendly_fire_scale: self.friendly_fire_scale.or(defaults.friendly_fire_scale),
            damage_multiplier: choose_optional(self.damage_multiplier, defaults.damage_multiplier.clone(), "damage_multiplier")?.unwrap_or(1.0),
            explosive_bullets: merge_ability(
                self.explosive_bullets,
                defaults.explosive_bullets.clone(),
                "explosive_bullets",
                ExplosiveBulletsInput::resolve,
            )?,
            friendly_fire_heal: merge_friendly_fire_heal(self.friendly_fire_heal, defaults.friendly_fire_heal.clone())?,
            inferno_damage: choose_optional(self.inferno_damage, defaults.inferno_damage.clone(), "inferno_damage")?,
            teamkill_limit: choose_optional(self.teamkill_limit, defaults.teamkill_limit.clone(), "teamkill_limit")?,
            hegrenade_impulse: merge_ability(
                self.hegrenade_impulse,
                defaults.hegrenade_impulse.clone(),
                "hegrenade_impulse",
                HeGrenadeImpulseInput::resolve,
            )?,
            proximity_fuse: merge_ability_result(
                self.proximity_fuse,
                defaults.proximity_fuse.clone(),
                "proximity_fuse",
                ProximityFuseInput::resolve,
            )?,
            wall_hang: choose(self.wall_hang, defaults.wall_hang, false),
            grapple: merge_ability(self.grapple, defaults.grapple.clone(), "grapple", |input, base| input.resolve(base))?,
            he_guidance: merge_ability(self.he_guidance, defaults.he_guidance.clone(), "he_guidance", |input, base| {
                input.resolve(base)
            })?,
            teammates_as_enemies: self.teammates_as_enemies.or(defaults.teammates_as_enemies),
            random_spawn: self.random_spawn.or(defaults.random_spawn),
            drop_weapons: self.drop_weapons.or(defaults.drop_weapons),
            drop_grenades: self.drop_grenades.or(defaults.drop_grenades),
            shoot_dropped_grenades: self.shoot_dropped_grenades.or(defaults.shoot_dropped_grenades),
            fall_damage_scale: self.fall_damage_scale.or(defaults.fall_damage_scale),
            ordinary_chickens: choose(self.ordinary_chickens, defaults.ordinary_chickens, true),
            chicken_eating: self.chicken_eating.or(defaults.chicken_eating),
            regeneration: merge_ability_result(
                self.regeneration,
                defaults.regeneration.clone(),
                "regeneration",
                RegenerationInput::resolve,
            )?,
            chicken_aura: merge_ability_result(
                self.chicken_aura,
                defaults.chicken_aura.clone(),
                "chicken_aura",
                ChickenAuraInput::resolve,
            )?,
            medical_smoke: choose(self.medical_smoke, defaults.medical_smoke, false),
            trampoline: choose(self.trampoline, defaults.trampoline, false),
            healing_cap_base_multiple: choose(self.healing_cap_base_multiple, defaults.healing_cap_base_multiple, 1.0),
            dont_stop: merge_ability(self.dont_stop, defaults.dont_stop.clone(), "dont_stop", DontStopInput::resolve)?,
            hot_floor: merge_ability(self.hot_floor, defaults.hot_floor.clone(), "hot_floor", HotFloorInput::resolve)?,
            weapon_on_fire: merge_ability(
                self.weapon_on_fire,
                defaults.weapon_on_fire.clone(),
                "weapon_on_fire",
                WeaponOnFireInput::resolve,
            )?,
            long_flash: merge_ability(self.long_flash, defaults.long_flash.clone(), "long_flash", LongFlashInput::resolve)?,
            big_head: merge_ability(self.big_head, defaults.big_head.clone(), "big_head", BigHeadInput::resolve)?,
        })
    }
}

impl Mode {
    pub fn localized_name(&self, bundle: &TextBundle, language: &str) -> Result<String, String> {
        if !self.name.is_empty() {
            return Ok(self.name.clone());
        }
        self.name_key
            .as_deref()
            .map_or_else(|| Ok(self.name.clone()), |key| bundle.render(language, key, &[]))
    }

    pub fn localized_description(&self, bundle: &TextBundle, language: &str) -> Result<String, String> {
        if !self.description.is_empty() {
            return Ok(self.description.clone());
        }
        self.description_key
            .as_deref()
            .map_or_else(|| Ok(self.description.clone()), |key| bundle.render(language, key, &[]))
    }

    pub fn localized_chat(&self, bundle: &TextBundle, language: &str) -> Result<[String; 5], String> {
        let id = bundle.render(language, "mode.chat.id", &[("id", self.id.as_str())])?;
        Ok([
            " \u{4}----------------------------------------".into(),
            format!(" \u{4}{}", self.localized_name(bundle, language)?),
            format!(" \u{4}{id}"),
            format!(" \u{4}{}", self.localized_description(bundle, language)?),
            " \u{4}----------------------------------------".into(),
        ])
    }

    pub fn localized_console(&self, bundle: &TextBundle, language: &str) -> Result<String, String> {
        bundle.render(
            language,
            "mode.console",
            &[("name", &self.localized_name(bundle, language)?), ("id", &self.id)],
        )
    }
}

fn resolve_mode_label(value: Option<String>, key: Option<&str>, id: &str, field: &str) -> Result<String, String> {
    match value {
        Some(value) => {
            if value.trim().is_empty() || value.contains(['\r', '\n', '\0']) {
                return Err(format!("{id}: explicit {field} must be nonempty single-line text"));
            }
            Ok(value)
        }
        None if key.is_some() => Ok(String::new()),
        None => Err(format!("{id}: {field} or {field}_key is required")),
    }
}

impl Config {
    pub fn parse_with_bundle(source: &str, bundle: &TextBundle) -> Result<Self, String> {
        let config = Self::parse(source)?;
        bundle.validate_config(&config)?;
        Ok(config)
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        let mut input: RawConfig = toml::from_str(source).map_err(|error| {
            if error.message().contains("unknown field `he_shockwave`") {
                format!("{error}\nUse hegrenade_impulse and weapon_damage.hegrenade = 0.0 for the former impulse-only behavior.")
            } else {
                error.to_string()
            }
        })?;
        if input.defaults.id.is_some()
            || input.defaults.enabled.is_some()
            || input.defaults.name.is_some()
            || input.defaults.description.is_some()
            || input.defaults.name_key.is_some()
            || input.defaults.description_key.is_some()
        {
            return Err("[defaults] may contain capability settings only".into());
        }
        for (name, weapons) in &input.weapon_groups {
            if let Some(item) = weapons.iter().find(|item| !supported_equipment_id(item)) {
                return Err(equipment_input_error(&format!("weapon_groups.{name}"), item));
            }
        }
        input.defaults.validate_hud()?;
        input.defaults.validate_native_effect_inputs()?;
        input.defaults.resolve_weapons(&input.weapon_groups, "defaults")?;
        let defaults = input.defaults.clone();
        let warmup = input.warmup.map(|warmup| warmup.into_config(&input.weapon_groups)).transpose()?;
        let chicken = input.chicken.map(ChickenInput::resolve).transpose()?;
        let healthshot = input.healthshot.map(HealthshotInput::resolve).transpose()?;
        let default_language = input.i18n.default_language;
        text::validate_language(&default_language).map_err(|error| format!("i18n.default_language: {error}"))?;
        let modes = std::mem::take(&mut input.modes)
            .into_iter()
            .map(|mut mode| {
                let owner = mode.id.clone().unwrap_or_else(|| "mode".into());
                mode.resolve_weapons(&input.weapon_groups, &owner)?;
                mode.into_mode(&defaults)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let config = Self {
            default_language,
            diagnostics: input.diagnostics,
            hostage_rescue_compat: input.hostage_rescue_compat,
            prices: input.prices,
            warmup,
            chicken,
            healthshot,
            modes,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(chicken) = self.chicken {
            chicken.validate()?;
        }
        if let Some(healthshot) = self.healthshot {
            healthshot.validate()?;
        }
        let mut ids = HashSet::new();
        for mode in &self.modes {
            if !ids.insert(&mode.id) {
                return Err(format!("duplicate mode id: {}", mode.id));
            }
        }
        for mode in self.modes.iter().chain(self.warmup.iter().map(|warmup| &warmup.mode)) {
            if !mode.healing_cap_base_multiple.is_finite() || mode.healing_cap_base_multiple <= 0.0 {
                return Err(format!("{}: healing_cap_base_multiple must be finite and positive", mode.id));
            }
            if let Some(settings) = mode.regeneration {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.chicken_aura {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if !mode.damage_multiplier.is_finite() || mode.damage_multiplier < 0.0 {
                return Err(format!("{}: damage_multiplier must be finite and nonnegative", mode.id));
            }
            if mode.inferno_damage.is_some_and(|damage| !damage.is_finite() || damage < 0.0) {
                return Err(format!("{}: inferno_damage must be finite and nonnegative", mode.id));
            }
            if mode.teamkill_limit == Some(0) {
                return Err(format!("{}: teamkill_limit must be a positive integer", mode.id));
            }
            if let Some(settings) = mode.explosive_bullets {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.friendly_fire_heal {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.hegrenade_impulse {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = &mode.proximity_fuse {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.grapple {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.he_guidance {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.dont_stop {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.hot_floor {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.weapon_on_fire {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.long_flash {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(settings) = mode.big_head {
                settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(afterburn) = mode.afterburn {
                afterburn.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            if let Some(third_person) = mode.third_person {
                third_person.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            }
            mode.health_curve.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            mode.health_decay_settings.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            if let Some(ghost) = mode.ghost {
                ghost.validate().map_err(|error| format!("{}: {error}", mode.id))?;
                if mode.translucent {
                    return Err(format!("{}: ghost and translucent cannot be combined", mode.id));
                }
            }
            if mode.id.is_empty() || !mode.id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
                return Err("mode id must contain ASCII letters, digits, '_' or '-'".into());
            }
            if (mode.name.trim().is_empty() && mode.name_key.is_none()) || (mode.description.trim().is_empty() && mode.description_key.is_none()) {
                return Err(format!(
                    "{}: name or name_key and description or description_key must not be empty",
                    mode.id
                ));
            }
            if mode.name.contains(['\r', '\n']) || mode.description.contains(['\r', '\n']) {
                return Err(format!("{}: name and description must each fit one line", mode.id));
            }
            if !mode.health.validate(1, 10000)
                || !mode.armor.validate(0, 1000)
                || !mode.gravity.is_finite()
                || mode.gravity <= 0.0
                || mode.gravity > 10.0
            {
                return Err(format!("{}: invalid health, armor or gravity", mode.id));
            }
            if mode.weapons.iter().any(|weapon| !supported_equipment_id(weapon)) {
                let weapon = mode
                    .weapons
                    .iter()
                    .find(|weapon| !supported_equipment_id(weapon))
                    .expect("invalid item found");
                return Err(equipment_input_error(&mode.id, weapon));
            }
            if !mode.speed.is_finite() || mode.speed <= 0.0 || mode.speed > 10.0 {
                return Err(format!("{}: speed must be finite, positive and at most 10", mode.id));
            }
            if mode.taser_charge.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: taser charge must be finite and non-negative", mode.id));
            }
            if mode.taser_charge.is_some() && !mode.weapons.iter().any(|weapon| weapon == "taser") {
                return Err(format!("{}: taser charge requires a taser equipment candidate", mode.id));
            }
            if mode.random_loadout && mode.kill_loadout {
                return Err(format!("{}: random and kill loadouts cannot be combined", mode.id));
            }
            if (mode.random_loadout || mode.kill_loadout) && mode.weapons.is_empty() {
                return Err(format!("{}: random and kill loadouts require candidates", mode.id));
            }
            if mode.kill_loadout_start.is_some() && !mode.kill_loadout {
                return Err(format!("{}: kill_loadout_start requires kill_loadout", mode.id));
            }
            if mode.kill_loadout
                && mode.kill_loadout_start.unwrap_or(KillLoadoutStart::Glock) == KillLoadoutStart::Glock
                && !mode.weapons.iter().any(|weapon| weapon == "glock")
            {
                return Err(format!("{}: kill loadout candidates must include the initial Glock", mode.id));
            }
            if mode.friction.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: friction must be finite and non-negative", mode.id));
            }
            if mode.time_scale.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                return Err(format!("{}: time scale must be finite and positive", mode.id));
            }
            if mode.ground_acceleration.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: ground acceleration must be finite and non-negative", mode.id));
            }
            if mode.ground_reverse.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                return Err(format!("{}: ground reverse acceleration must be finite and positive", mode.id));
            }
            if mode.ground_reverse.is_some() && mode.ground_acceleration.is_some_and(|value| value != 0.0) {
                return Err(format!(
                    "{}: ground_reverse requires zero global ground_acceleration; clear it or set it to zero",
                    mode.id
                ));
            }
            if mode.recoil_view.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: view recoil must be finite and non-negative", mode.id));
            }
            if mode.view_punch_decay.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: view-punch decay must be finite and non-negative", mode.id));
            }
            if mode.fov.is_some_and(|value| !(1..=179).contains(&value)) {
                return Err(format!("{}: FOV must be between 1 and 179", mode.id));
            }
            if mode.hide_crosshair_after.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: crosshair hide delay must be finite and non-negative", mode.id));
            }
            if mode.bomb.as_ref().is_some_and(|bomb| {
                [bomb.plant_time, bomb.explode_time, bomb.defuse_kit_time, bomb.defuse_time]
                    .iter()
                    .any(|value| !value.is_finite() || *value < 0.0)
            }) {
                return Err(format!("{}: bomb times must be finite and non-negative", mode.id));
            }
            mode.weapon_damage.validate().map_err(|error| format!("{}: {error}", mode.id))?;
            if matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if !amount.is_finite() || amount < 0.0)
                || mode.reflect_damage.is_some_and(|value| !value.is_finite() || value < 0.0)
            {
                return Err(format!("{}: self and reflected damage must be finite and non-negative", mode.id));
            }
            if mode.friendly_fire_scale.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: friendly fire scale must be finite and nonnegative", mode.id));
            }
            if mode.fall_damage_scale.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: fall damage scale must be finite and non-negative", mode.id));
            }
            if mode.buy_time.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: buy time must be finite and non-negative", mode.id));
            }
            if mode
                .money_income
                .is_some_and(|income| !income.interval.is_finite() || income.interval <= 0.0 || income.amount == 0)
            {
                return Err(format!("{}: money income needs a positive finite interval and positive amount", mode.id));
            }
            if mode.dodgeball && !mode.weapons.iter().any(|weapon| weapon == "decoy") {
                return Err(format!("{}: dodgeball requires decoy equipment", mode.id));
            }
            if mode.throwing_knives && mode.give_knife == TeamFilter::None {
                return Err(format!("{}: throwing knives require a configured knife recipient", mode.id));
            }
            if mode
                .replenish
                .iter()
                .any(|kind| !mode.weapons.iter().any(|weapon| weapon == kind.item_id()))
            {
                return Err(format!("{}: replenishment requires each grenade in the equipment candidates", mode.id));
            }
            if mode
                .respawn
                .is_some_and(|respawn| respawn.team == TeamFilter::None || !respawn.delay_seconds.is_finite() || respawn.delay_seconds < 0.0)
            {
                return Err(format!("{}: respawn needs a team and finite non-negative delay", mode.id));
            }
            if mode.win_condition == WinCondition::TeamDeathmatch && mode.teammates_as_enemies == Some(true) {
                return Err(format!("{}: team deathmatch cannot be combined with teammates as enemies", mode.id));
            }
        }
        if let Some(warmup) = &self.warmup {
            if warmup.mode.win_condition != WinCondition::Native {
                return Err("warmup: win_condition is only available in formal rounds".into());
            }
            if warmup.mode.round_respawn != RoundRespawn::Disabled {
                return Err("warmup: use respawn instead of formal round_respawn".into());
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
        assert_eq!(mode.weapons, ["famas", "p250"]);
        assert_eq!(
            (mode.health, mode.armor, mode.gravity),
            (AttributeValue::Fixed(120), AttributeValue::Fixed(50), 1.0)
        );
    }

    #[test]
    fn public_catalogs_localize_the_generic_example_with_matching_placeholders() {
        let zh = TextCatalog::parse(include_str!("../../../config/i18n/zh-CN.toml")).unwrap();
        let en = TextCatalog::parse(include_str!("../../../config/i18n/en.toml")).unwrap();
        for source in [
            include_str!("../../../config/i18n/zh-CN.toml"),
            include_str!("../../../config/i18n/en.toml"),
        ] {
            let value: toml::Value = toml::from_str(source).unwrap();
            let mut keys = value["messages"]
                .as_table()
                .unwrap()
                .keys()
                .filter(|key| key.starts_with("mode."))
                .map(String::as_str)
                .collect::<Vec<_>>();
            keys.sort_unstable();
            assert_eq!(keys, ["mode.example.description", "mode.example.name"]);
        }
        let texts = TextBundle::load("zh-CN", [("zh-CN".into(), zh), ("en".into(), en)]).unwrap();
        let config = Config::parse_with_bundle(EXAMPLE, &texts).unwrap();
        assert_eq!(config.modes[0].localized_name(&texts, "zh-CN").unwrap(), "通用示例");
        assert_eq!(config.modes[0].localized_name(&texts, "en").unwrap(), "Generic example");
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
    fn legacy_classnames_and_grenade_aliases_report_canonical_ids() {
        let source = EXAMPLE.replace("famas", "weapon_famas");
        let error = Config::parse(&source).unwrap_err();
        assert!(error.contains("canonical item ID 'famas'"), "{error}");

        let source = EXAMPLE.replace("# replenish = [\"hegrenade\"]", "replenish = [\"he\"]");
        let error = Config::parse(&source).unwrap_err();
        assert!(error.contains("canonical item ID 'hegrenade'"), "{error}");
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
            EXAMPLE.replace("famas", "shield"),
            EXAMPLE.replace(
                "weapons = [\"famas\", \"p250\"]",
                "weapons = [\"famas\", \"p250\"]\nammo = \"unsupported\"",
            ),
            EXAMPLE.replace("# view_punch_decay = 5.0", "view_punch_decay = -1.0"),
            EXAMPLE.replace(
                "clear_weapons = \"all\"",
                "clear_weapons = \"all\"\nbomb = { kind = \"native\", plant_time = -1.0 }",
            ),
            EXAMPLE.replace("clear_weapons = \"all\"", "clear_weapons = \"all\"\nthrowing_knives = true"),
        ] {
            assert!(Config::parse(&invalid).is_err(), "unexpected acceptance: {invalid}");
        }
    }

    #[test]
    fn headshot_setting_accepts_only_supported_values() {
        let invalid = EXAMPLE.replace("# headshots = \"normal\"", "headshots = \"other\"");
        assert!(Config::parse(&invalid).is_err());
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
