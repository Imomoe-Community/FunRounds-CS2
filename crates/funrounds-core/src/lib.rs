// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;
use std::collections::HashSet;

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
pub use players::{PlayerIdentity, PlayerTask, PlayerTracker};
mod rules;
pub use rules::{
    DODGEBALL_DAMAGE_MULTIPLIER, LIFESTEAL_MAX_HEALTH, MovementScale, PeriodicMovement, THROWING_KNIFE_BODY_DAMAGE, THROWING_KNIFE_ELASTICITY,
    THROWING_KNIFE_GRAVITY, THROWING_KNIFE_HEAD_DAMAGE, THROWING_KNIFE_HEADSHOT_DISTANCE, THROWING_KNIFE_LIFETIME, THROWING_KNIFE_SPEED,
    TeamDeathmatch, TeamResult, dodgeball_damage, lifesteal_recovery, low_health_finish,
};
mod health;
use health::{HealthCurveInput, HealthDecayInput};
mod text;
pub use text::{TextBundle, TextCatalog};
mod combat;
pub use combat::{CombatLife, combat_hit, weapon_damage_scale};
mod random_damage;
pub use random_damage::{RandomDamage, random_damage};
mod suicide;
pub use suicide::{SuicideSequence, suicide_blast_damage};
mod equipment;
mod money;
pub use equipment::{WeaponSlot, self_push_force, weapon_slot};
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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponDamage {
    #[serde(default = "one")]
    pub taser: f32,
    #[serde(default = "one")]
    pub knife: f32,
}

impl Default for WeaponDamage {
    fn default() -> Self {
        Self { taser: 1.0, knife: 1.0 }
    }
}

fn one() -> f32 {
    1.0
}

pub const VERSION: &str = "0.3-Alpha Test 2";

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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GrenadeKind {
    He,
    Flash,
    Smoke,
    Decoy,
    Molotov,
    Incendiary,
}

impl GrenadeKind {
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
    pub periodic_speed: bool,
    pub periodic_gravity: bool,
    pub c4_count: Option<u8>,
    pub c4_buy: bool,
    pub round_respawn: RoundRespawn,
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
    pub teammates_as_enemies: Option<bool>,
    pub random_spawn: Option<TeamFilter>,
    pub drop_weapons: Option<bool>,
    pub drop_grenades: Option<bool>,
    pub shoot_dropped_grenades: Option<bool>,
    pub fall_damage_scale: Option<f32>,
    pub ordinary_chickens: bool,
    pub chicken_eating: Option<bool>,
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
    fn into_config(mut self) -> Result<WarmupConfig, String> {
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
        let enabled = self.mode.enabled.unwrap_or(true);
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

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct WeaponDamageOverride {
    taser: Option<f32>,
    knife: Option<f32>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HudElement {
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
    weapons: Option<Vec<String>>,
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
    weapon_damage: Option<WeaponDamageOverride>,
    taser_charge: Option<OptionalOverride<f32>>,
    bunnyhop: Option<bool>,
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
    periodic_speed: Option<bool>,
    periodic_gravity: Option<bool>,
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
    teammates_as_enemies: Option<bool>,
    random_spawn: Option<TeamFilter>,
    drop_weapons: Option<bool>,
    drop_grenades: Option<bool>,
    shoot_dropped_grenades: Option<bool>,
    fall_damage_scale: Option<f32>,
    ordinary_chickens: Option<bool>,
    chicken_eating: Option<bool>,
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
        let weapon_damage = WeaponDamage {
            taser: choose(
                self.weapon_damage.as_ref().and_then(|damage| damage.taser),
                defaults.weapon_damage.as_ref().and_then(|damage| damage.taser),
                1.0,
            ),
            knife: choose(
                self.weapon_damage.as_ref().and_then(|damage| damage.knife),
                defaults.weapon_damage.as_ref().and_then(|damage| damage.knife),
                1.0,
            ),
        };
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
        Ok(Mode {
            id,
            enabled,
            name,
            description,
            name_key: self.name_key,
            description_key: self.description_key,
            weapons: choose(self.weapons, defaults.weapons.clone(), Vec::new()),
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
            periodic_speed: choose(self.periodic_speed, defaults.periodic_speed, false),
            periodic_gravity: choose(self.periodic_gravity, defaults.periodic_gravity, false),
            c4_count: choose_optional(self.c4_count, defaults.c4_count.clone(), "c4_count")?,
            c4_buy: choose(self.c4_buy, defaults.c4_buy, false),
            round_respawn: self
                .round_respawn
                .or(defaults.round_respawn)
                .map(RoundRespawnInput::resolve)
                .unwrap_or_default(),
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
            teammates_as_enemies: self.teammates_as_enemies.or(defaults.teammates_as_enemies),
            random_spawn: self.random_spawn.or(defaults.random_spawn),
            drop_weapons: self.drop_weapons.or(defaults.drop_weapons),
            drop_grenades: self.drop_grenades.or(defaults.drop_grenades),
            shoot_dropped_grenades: self.shoot_dropped_grenades.or(defaults.shoot_dropped_grenades),
            fall_damage_scale: self.fall_damage_scale.or(defaults.fall_damage_scale),
            ordinary_chickens: choose(self.ordinary_chickens, defaults.ordinary_chickens, true),
            chicken_eating: self.chicken_eating.or(defaults.chicken_eating),
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
        let mut input: RawConfig = toml::from_str(source).map_err(|error| error.to_string())?;
        if input.defaults.id.is_some()
            || input.defaults.enabled.is_some()
            || input.defaults.name.is_some()
            || input.defaults.description.is_some()
            || input.defaults.name_key.is_some()
            || input.defaults.description_key.is_some()
        {
            return Err("[defaults] may contain capability settings only".into());
        }
        input.defaults.validate_hud()?;
        let defaults = input.defaults.clone();
        let warmup = input.warmup.map(WarmupInput::into_config).transpose()?;
        let chicken = input.chicken.map(ChickenInput::resolve).transpose()?;
        let healthshot = input.healthshot.map(HealthshotInput::resolve).transpose()?;
        let default_language = input.i18n.default_language;
        if !matches!(default_language.as_str(), "zh-CN" | "en") {
            return Err(format!("i18n.default_language must be zh-CN or en, got {default_language:?}"));
        }
        let modes = std::mem::take(&mut input.modes)
            .into_iter()
            .map(|mode| mode.into_mode(&defaults))
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
            if mode
                .weapons
                .iter()
                .any(|weapon| equipment::weapon_slot(weapon).is_none() || weapon == "weapon_knife")
            {
                return Err(format!("{}: equipment must use the supported CS2 weapon whitelist", mode.id));
            }
            if !mode.speed.is_finite() || mode.speed <= 0.0 || mode.speed > 10.0 {
                return Err(format!("{}: speed must be finite, positive and at most 10", mode.id));
            }
            if mode.taser_charge.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err(format!("{}: taser charge must be finite and non-negative", mode.id));
            }
            if mode.taser_charge.is_some() && !mode.weapons.iter().any(|weapon| weapon == "weapon_taser") {
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
                && !mode.weapons.iter().any(|weapon| weapon == "weapon_glock")
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
            if !mode.weapon_damage.taser.is_finite()
                || mode.weapon_damage.taser < 0.0
                || !mode.weapon_damage.knife.is_finite()
                || mode.weapon_damage.knife < 0.0
            {
                return Err(format!("{}: weapon damage multipliers must be finite and non-negative", mode.id));
            }
            if matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if !amount.is_finite() || amount < 0.0)
                || mode.reflect_damage.is_some_and(|value| !value.is_finite() || value < 0.0)
            {
                return Err(format!("{}: self and reflected damage must be finite and non-negative", mode.id));
            }
            if mode
                .friendly_fire_scale
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            {
                return Err(format!("{}: friendly fire scale must be finite and between 0 and 1", mode.id));
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
            if mode.dodgeball && !mode.weapons.iter().any(|weapon| weapon == "weapon_decoy") {
                return Err(format!("{}: dodgeball requires decoy equipment", mode.id));
            }
            if mode.throwing_knives && mode.give_knife == TeamFilter::None {
                return Err(format!("{}: throwing knives require a configured knife recipient", mode.id));
            }
            if mode
                .replenish
                .iter()
                .any(|kind| !mode.weapons.iter().any(|weapon| weapon == kind.classname()))
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
        assert_eq!(mode.weapons, ["weapon_famas", "weapon_p250"]);
        assert_eq!(
            (mode.health, mode.armor, mode.gravity),
            (AttributeValue::Fixed(120), AttributeValue::Fixed(50), 1.0)
        );
    }

    #[test]
    fn public_catalogs_localize_the_generic_example_with_matching_placeholders() {
        let zh = TextCatalog::parse(include_str!("../../../config/i18n/zh-CN.toml")).unwrap();
        let en = TextCatalog::parse(include_str!("../../../config/i18n/en.toml")).unwrap();
        TextBundle::validate_complete_catalogs(&zh, &en).unwrap();
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
            EXAMPLE.replace("weapon_famas", "weapon_shield"),
            EXAMPLE.replace(
                "weapons = [\"weapon_famas\", \"weapon_p250\"]",
                "weapons = [\"weapon_famas\", \"weapon_p250\"]\nammo = \"unsupported\"",
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
