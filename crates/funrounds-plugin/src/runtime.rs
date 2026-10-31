// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    commands::Command,
    effects::{Effect, equipment},
    fault::{Fault, Faults, Mechanism, Recovery, Scope},
    game::{self, LivePlayer},
    parameters::{self, Parameters},
    s2sdk,
};
use funrounds_core::{
    Ammo, Config, Headshots, MoneyIncome, Phase, PlayerIdentity, PlayerTask, PlayerTracker, RoundController, RoundGeneration, RoundKey, RoundRespawn,
    SelfDamage, TeamDeathmatch,
};
use plugify::Str;
use rand::{RngExt, SeedableRng, rngs::StdRng, seq::SliceRandom};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::thread::{ThreadId, current};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    Loadout,
    Grenade(u64),
    RecoilSettle,
    RecoilObservation,
    AmmoObservation(i32),
}

pub struct EventCapabilities {
    pub grenade_thrown: bool,
    pub weapon_fire: bool,
    pub player_hurt: bool,
    pub damage: bool,
    pub chicken_damage: bool,
    pub chicken_follow_block_support: bool,
    pub combat_events: bool,
    pub bot_takeover: crate::capability::Probe,
    pub round_prestart: bool,
    pub bombs: crate::capability::Probe,
    pub bomb_plant_tracking: crate::capability::Probe,
    pub moving_blast: crate::capability::Probe,
    pub healthshot: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GrenadeThrownRoute {
    track_dodgeball_decoy: bool,
    replenish: bool,
}

#[derive(Clone, Copy)]
struct PendingTakeover {
    slot: i32,
    controller_handle: i32,
    expected: PlayerIdentity,
    round: RoundGeneration,
    event_tick: i32,
}

#[derive(Clone, Copy)]
pub(crate) struct RandomHealReceipt {
    pub target: PlayerTask,
    pub source: PlayerTask,
    pub target_live: LivePlayer,
    pub source_live: LivePlayer,
    pub frame_token: u64,
    pub amount: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeSelfDamageShot {
    task: PlayerTask,
    player: LivePlayer,
    tick: i32,
    weapon: FiredWeapon,
    player_hit: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FiredWeapon {
    handle: i32,
    classname: String,
    item_definition: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingChickenBlastKill {
    round: RoundGeneration,
    victim: PlayerIdentity,
    tick: i32,
    damage_frame_token: Option<u64>,
    source: crate::chicken_entities::KillWeaponSource,
}

impl PendingChickenBlastKill {
    fn matches(&self, round: RoundGeneration, victim: PlayerIdentity, attacker: PlayerIdentity, tick: i32, frame_token: Option<u64>) -> bool {
        self.round == round
            && self.victim == victim
            && self.source.attacker == attacker
            && self.tick == tick
            && self.damage_frame_token.is_none_or(|expected| frame_token == Some(expected))
    }
}

fn chicken_weapon_source_readback_matches(
    source: &crate::chicken_entities::KillWeaponSource,
    attacker: PlayerIdentity,
    valid: bool,
    owner: i32,
    inventory: &[i32],
    classname: &str,
    item_definition: u16,
) -> bool {
    source.attacker == attacker
        && valid
        && owner == attacker.pawn_handle as i32
        && inventory.contains(&source.handle)
        && source.classname == classname
        && source.item_definition == item_definition
}

fn tracked_upgrade_weapon(effect: &Effect, player: LivePlayer) -> Option<KillWeaponTarget> {
    let inventory = s2sdk::GetClientWeapons(player.slot);
    let (handle, classname, item_definition) = effect.upgraded_weapon().filter(|(handle, classname, expected_item_definition)| {
        s2sdk::IsValidEntHandle(*handle)
            && s2sdk::GetEntityOwner(*handle) == player.pawn_handle
            && inventory.iter().any(|owned| owned == handle)
            && s2sdk::GetEntityClassname(*handle).as_str() == classname
            && unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition.expect("required native ABI checked at startup")(*handle) }
                == *expected_item_definition
    })?;
    if !crate::effects::equipment::kill_loadout_source_weapon(&classname) {
        return None;
    }
    Some(KillWeaponTarget {
        attacker: effect.identity,
        handle,
        classname,
        item_definition,
    })
}

fn prepare_kill_loadout_upgrade(mode: &funrounds_core::Mode, rng: &mut impl rand::Rng, target: &KillWeaponTarget) -> Option<PreparedUpgrade> {
    if !crate::effects::equipment::kill_loadout_source_weapon(&target.classname) {
        return None;
    }
    let excluded = mode
        .weapons
        .iter()
        .filter(|candidate| crate::effects::equipment::kill_loadout_candidate_matches_classname(candidate, &target.classname))
        .map(String::as_str)
        .collect::<Vec<_>>();
    let weapon = mode.select_kill_weapon_excluding(rng, &excluded).ok()?.to_owned();
    Some(PreparedUpgrade {
        target: target.handle,
        weapon,
    })
}

fn kill_weapon_target_readback_matches(
    target: &KillWeaponTarget,
    attacker: PlayerIdentity,
    valid: bool,
    owner: i32,
    inventory: &[i32],
    classname: &str,
    item_definition: u16,
) -> bool {
    target.attacker == attacker
        && valid
        && owner == attacker.pawn_handle as i32
        && inventory.contains(&target.handle)
        && target.classname == classname
        && target.item_definition == item_definition
        && crate::effects::equipment::kill_loadout_source_weapon(classname)
}

fn capture_chicken_damage_weapon_source(
    damage: crate::chicken_entities::EntityDamageSource,
    players: &PlayerTracker,
    controllers: &HashMap<i32, i32>,
) -> Option<crate::chicken_entities::KillWeaponSource> {
    let item_definition_reader = unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
    for slot in 0..s2sdk::GetMaxClients() {
        let Some(player) = LivePlayer::resolve(slot) else { continue };
        if (s2sdk::GetClientTeam(slot) as i32) < 2
            || player.pawn_handle != damage.attacker
            || controllers.get(&slot) != Some(&player.controller_handle)
        {
            continue;
        }
        let Some(attacker) = players
            .identity(slot)
            .filter(|identity| identity.pawn_handle == player.pawn_handle as u32)
        else {
            continue;
        };
        let inventory = s2sdk::GetClientWeapons(slot);
        for handle in [damage.ability, damage.inflictor] {
            if handle == -1
                || !s2sdk::IsValidEntHandle(handle)
                || s2sdk::GetEntityOwner(handle) != player.pawn_handle
                || !inventory.iter().any(|owned| *owned == handle)
            {
                continue;
            }
            let classname = s2sdk::GetEntityClassname(handle).to_string();
            if !classname.starts_with("weapon_") {
                continue;
            }
            let item_definition = unsafe { item_definition_reader(handle) };
            return Some(crate::chicken_entities::KillWeaponSource {
                attacker,
                handle,
                classname,
                item_definition,
            });
        }
    }
    None
}

fn chicken_weapon_source_is_current(
    source: &crate::chicken_entities::KillWeaponSource,
    players: &PlayerTracker,
    controllers: &HashMap<i32, i32>,
) -> bool {
    let Some(player) = LivePlayer::resolve(source.attacker.slot) else {
        return false;
    };
    if players.identity(player.slot) != Some(source.attacker)
        || controllers.get(&player.slot) != Some(&player.controller_handle)
        || player.pawn_handle != source.attacker.pawn_handle as i32
    {
        return false;
    }
    let valid = s2sdk::IsValidEntHandle(source.handle);
    if !valid {
        return false;
    }
    let owner = s2sdk::GetEntityOwner(source.handle);
    let inventory = s2sdk::GetClientWeapons(player.slot).iter().copied().collect::<Vec<_>>();
    let classname = s2sdk::GetEntityClassname(source.handle).to_string();
    let item_definition_reader = unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
    let item_definition = unsafe { item_definition_reader(source.handle) };
    chicken_weapon_source_readback_matches(source, source.attacker, valid, owner, &inventory, &classname, item_definition)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OwnedFloat {
    previous: f32,
    installed: f32,
}

#[derive(Clone, Debug)]
struct WeaponInventory {
    player: LivePlayer,
    identity: PlayerIdentity,
    weapons: HashMap<i32, String>,
}

impl NativeSelfDamageShot {
    fn matches_hit(&self, task: PlayerTask, player: LivePlayer, tick: i32, damage: &crate::damage::Damage) -> bool {
        self.task == task
            && self.player == player
            && self.tick == tick
            && self.weapon.handle == damage.weapon
            && damage.weapon_class.as_deref() == Some(self.weapon.classname.as_str())
    }

    fn mark_player_hit(&mut self, task: PlayerTask, player: LivePlayer, tick: i32, damage: &crate::damage::Damage) -> bool {
        if !self.matches_hit(task, player, tick, damage) {
            return false;
        }
        self.player_hit = true;
        true
    }
}

fn native_self_damage_signature(damage: &crate::damage::Damage, player: LivePlayer) -> bool {
    !damage.skip_plugin_rules
        && damage.victim == player.pawn_handle
        && damage.attacker == player.pawn_handle
        && damage.inflictor == player.pawn_handle
        && damage.weapon == -1
        && damage.weapon_kind == 0
        && damage.damage_type == 0
        && damage.amount.is_finite()
        && damage.amount > 0.0
}

fn round_end_finish_valid(
    enabled: bool,
    phase: Phase,
    task: PlayerTask,
    player: LivePlayer,
    current_identity: Option<PlayerIdentity>,
    live_player: Option<LivePlayer>,
) -> bool {
    enabled && phase == Phase::Idle && current_identity == Some(task.player) && live_player == Some(player)
}

fn round_end_finish_due(tick: i32, queued_tick: i32) -> bool {
    tick > queued_tick
}

fn tracks_chicken_life(phase: Phase) -> bool {
    matches!(phase, Phase::Preparing | Phase::Running | Phase::Warmup)
}

fn active_combat_phase(phase: Phase) -> bool {
    matches!(phase, Phase::Running | Phase::Warmup)
}

fn damageable_chicken(handle: i32) -> bool {
    const BASE_ENTITY: &str = "CBaseEntity";

    if !s2sdk::IsValidEntHandle(handle) || s2sdk::GetEntityClassname(handle).as_str() != "chicken" {
        return false;
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
        return false;
    }
    s2sdk::GetEntSchema2(pointer, &Str::from_str(BASE_ENTITY), &Str::from_str("m_bTakesDamage"), 0) == 1
        && s2sdk::GetEntSchema2(pointer, &Str::from_str(BASE_ENTITY), &Str::from_str("m_iHealth"), 0) > 0
        && s2sdk::GetEntSchema2(pointer, &Str::from_str(BASE_ENTITY), &Str::from_str("m_lifeState"), 0) == 0
}

fn use_button_rising_edge(states: &mut HashMap<i32, (PlayerIdentity, bool)>, identity: PlayerIdentity, down: bool) -> bool {
    let rising = down && !states.get(&identity.slot).is_some_and(|(old, was_down)| *old == identity && *was_down);
    if down {
        states.insert(identity.slot, (identity, true));
    } else if states.get(&identity.slot).is_some_and(|(old, _)| *old == identity) {
        states.remove(&identity.slot);
    }
    rising
}

fn update_owned_minimum(properties: &mut HashMap<i32, OwnedFloat>, handle: i32, current: f32, installed: f32) -> Option<f32> {
    if !current.is_finite() || !installed.is_finite() {
        return None;
    }
    match properties.get(&handle).copied() {
        Some(_) if (current - installed).abs() <= 0.0001 => None,
        Some(owner) if current < installed => {
            properties.insert(handle, owner);
            Some(installed)
        }
        Some(_) => {
            properties.remove(&handle);
            None
        }
        None if current < installed => {
            properties.insert(
                handle,
                OwnedFloat {
                    previous: current,
                    installed,
                },
            );
            Some(installed)
        }
        None => None,
    }
}

fn owned_float_restore(owner: OwnedFloat, current: f32) -> Option<f32> {
    (current.is_finite() && (current - owner.installed).abs() <= 0.0001).then_some(owner.previous)
}

fn matches_weapon_item(classname: &str, item: &str) -> bool {
    let item = item.trim().to_ascii_lowercase();
    let classname = classname.trim().to_ascii_lowercase();
    !item.is_empty() && (item == classname || format!("weapon_{item}") == classname || format!("weapon_{classname}") == item)
}

fn player_movement_services_for_handle(handle: i32) -> Option<usize> {
    if !s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    let pawn = s2sdk::EntHandleToEntPointer(handle);
    if pawn == 0 || s2sdk::EntPointerToEntHandle(pawn) != handle || s2sdk::GetEntityClassname(handle).as_str() != "player" {
        return None;
    }
    let pawn_class = Str::from_str("CBasePlayerPawn");
    let services_field = Str::from_str("m_pMovementServices");
    let movement_class = Str::from_str("CCSPlayer_MovementServices");
    let duck_field = Str::from_str("m_flDuckSpeed");
    if s2sdk::GetSchemaOffset(&pawn_class, &services_field) < 0 || s2sdk::GetSchemaOffset(&movement_class, &duck_field) < 0 {
        return None;
    }
    let services = s2sdk::GetEntSchema2(pawn, &pawn_class, &services_field, 0) as usize;
    (services != 0).then_some(services)
}

fn duck_speed_for_handle(handle: i32) -> Option<f32> {
    let services = player_movement_services_for_handle(handle)?;
    let speed = s2sdk::GetEntSchemaFloat2(services, &Str::from_str("CCSPlayer_MovementServices"), &Str::from_str("m_flDuckSpeed"), 0) as f32;
    speed.is_finite().then_some(speed)
}

fn set_duck_speed_for_handle(handle: i32, value: f32) -> bool {
    if !value.is_finite() {
        return false;
    }
    let Some(services) = player_movement_services_for_handle(handle) else {
        return false;
    };
    s2sdk::SetEntSchemaFloat2(
        services,
        &Str::from_str("CCSPlayer_MovementServices"),
        &Str::from_str("m_flDuckSpeed"),
        f64::from(value),
        true,
        0,
    );
    duck_speed_for_handle(handle).is_some_and(|actual| (actual - value).abs() <= 0.0001)
}

fn release_duck_speed(handle: i32, owner: OwnedFloat) -> bool {
    if !s2sdk::IsValidEntHandle(handle) {
        return true;
    }
    let Some(current) = duck_speed_for_handle(handle) else { return false };
    let Some(previous) = owned_float_restore(owner, current) else {
        return true;
    };
    set_duck_speed_for_handle(handle, previous)
}

fn duck_speed_update_candidate(player: LivePlayer, active_player: Option<LivePlayer>, team: i32) -> Option<f32> {
    if active_player != Some(player) || !matches!(team, 2 | 3) {
        return None;
    }
    duck_speed_for_handle(player.pawn_handle)
}

fn release_model_scale(handle: i32, owner: OwnedFloat) -> bool {
    if !s2sdk::IsValidEntHandle(handle) {
        return true;
    }
    let Some(current) = Runtime::entity_model_scale(handle) else {
        return false;
    };
    let Some(previous) = owned_float_restore(owner, current) else {
        return true;
    };
    Runtime::set_entity_model_scale(handle, previous)
}

fn lethal_suicide_finish_amount(anchor: f32, health: i32) -> Option<f32> {
    if !anchor.is_finite() || anchor <= 0.0 || health <= 0 {
        return None;
    }
    let current_health_amount = health as f32 * 10.0;
    current_health_amount.is_finite().then_some(anchor.max(current_health_amount))
}

// Execute after releasing the runtime lock: these engine calls can fire synchronous events.
pub enum EngineAction {
    Rollback(PlayerTask, LivePlayer),
    Loadout(PlayerTask, LivePlayer, Vec<String>),
    SettleLoadout(PlayerTask, LivePlayer, i32),
    Upgrade(PlayerTask, LivePlayer, Option<KillWeaponTarget>),
    ThirdPerson(PlayerTask, LivePlayer),
    GrantC4(PlayerTask, LivePlayer),
    RemoveC4(PlayerTask, LivePlayer, Vec<i32>),
    Respawn(PlayerTask, LivePlayer, i32),
    RoundRespawn(PlayerTask, LivePlayer, i32),
    LowHealthFinish(PlayerTask, LivePlayer, bool),
    EndRound(RoundGeneration, s2sdk::CSRoundEndReason),
    CenterDisplay {
        generation: RoundGeneration,
    },
    Projectile(crate::projectiles::Action),
    HostageRescue(crate::compat::hostage_rescue::ForwardAction),
    Bomb(crate::bombs::Action),
    Equipment(equipment::Action),
    ProjectileImpact(crate::projectiles::Impact),
    Reflection(crate::damage::Reflection),
    DelayedDamage(crate::damage::DelayedDamage),
    Suicide(crate::suicide::Action),
    ChickenSpawn {
        round: RoundGeneration,
        origin: plugify::Vec3,
    },
    ChickenBlast {
        round: RoundGeneration,
        chicken: i32,
        attacker: Option<i32>,
        origin: plugify::Vec3,
        config: funrounds_core::ChickenBlastConfig,
    },
    ChickenRemove(i32),
    SuicideFeedbackRemove(i32),
    ChickenPurchase {
        task: PlayerTask,
        player: LivePlayer,
        price: u32,
        origin: plugify::Vec3,
    },
    HealthshotUse {
        task: PlayerTask,
        player: LivePlayer,
        base_health: i32,
        config: funrounds_core::HealthshotConfig,
    },
    HealthshotPurchase {
        task: PlayerTask,
        player: LivePlayer,
        price: u32,
    },
    ChickenEat {
        task: PlayerTask,
        player: LivePlayer,
        chicken: i32,
        outcome: funrounds_core::ChickenEatOutcome,
    },
    AfterburnIgnite {
        task: PlayerTask,
        player: LivePlayer,
    },
    AfterburnRemove(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KillWeaponTarget {
    pub attacker: PlayerIdentity,
    pub handle: i32,
    pub classname: String,
    pub item_definition: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PreparedUpgrade {
    pub target: i32,
    pub weapon: String,
}

pub struct PreparedEquipmentEffect {
    pub effect: Effect,
    pub mode: funrounds_core::Mode,
    pub upgrade: Option<PreparedUpgrade>,
}

fn clear_chicken_purchase_state(slot: i32, pending: &mut HashMap<i32, PlayerTask>, actions: &mut Vec<EngineAction>) {
    pending.remove(&slot);
    actions.retain(|action| !matches!(action, EngineAction::ChickenPurchase { task, .. } if task.player.slot == slot));
}

pub struct Runtime {
    pub controller: RoundController,
    config_path: PathBuf,
    i18n_path: PathBuf,
    texts: funrounds_core::TextBundle,
    pending_texts: Option<funrounds_core::TextBundle>,
    game_thread: ThreadId,
    rng: StdRng,
    last_tick: Option<i32>,
    round: Option<RoundKey>,
    prestarted_round: Option<RoundKey>,
    players: PlayerTracker,
    controllers: HashMap<i32, i32>,
    pending_takeovers: Vec<PendingTakeover>,
    pending: Vec<(PlayerTask, i32, PendingAction)>,
    refills: equipment::Refills,
    center_display: Option<(RoundGeneration, i32)>,
    effects: HashMap<i32, Effect>,
    retired_effects: Vec<Effect>,
    third_person_map_detach_pending: bool,
    equipment: Vec<String>,
    parameters: Parameters,
    healthshot_parameters: Parameters,
    healthshot_hooks_ready: bool,
    healthshot_override_active: bool,
    healthshot_faulted: bool,
    healthshot_purchases: HashMap<i32, PlayerTask>,
    ground_reverse: crate::ground_reverse::GroundReverse,
    night_vision: crate::night_vision::NightVision,
    ghost: crate::effects::GhostRuntime,
    team_names: Parameters,
    last_name: Option<crate::i18n::ModeNames>,
    welcomes: Vec<(PlayerIdentity, f32)>,
    preferences: Option<crate::preferences::Preferences>,
    effects_ready: bool,
    capability_reasons: Vec<String>,
    faults: Faults,
    team_match: Option<TeamDeathmatch>,
    periodic_movement: Option<(RoundGeneration, funrounds_core::PeriodicMovement)>,
    money_income: Option<(RoundGeneration, MoneyIncome, f32)>,
    respawns: Vec<(PlayerTask, LivePlayer, i32, f32)>,
    round_respawns: Vec<(PlayerTask, LivePlayer, i32, f32)>,
    low_health_finishes: Vec<PlayerTask>,
    bombs: crate::bombs::Bombs,
    projectiles: crate::projectiles::Projectiles,
    hostage_rescue: crate::compat::hostage_rescue::State,
    chickens: crate::chicken_adapter::ChickenAdapter,
    chicken_entities: crate::chicken_entities::Chickens,
    chicken_ready: bool,
    chicken_follow_block_support: bool,
    chicken_use_ready: bool,
    chicken_use_buttons: HashMap<i32, (PlayerIdentity, bool)>,
    chicken_spawn_round: Option<RoundGeneration>,
    pending_chicken_blast_kills: HashMap<i32, PendingChickenBlastKill>,
    chicken_cleanup: Vec<i32>,
    chicken_purchases: HashMap<i32, PlayerTask>,
    afterburns: crate::afterburn::Afterburns,
    afterburn_cleanup: Vec<i32>,
    impacts: Vec<crate::projectiles::Impact>,
    chicken_impacts: Vec<crate::projectiles::ChickenImpact>,
    suicide_feedback: crate::suicide::FeedbackEntities,
    suicide_feedback_cleanup: Vec<i32>,
    combat: HashMap<i32, (PlayerTask, funrounds_core::CombatLife)>,
    reflections: Vec<crate::damage::Reflection>,
    delayed_damage: Vec<crate::damage::DelayedDamage>,
    random_heals: Vec<RandomHealReceipt>,
    native_self_damage_shots: HashMap<i32, NativeSelfDamageShot>,
    suicides: crate::suicide::Suicides,
    c4_assigned: bool,
    engine_actions: Vec<EngineAction>,
    equipment_in_flight: usize,
    settling_loadouts: Vec<(PlayerTask, i32, i32)>,
    prestart_context: Option<(i32, bool)>,
    warmup_active: bool,
    warmup_activation_pending: bool,
    round_transition_waiting: bool,
    duck_speed: HashMap<i32, OwnedFloat>,
    weapon_inventory: HashMap<i32, WeaponInventory>,
    model_scales: HashMap<i32, OwnedFloat>,
    model_scale_restore_pending: HashSet<i32>,
}

impl Runtime {
    pub fn new(events: EventCapabilities) -> Result<Self, String> {
        let mut takeover_reasons = Vec::new();
        if !events.bot_takeover.clone().record("bot_takeover", &mut takeover_reasons) {
            return Err(format!("required bot takeover tracking unavailable: {}", takeover_reasons.join("; ")));
        }
        let config_root = PathBuf::from(plugify::get_configs_dir().to_string());
        let (config_path, conflict) = crate::i18n::config_path(&config_root);
        if let Some(conflict) = conflict {
            log(&conflict);
        }
        let i18n_path = config_root.join("funrounds/i18n");
        let mut config = read_config(&config_path);
        let default = config.as_ref().map_or("zh-CN", |config| config.default_language.as_str());
        let texts = crate::i18n::load(&i18n_path, default, false, config.as_ref().ok())?;
        if let Ok(candidate) = &config
            && let Err(error) = texts.validate_config(candidate)
        {
            config = Err(error);
        }
        let healthshot_configured = config.as_ref().is_ok_and(|config| config.healthshot.is_some());
        if let Err(error) = &config {
            s2sdk::PrintToServer(&plugify::Str::from_str(&format!("[FunRounds] ordinary gameplay: {error}")));
        }
        let mut capability_reasons = Vec::new();
        let mut capabilities = parameters::capabilities(events.weapon_fire, events.round_prestart, &mut capability_reasons);
        capabilities.healthshot = healthshot_configured && events.healthshot;
        if healthshot_configured && !events.healthshot {
            capability_reasons.push("healthshot:[native hooks or required health ConVars unavailable]".into());
        }
        capabilities.translucent = crate::effects::translucent_probe().record("translucent", &mut capability_reasons);
        capabilities.ghost_visibility = crate::effects::ghost_visibility_probe().record("ghost_visibility", &mut capability_reasons);
        capabilities.ghost_breath = crate::effects::ghost_breath_probe().record("ghost_breath", &mut capability_reasons);
        capabilities.grenade_replenishment = equipment::refill_probe(events.grenade_thrown).record("grenade_replenishment", &mut capability_reasons);
        capabilities.lifesteal = events.player_hurt;
        capabilities.headshots = events.damage;
        let mut random_damage = crate::capability::Probe::default();
        random_damage.require(events.damage, "hook:entity_damage");
        random_damage.require(events.player_hurt, "event:player_hurt");
        random_damage.require(events.combat_events, "events:player_spawn/player_death");
        random_damage.require(crate::damage::damage_frame_token_available(), "import:GetCurrentDamageFrameToken");
        random_damage.require(unsafe { crate::s2sdk::__s2sdk_SetClientHealth }.is_some(), "import:SetClientHealth");
        capabilities.random_damage = random_damage.record("random_damage", &mut capability_reasons);
        let mut suicide = crate::suicide::probe(crate::CAN_ACQUIRE_HOOKED.load(std::sync::atomic::Ordering::Acquire));
        suicide.require(events.player_hurt, "event:player_hurt");
        suicide.require(events.combat_events, "events:player_spawn/player_death");
        suicide.require(
            crate::KNIFE_DEATH_PRE_HOOKED.load(std::sync::atomic::Ordering::Acquire),
            "hook:player_death_pre",
        );
        suicide.require(unsafe { s2sdk::__s2sdk_SetEventString }.is_some(), "import:SetEventString");
        capabilities.suicide = suicide.record("suicide", &mut capability_reasons);
        let mut source_damage = crate::capability::Probe::default();
        source_damage.require(events.damage, "hook:entity_damage");
        capabilities.weapon_damage = source_damage.clone().record("weapon_damage", &mut capability_reasons);
        source_damage.require(crate::damage::reflection_available(), "import:TakeEntityDamageWithWeapon");
        capabilities.native_self_damage = source_damage.clone().record("native_self_damage", &mut capability_reasons);
        capabilities.reflect_damage = source_damage.record("reflect_damage", &mut capability_reasons);
        let mut reflection_lifesteal_frame = crate::capability::Probe::default();
        reflection_lifesteal_frame.require(crate::damage::damage_frame_token_available(), "import:GetCurrentDamageFrameToken");
        capabilities.reflection_lifesteal_frame = reflection_lifesteal_frame.record("reflection_lifesteal_frame", &mut capability_reasons);
        let mut delayed_damage = crate::capability::Probe::default();
        delayed_damage.require(events.damage, "hook:entity_damage");
        delayed_damage.require(crate::damage::reflection_available(), "import:TakeEntityDamageWithWeapon");
        delayed_damage.require(events.combat_events, "events:player_spawn/player_death");
        capabilities.delayed_damage = delayed_damage.record("delayed_damage", &mut capability_reasons);
        let mut combat = crate::capability::Probe::default();
        combat.require(events.damage, "hook:entity_damage");
        combat.require(events.combat_events, "events:player_spawn/player_death");
        capabilities.energy_shield = combat.clone().record("energy_shield", &mut capability_reasons);
        capabilities.combat_protection = combat.record("combat_protection", &mut capability_reasons);
        let bomb_capabilities = crate::bombs::capabilities(
            events.bombs,
            events.bomb_plant_tracking,
            events.moving_blast,
            events.round_prestart,
            &mut capability_reasons,
        );
        capabilities.bomb_timing = bomb_capabilities.bomb_timing;
        capabilities.ct_bomb_no_defuse = bomb_capabilities.ct_bomb_no_defuse;
        capabilities.chicken_bomb = bomb_capabilities.chicken_bomb;
        capabilities.carry_bomb = bomb_capabilities.carry_bomb;
        capabilities.bomb_bounce = bomb_capabilities.bomb_bounce;
        let (mut decoys, mut knives) = crate::projectiles::Projectiles::capabilities(events.damage);
        decoys.require(events.grenade_thrown, "event:grenade_thrown");
        knives.require(events.weapon_fire, "event:weapon_fire");
        capabilities.dodgeball = decoys.record("dodgeball", &mut capability_reasons);
        capabilities.throwing_knives = knives.record("throwing_knives", &mut capability_reasons);
        let mut speed = crate::capability::Probe::default();
        crate::capability::imports!(speed; __s2sdk_GetClientSpeed, __s2sdk_SetClientSpeed);
        capabilities.health_speed = speed.record("health_speed", &mut capability_reasons);
        capabilities.health_damage = events.damage;
        let mut decay = crate::capability::Probe::default();
        crate::capability::imports!(decay; __s2sdk_GetClientHealth, __s2sdk_SetClientHealth);
        capabilities.health_decay = decay.record("health_decay", &mut capability_reasons);
        let mut push = crate::capability::Probe::default();
        crate::capability::imports!(push;
            __s2sdk_GetClientAbsVelocity, __s2sdk_SetClientAbsVelocity, __s2sdk_GetClientEyeAngles,
            __s2sdk_GetClientButtons, __s2sdk_GetClientFlags,
        );
        push.require(events.weapon_fire, "event:weapon_fire");
        capabilities.self_push = push.record("self_push", &mut capability_reasons);
        let mut model_scale = crate::capability::Probe::default();
        crate::capability::imports!(model_scale; __s2sdk_GetEntityModelScale, __s2sdk_SetEntityModelScale);
        model_scale.record("model_scale", &mut capability_reasons);
        let mut c4 = crate::suicide::c4_probe(false);
        c4.require(capabilities.c4_assignment, "convar:mp_give_player_c4/event:round_prestart");
        capabilities.c4_assignment = c4.record("c4_assignment", &mut capability_reasons);
        capabilities.c4_purchase = crate::suicide::c4_probe(true).record("c4_purchase", &mut capability_reasons);
        let mut respawn = crate::capability::Probe::default();
        crate::capability::imports!(respawn; __s2sdk_RespawnClient);
        respawn.require(events.combat_events, "events:player_spawn/player_death");
        capabilities.round_respawn = respawn.clone().record("round_respawn", &mut capability_reasons);
        capabilities.respawn = respawn.record("respawn", &mut capability_reasons);
        let mut round_respawn_all = crate::capability::Probe::default();
        crate::capability::imports!(round_respawn_all; __s2sdk_ForceRespawnClient);
        round_respawn_all.require(events.combat_events, "events:player_spawn/player_death");
        capabilities.round_respawn_all = round_respawn_all.record("round_respawn_all", &mut capability_reasons);
        if !events.player_hurt {
            capability_reasons.push("lifesteal:[event:player_hurt]".into());
        }
        if !events.damage {
            capability_reasons.push("headshots:[hook:entity_damage]".into());
        }
        let chicken_configured = config.as_ref().is_ok_and(|config| config.chicken.is_some());
        let chicken_ready = events.chicken_damage && crate::chicken_entities::available();
        // The config-aware core gate preserves legacy configurations with no
        // `[chicken]` section. Keep the capability itself truthful so a later
        // reload that adds `[chicken]` cannot enable unsupported live behavior.
        capabilities.ordinary_chickens = chicken_ready && events.chicken_follow_block_support;
        if chicken_configured && !events.chicken_follow_block_support {
            capability_reasons.push("ordinary_chickens:[native chicken-follow blocking unavailable]".into());
        }
        if chicken_configured && !chicken_ready {
            capability_reasons.push(format!(
                "chicken:[hook:entity_damage={}, schemas_and_entity_api={}]",
                events.chicken_damage,
                crate::chicken_entities::available()
            ));
        }
        let mut afterburn = crate::capability::Probe::default();
        crate::capability::imports!(afterburn;
            __s2sdk_GetGameTime, __s2sdk_GetClientHealth, __s2sdk_GetEntityClassname, __s2sdk_IsValidEntHandle,
            __s2sdk_EntHandleToEntPointer, __s2sdk_EntPointerToEntHandle, __s2sdk_GetEntSchemaEnt2,
        );
        afterburn.require(events.damage, "hook:entity_damage");
        afterburn.require(
            unsafe {
                std::ptr::addr_of!(crate::funrounds_native::__funrounds_native_TryIgnitePlayer)
                    .read()
                    .is_some()
            },
            "native:TryIgnitePlayer",
        );
        afterburn.schema("CBaseEntity", "m_hEffectEntity");
        afterburn.schema("CEntityFlame", "m_hEntAttached");
        capabilities.afterburn = afterburn.record("afterburn", &mut capability_reasons);
        log(&format!(
            "capabilities={capabilities:?} capability_reasons={}",
            capability_reasons.join(";")
        ));
        let controller = RoundController::new(config.ok(), capabilities)?;
        let rng = StdRng::try_from_rng(&mut rand::rngs::SysRng).map_err(|error| format!("cannot initialize randomness: {error}"))?;
        let preferences = crate::preferences::Preferences::new();
        Ok(Self {
            controller,
            config_path,
            i18n_path,
            texts,
            pending_texts: None,
            game_thread: current().id(),
            rng,
            last_tick: None,
            round: None,
            prestarted_round: None,
            players: PlayerTracker::default(),
            controllers: HashMap::new(),
            pending_takeovers: Vec::new(),
            pending: Vec::new(),
            refills: equipment::Refills::default(),
            center_display: None,
            effects: HashMap::new(),
            retired_effects: Vec::new(),
            third_person_map_detach_pending: false,
            equipment: Vec::new(),
            parameters: Parameters::default(),
            healthshot_parameters: Parameters::default(),
            healthshot_hooks_ready: false,
            healthshot_override_active: false,
            healthshot_faulted: false,
            healthshot_purchases: HashMap::new(),
            ground_reverse: crate::ground_reverse::GroundReverse::default(),
            night_vision: crate::night_vision::NightVision::default(),
            ghost: crate::effects::GhostRuntime::default(),
            team_names: Parameters::default(),
            last_name: None,
            welcomes: Vec::new(),
            preferences: Some(preferences),
            effects_ready: true,
            capability_reasons,
            faults: Faults::new(capabilities),
            team_match: None,
            periodic_movement: None,
            money_income: None,
            respawns: Vec::new(),
            round_respawns: Vec::new(),
            low_health_finishes: Vec::new(),
            bombs: crate::bombs::Bombs::default(),
            projectiles: crate::projectiles::Projectiles::default(),
            hostage_rescue: crate::compat::hostage_rescue::State::new(),
            chickens: crate::chicken_adapter::ChickenAdapter::default(),
            chicken_entities: crate::chicken_entities::Chickens::default(),
            chicken_ready,
            chicken_follow_block_support: events.chicken_follow_block_support,
            chicken_use_ready: false,
            chicken_use_buttons: HashMap::new(),
            chicken_spawn_round: None,
            pending_chicken_blast_kills: HashMap::new(),
            chicken_cleanup: Vec::new(),
            chicken_purchases: HashMap::new(),
            afterburns: crate::afterburn::Afterburns::default(),
            afterburn_cleanup: Vec::new(),
            impacts: Vec::new(),
            chicken_impacts: Vec::new(),
            suicide_feedback: crate::suicide::FeedbackEntities::default(),
            suicide_feedback_cleanup: Vec::new(),
            combat: HashMap::new(),
            reflections: Vec::new(),
            delayed_damage: Vec::new(),
            random_heals: Vec::new(),
            native_self_damage_shots: HashMap::new(),
            suicides: crate::suicide::Suicides::default(),
            c4_assigned: false,
            engine_actions: Vec::new(),
            equipment_in_flight: 0,
            settling_loadouts: Vec::new(),
            prestart_context: None,
            warmup_active: false,
            warmup_activation_pending: false,
            round_transition_waiting: false,
            duck_speed: HashMap::new(),
            weapon_inventory: HashMap::new(),
            model_scales: HashMap::new(),
            model_scale_restore_pending: HashSet::new(),
        })
    }

    pub(crate) fn healthshot_configured(&self) -> bool {
        self.controller.config().is_some_and(|config| config.healthshot.is_some())
    }

    pub(crate) fn chicken_use_configured(&self) -> bool {
        self.controller
            .config()
            .is_some_and(|config| config.chicken.is_some_and(|chicken| chicken.eating.enabled))
    }

    pub(crate) fn chicken_follow_block_supported(&self) -> bool {
        self.chicken_follow_block_support
    }

    /// The native veto applies to every configured FunRounds chicken round, even when eating is disabled.
    pub(crate) fn chicken_follow_block_desired(&self) -> bool {
        self.controller.enabled()
            && self.faults.recovery != Recovery::Stopped
            && self.controller.config().is_some_and(|config| config.chicken.is_some())
    }

    pub(crate) fn chicken_use_hook_ready(&mut self) {
        self.chicken_use_ready = true;
    }

    pub(crate) fn chicken_use_hook_failure(&mut self, reason: impl Into<String>) {
        self.chicken_use_ready = false;
        let reason = reason.into();
        self.capability_reasons.push(format!("chicken_eating:[{reason}]"));
        log(&format!("chicken eating capability unavailable: {reason}"));
    }

    pub(crate) fn observe_chicken_use_target(&mut self, pawn_handle: i32, target_handle: i32, in_use: bool) {
        let Some(player) = LivePlayer::resolve_pawn_owner(pawn_handle) else {
            return;
        };
        if LivePlayer::resolve(player.slot) != Some(player) || self.controllers.get(&player.slot) != Some(&player.controller_handle) {
            return;
        }
        let Some(identity) = self
            .players
            .identity(player.slot)
            .filter(|identity| identity.pawn_handle == pawn_handle as u32)
        else {
            return;
        };
        if !in_use {
            use_button_rising_edge(&mut self.chicken_use_buttons, identity, false);
            return;
        }
        if !self.chicken_use_ready
            || !self.chicken_ready
            || !self.ordinary_chickens_enabled()
            || !self.controller.enabled()
            || !matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
            || !s2sdk::IsValidEntHandle(target_handle)
            || s2sdk::GetEntityClassname(target_handle).as_str() != "chicken"
            || !self.chicken_entities.owns(target_handle, self.controller.generation())
        {
            return;
        }
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return;
        };
        let Some(mode) = self.controller.current_mode() else { return };
        if !config.eating.enabled_for_mode(mode.chicken_eating) {
            return;
        }
        let Some(task) = self.combat_task(player) else { return };
        let current_health = s2sdk::GetClientHealth(player.slot);
        let Some(base_health) = self.chickens.base_health(task).ok() else {
            return;
        };
        // A selector may report no target before finding a chicken during the same press.
        // Only a valid consumable target spends this press; releases are polled independently.
        if self
            .chicken_use_buttons
            .get(&identity.slot)
            .is_some_and(|(old, down)| *old == identity && *down)
            || !self.chicken_entities.mark_consumed(target_handle, task.round)
        {
            return;
        }
        let now = f64::from(s2sdk::GetGameTime());
        let outcome = match self.chickens.consume(task, current_health, base_health, now, config, &mut self.rng) {
            Ok(outcome) => outcome,
            Err(_) => {
                self.chicken_entities.release_consumed(target_handle, task.round);
                return;
            }
        };
        use_button_rising_edge(&mut self.chicken_use_buttons, identity, true);
        self.engine_actions.push(EngineAction::ChickenEat {
            task,
            player,
            chicken: target_handle,
            outcome,
        });
    }

    pub(crate) fn finish_chicken_use(&mut self, round: RoundGeneration, chicken: i32) {
        self.chicken_entities.remove_chicken(chicken, round);
        self.restore_model_scale(chicken);
        if self.chicken_entities.is_empty() {
            crate::CHICKEN_DAMAGE_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
        }
    }

    fn release_chicken_use_buttons(&mut self) {
        // The native target selector is not called on every released-input frame.
        // Button state 0 is current down state; transition masks must not keep the latch held.
        self.chicken_use_buttons.retain(|slot, (identity, _)| {
            self.players.identity(*slot) == Some(*identity)
                && LivePlayer::resolve(*slot).is_some_and(|player| player.pawn_handle as u32 == identity.pawn_handle)
                && s2sdk::GetClientButtons(*slot, 0) & (1 << 5) != 0
        });
    }

    pub(crate) fn healthshot_hook_ready(&mut self) {
        self.healthshot_hooks_ready = true;
    }

    pub(crate) fn healthshot_hook_failure(&mut self, reason: impl Into<String>) {
        self.healthshot_hooks_ready = false;
        self.healthshot_faulted = true;
        let reason = reason.into();
        self.faults.exclude(Mechanism::Healthshot, reason.clone());
        self.controller.set_capabilities(self.faults.capabilities());
        self.capability_reasons.push(format!("healthshot:[{reason}]"));
        log(&format!("healthshot capability disabled: {reason}"));
    }

    fn healthshot_active(&self) -> bool {
        self.healthshot_hooks_ready
            && !self.healthshot_faulted
            && self.controller.enabled()
            && self.controller.config().is_some_and(|config| config.healthshot.is_some())
            && self.faults.capabilities().healthshot
    }

    fn sync_healthshot_override(&mut self) -> Result<(), Fault> {
        let wanted = self.healthshot_active();
        if healthshot_override_settled(wanted, self.healthshot_override_active, self.healthshot_parameters.is_restored()) {
            return Ok(());
        }
        self.healthshot_parameters.set_healthshot_override(wanted)?;
        self.healthshot_override_active = wanted;
        if !wanted {
            self.healthshot_purchases.clear();
            self.engine_actions
                .retain(|action| !matches!(action, EngineAction::HealthshotPurchase { .. }));
        }
        Ok(())
    }

    pub(crate) fn reject_healthshot_use(&self, pawn_handle: i32, weapon_handle: i32) -> bool {
        if !self.healthshot_active() || !self.healthshot_override_active {
            return false;
        }
        let Some(player) = LivePlayer::resolve_pawn_owner(pawn_handle) else {
            return true;
        };
        if LivePlayer::resolve(player.slot) != Some(player)
            || !s2sdk::IsValidEntHandle(weapon_handle)
            || s2sdk::GetEntityClassname(weapon_handle).as_str() != "weapon_healthshot"
            || !s2sdk::GetClientWeapons(player.slot).iter().any(|handle| *handle == weapon_handle)
        {
            return true;
        }
        let Some(task) = self.combat_task(player) else { return true };
        let Some(base_health) = self.chickens.base_health(task).ok() else {
            return true;
        };
        let Some(config) = self.controller.config().and_then(|config| config.healthshot) else {
            return false;
        };
        matches!(
            funrounds_core::use_healthshot(config, s2sdk::GetClientHealth(player.slot), base_health),
            funrounds_core::HealthshotOutcome::NotConsumed
        )
    }

    pub(crate) fn queue_healthshot_use(&mut self, pawn_handle: i32, weapon_handle: i32) {
        if !self.healthshot_active() || !self.healthshot_override_active {
            return;
        }
        let Some(player) = LivePlayer::resolve_pawn_owner(pawn_handle) else {
            return;
        };
        if LivePlayer::resolve(player.slot) != Some(player)
            || !s2sdk::IsValidEntHandle(weapon_handle)
            || s2sdk::GetEntityClassname(weapon_handle).as_str() != "weapon_healthshot"
            || !s2sdk::GetClientWeapons(player.slot).iter().any(|handle| *handle == weapon_handle)
        {
            return;
        }
        let Some(task) = self.combat_task(player) else { return };
        let Some(base_health) = self.chickens.base_health(task).ok() else {
            return;
        };
        let Some(config) = self.controller.config().and_then(|config| config.healthshot) else {
            return;
        };
        if matches!(
            funrounds_core::use_healthshot(config, s2sdk::GetClientHealth(player.slot), base_health),
            funrounds_core::HealthshotOutcome::Applied { .. }
        ) {
            self.engine_actions.push(EngineAction::HealthshotUse {
                task,
                player,
                base_health,
                config,
            });
        }
    }

    fn request_healthshot(&mut self, slot: i32) -> Result<String, String> {
        if !self.healthshot_active() || !self.healthshot_override_active || !active_combat_phase(self.controller.phase()) {
            return Err(self.text(slot, "healthshot.unavailable", &[]));
        }
        let player = LivePlayer::resolve(slot).ok_or(self.text(slot, "player.alive_required", &[]))?;
        if !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
            return Err(self.text(slot, "healthshot.team_required", &[]));
        }
        if Self::owns_healthshot(player) {
            return Err(self.text(slot, "healthshot.already_owned", &[]));
        }
        let task = self.combat_task(player).ok_or(self.text(slot, "player.life_pending", &[]))?;
        if self.healthshot_purchases.contains_key(&slot) {
            return Err(self.text(slot, "healthshot.pending", &[]));
        }
        let price = self
            .controller
            .config()
            .ok_or(self.text(slot, "config.unavailable", &[]))?
            .prices
            .healthshot_cash;
        self.healthshot_purchases.insert(slot, task);
        self.engine_actions.push(EngineAction::HealthshotPurchase { task, player, price });
        Ok(self.text(slot, "healthshot.request", &[("price", &price.to_string())]))
    }

    fn owns_healthshot(player: LivePlayer) -> bool {
        s2sdk::GetClientWeapons(player.slot)
            .iter()
            .copied()
            .any(|handle| s2sdk::IsValidEntHandle(handle) && s2sdk::GetEntityClassname(handle).as_str() == "weapon_healthshot")
    }

    pub(crate) fn healthshot_buyer_current(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.healthshot_active()
            && self.healthshot_override_active
            && active_combat_phase(self.controller.phase())
            && self.healthshot_purchases.get(&player.slot) == Some(&task)
            && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && LivePlayer::resolve(player.slot) == Some(player)
    }

    pub(crate) fn finish_healthshot_purchase(&mut self, task: PlayerTask) {
        if self.healthshot_purchases.get(&task.player.slot) == Some(&task) {
            self.healthshot_purchases.remove(&task.player.slot);
        }
    }

    pub fn on_game_thread(&self) -> bool {
        current().id() == self.game_thread
    }

    pub fn take_preferences(&mut self) -> Option<crate::preferences::Preferences> {
        self.preferences.take()
    }

    pub fn restore_preferences(&mut self, preferences: crate::preferences::Preferences) {
        assert!(self.preferences.is_none(), "player preferences already attached");
        self.preferences = Some(preferences);
    }

    pub fn headshots(&self) -> Headshots {
        if self.controller.phase() == Phase::Idle {
            return Headshots::Normal;
        }
        self.controller.current_mode().map_or(Headshots::Normal, |mode| mode.headshots)
    }

    pub fn projectile_damage(&mut self, damage: &mut crate::damage::Damage) -> Option<(f32, bool)> {
        if let Some(impact) = self.projectiles.attribute_synthetic_impact(damage) {
            return Some((1.0, impact.headshot));
        }
        let mode = self.controller.current_mode();
        let claim = self.projectiles.claim_damage(
            damage,
            self.controller.generation(),
            &self.players,
            mode.is_some_and(|mode| mode.dodgeball),
            mode.is_some_and(|mode| mode.throwing_knives),
        );
        (!self.queue_projectile_claim(claim)).then_some((1.0, false))
    }

    pub fn active_knife_death(&self, victim: i32, attacker: i32) -> bool {
        self.projectiles.active_knife_death(victim, attacker)
    }

    pub fn projectile_touch(&mut self, entity: i32, other: i32, damageable_chicken: bool) -> bool {
        let claim = self.projectiles.claim_touch(
            entity,
            other,
            self.controller.generation(),
            &self.players,
            crate::projectiles::TouchContext {
                dodgeball_enabled: active_combat_phase(self.controller.phase()) && self.controller.current_mode().is_some_and(|mode| mode.dodgeball),
                knives_enabled: active_combat_phase(self.controller.phase())
                    && self.controller.current_mode().is_some_and(|mode| mode.throwing_knives),
                damageable_chicken,
            },
        );
        self.queue_projectile_claim(claim)
    }

    fn queue_projectile_claim(&mut self, claim: crate::projectiles::Claim) -> bool {
        match claim {
            crate::projectiles::Claim::Ignore => false,
            crate::projectiles::Claim::Block { projectile, remove } => {
                if remove {
                    self.projectiles.retire(projectile);
                }
                true
            }
            crate::projectiles::Claim::Apply(impact) => {
                self.impacts.push(impact);
                true
            }
            crate::projectiles::Claim::DamageChicken(impact) => {
                self.chicken_impacts.push(impact);
                true
            }
        }
    }

    pub fn is_damageable_chicken(&self, handle: i32) -> bool {
        damageable_chicken(handle)
    }

    pub fn take_chicken_impacts(&mut self) -> Vec<crate::projectiles::ChickenImpact> {
        std::mem::take(&mut self.chicken_impacts)
    }

    pub fn chicken_impact_current(&self, impact: crate::projectiles::ChickenImpact) -> bool {
        let round = self.controller.generation();
        let player = impact.task.player.slot;
        let Some(live) = LivePlayer::resolve(player) else { return false };
        self.classic_round()
            && active_combat_phase(self.controller.phase())
            && self.controller.current_mode().is_some_and(|mode| mode.throwing_knives)
            && self.projectiles.chicken_impact_current(impact, round, &self.players)
            && self.players.validates(impact.task, round, live.pawn_handle as u32)
            && self.controllers.get(&player) == Some(&live.controller_handle)
            && self.players.identity(player) == Some(impact.task.player)
            && damageable_chicken(impact.chicken)
    }

    pub fn finish_chicken_impact(&mut self, impact: crate::projectiles::ChickenImpact, result: Result<i32, Fault>) {
        self.projectiles.finish_chicken_impact(impact);
        if let Err(error) = result {
            self.handle_fault(error);
        }
    }

    pub fn prepare_impact(&mut self, mut impact: crate::projectiles::Impact) -> Option<crate::projectiles::Impact> {
        let mode = self.controller.current_mode()?;
        if !active_combat_phase(self.controller.phase())
            || !(match impact.kind {
                crate::projectiles::Kind::Knife => mode.throwing_knives,
                crate::projectiles::Kind::Dodgeball => mode.dodgeball,
            })
            || !LivePlayer::resolve(impact.task.player.slot).is_some_and(|player| {
                self.players
                    .validates(impact.task, self.controller.generation(), player.pawn_handle as u32)
            })
            || LivePlayer::resolve(impact.victim.slot) != Some(impact.victim)
            || !self
                .players
                .validates(impact.victim_task, self.controller.generation(), impact.victim.pawn_handle as u32)
            || !s2sdk::IsValidEntHandle(impact.projectile)
        {
            return None;
        }
        if impact.kind == crate::projectiles::Kind::Dodgeball {
            impact.amount = funrounds_core::dodgeball_damage(s2sdk::GetClientHealth(impact.victim.slot));
        }
        self.projectiles.begin_native_impact(impact).then_some(impact)
    }

    pub fn finish_impact(&mut self, impact: crate::projectiles::Impact) {
        self.projectiles.finish_native_impact(impact)
    }

    pub fn take_projectile_cleanup(&mut self) -> Vec<crate::projectiles::Action> {
        self.projectiles.cleanup_actions()
    }

    pub fn finish_projectile_action(&mut self, action: crate::projectiles::Action, result: crate::projectiles::ActionResult) {
        let accept = self.action_valid(&EngineAction::Projectile(action));
        if let Err(error) = self.projectiles.action_finished(action, result, accept) {
            self.handle_fault(error);
        }
    }

    pub fn diagnostics(&self) -> bool {
        self.controller.config().is_some_and(|config| config.diagnostics)
    }

    pub fn classic_enabled(&self) -> bool {
        self.effects_ready && self.controller.enabled()
    }

    pub fn chat_commands_enabled(&self) -> bool {
        self.controller.enabled()
    }

    pub fn legacy_textmsg_enabled(&self) -> bool {
        self.controller.enabled() && self.faults.recovery != Recovery::Stopped
    }

    pub fn classic_round(&self) -> bool {
        self.classic_enabled() && self.controller.phase() != Phase::Idle
    }

    pub fn hostage_rescue_touch_enabled(&self) -> bool {
        self.hostage_rescue.available() && self.classic_round() && self.controller.hostage_rescue_compat_enabled()
    }

    pub fn hostage_rescue_touch(&mut self, entity: i32, other: i32) -> Option<crate::compat::hostage_rescue::ForwardAction> {
        use crate::compat::hostage_rescue::{ForwardAction, eligible_hostage, is_playing_team};

        if !self.hostage_rescue_touch_enabled() || self.controller.phase() != Phase::Running {
            return None;
        }
        let trigger = if crate::touch::owns_rescue_zone(entity) {
            entity
        } else if crate::touch::owns_rescue_zone(other) {
            other
        } else {
            return None;
        };
        let pawn_handle = if trigger == entity { other } else { entity };
        let player = (0..s2sdk::GetMaxClients())
            .filter_map(LivePlayer::resolve)
            .find(|player| player.pawn_handle == pawn_handle)?;
        if !is_playing_team(s2sdk::GetClientTeam(player.slot) as i32) || self.controllers.get(&player.slot) != Some(&player.controller_handle) {
            return None;
        }
        let identity = self.players.identity(player.slot)?;
        let task = PlayerTask {
            player: identity,
            round: self.controller.generation(),
        };
        if !self.players.validates(task, self.controller.generation(), player.pawn_handle as u32) {
            return None;
        }
        let hostage = eligible_hostage(trigger, player)?;
        let action = ForwardAction {
            task,
            player,
            trigger,
            hostage,
        };
        if !self.action_valid(&EngineAction::HostageRescue(action)) || !self.hostage_rescue.reserve(action) {
            return None;
        }
        Some(action)
    }

    pub fn hostage_rescue_hook_failure(&mut self, reason: impl Into<String>) {
        self.hostage_rescue.set_failure(reason);
    }

    pub fn hostage_rescue_hook_ready(&mut self) {
        self.hostage_rescue.clear_failure();
    }

    fn hostage_rescue_status(&self) -> (bool, String) {
        self.hostage_rescue
            .status(self.controller.hostage_rescue_compat_enabled(), self.classic_round())
    }

    pub fn classic_damage_scale(&self, damage: &crate::damage::Damage, players: &game::DamagePlayers) -> Option<f32> {
        if !self.classic_round() {
            return Some(1.0);
        }
        let damage_type = damage.damage_type;
        let game::DamagePlayers { victim, attacker } = *players;
        let self_damage = victim.is_some() && victim == attacker;
        let mode = self.controller.current_mode()?;
        let self_damage_allowed = matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if amount > 0.0)
            || matches!(mode.self_damage, Some(SelfDamage::MissPenalty))
                && victim.is_some_and(|player| mode65_self_damage(damage, player.pawn_handle, s2sdk::GetClientHealth(player.slot)));
        let attacker_health = if !mode.health_damage {
            None
        } else {
            attacker
                .filter(|player| {
                    LivePlayer::resolve(player.slot) == Some(*player)
                        && self.controllers.get(&player.slot) == Some(&player.controller_handle)
                        && self.effects.get(&player.slot).is_some_and(|effect| {
                            Some(effect.identity) == self.players.identity(player.slot)
                                && effect.identity.pawn_handle == player.pawn_handle as u32
                                && !effect.has_pending_loadout()
                        })
                })
                .map(|player| (s2sdk::GetClientHealth(player.slot), self.effects[&player.slot].attributes.health))
        };
        let suicide_scale = if players.victim.is_some_and(|victim| self.suicides.active(victim)) {
            5.0
        } else {
            1.0
        };
        let common = suicide_scale
            * mode
                .health_curve
                .damage_scale(attacker_health, self_damage, damage_type & 32 != 0, self_damage_allowed)?;
        let weapon = (damage.weapon != -1 && s2sdk::IsValidEntHandle(damage.weapon))
            .then(|| s2sdk::GetEntityClassname(damage.weapon).to_string())
            .or_else(|| damage.weapon_class.clone());
        Some(
            common
                * funrounds_core::weapon_damage_scale(
                    mode,
                    weapon.as_deref().map(crate::damage::weapon_scale_class),
                    attacker.is_some() && !self_damage && damage_type & 32 == 0,
                ),
        )
    }

    fn combat_task(&self, player: LivePlayer) -> Option<PlayerTask> {
        let identity = self.players.identity(player.slot)?;
        (identity.pawn_handle == player.pawn_handle as u32 && self.controllers.get(&player.slot) == Some(&player.controller_handle)).then_some(
            PlayerTask {
                round: self.controller.generation(),
                player: identity,
            },
        )
    }

    fn clear_chicken_life(&mut self, identity: PlayerIdentity) {
        self.clear_pending_chicken_blast_for_life(identity);
        self.chickens.clear_life(PlayerTask {
            round: self.controller.generation(),
            player: identity,
        });
        self.afterburns.clear_life(PlayerTask {
            round: self.controller.generation(),
            player: identity,
        });
    }

    fn afterburn_settings(&self) -> Option<funrounds_core::AfterburnSettings> {
        (self.classic_enabled() && active_combat_phase(self.controller.phase()) && self.faults.capabilities().afterburn)
            .then(|| self.controller.current_mode().and_then(|mode| mode.afterburn))
            .flatten()
    }

    pub(crate) fn afterburn_action_valid(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.afterburn_settings().is_some()
            && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
            && LivePlayer::resolve(player.slot) == Some(player)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && self.afterburns.entries().iter().any(|(current, current_player, deadline)| {
                *current == task && *current_player == player && *deadline > f64::from(s2sdk::GetGameTime())
            })
    }

    pub(crate) fn begin_afterburn_ignite(&mut self, task: PlayerTask, player: LivePlayer, now: f64) -> Option<f32> {
        self.afterburn_action_valid(task, player)
            .then(|| self.afterburns.begin_ignite(task, player, now))
            .flatten()
    }

    pub(crate) fn afterburn_attachment_seen(&mut self, task: PlayerTask, player: LivePlayer, now: f64, attached: Option<i32>) {
        let _ = self.afterburns.observe_attachment(task, player, now, attached);
        self.afterburn_cleanup.extend(self.afterburns.take_removals());
    }

    pub(crate) fn record_afterburn_flame(&mut self, task: PlayerTask, player: LivePlayer, flame: i32, now: f64) -> bool {
        self.afterburn_action_valid(task, player) && self.afterburns.record_flame(task, player, flame, now)
    }

    pub(crate) fn afterburn_ignition_failed(&mut self, message: impl Into<String>) {
        self.handle_fault(Fault::mechanism(Mechanism::Afterburn, message));
    }

    pub(crate) fn take_afterburn_cleanup(&mut self) -> Vec<i32> {
        self.afterburn_cleanup.extend(self.afterburns.take_removals());
        std::mem::take(&mut self.afterburn_cleanup)
    }

    pub(crate) fn queue_afterburn_ground_tick(&mut self, damage: &crate::damage::Damage, players: &game::DamagePlayers) {
        if damage.inflictor == -1 || !s2sdk::IsValidEntHandle(damage.inflictor) {
            return;
        }
        let (Some(settings), Some(player)) = (self.afterburn_settings(), players.victim) else {
            return;
        };
        let Some(task) = self.combat_task(player) else { return };
        let now = f64::from(s2sdk::GetGameTime());
        if crate::afterburn::genuine_ground_burn(
            damage.skip_plugin_rules,
            damage.damage_type,
            s2sdk::GetEntityClassname(damage.inflictor).as_str(),
        ) && self.afterburns.refresh(task, player, now, settings.duration_seconds)
            && self.afterburns.queue_ignite(task, player)
        {
            self.engine_actions.push(EngineAction::AfterburnIgnite { task, player });
        }
    }

    pub(crate) fn afterburn_flame_damage(&self, damage: &crate::damage::Damage, players: &game::DamagePlayers) -> Option<f32> {
        if damage.amount <= 0.0
            || !damage.amount.is_finite()
            || damage.inflictor == -1
            || !s2sdk::IsValidEntHandle(damage.inflictor)
            || !crate::afterburn::flame_tick_damage(
                damage.skip_plugin_rules,
                damage.damage_type,
                s2sdk::GetEntityClassname(damage.inflictor).as_str(),
            )
        {
            return None;
        }
        let (Some(settings), Some(player)) = (self.afterburn_settings(), players.victim) else {
            return None;
        };
        let task = self.combat_task(player)?;
        self.afterburns
            .owns_flame(task, player, damage.inflictor)
            .then(|| crate::afterburn::flame_attached_to(player, damage.inflictor))
            .filter(|attached| *attached)
            .map(|_| settings.tick_damage(s2sdk::GetClientHealth(player.slot)))
    }

    fn update_afterburns(&mut self, now: f64) {
        self.afterburns.clear_round(self.controller.generation());
        let entries = self.afterburns.entries();
        for (task, player, _) in entries {
            if self.afterburn_settings().is_none()
                || !self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
                || LivePlayer::resolve(player.slot) != Some(player)
                || self.controllers.get(&player.slot) != Some(&player.controller_handle)
            {
                self.afterburns.clear_life(task);
                continue;
            }
            let attached = crate::afterburn::attached_effect(player);
            if self.afterburns.observe_attachment(task, player, now, attached) {
                self.engine_actions.push(EngineAction::AfterburnIgnite { task, player });
            }
        }
        self.afterburn_cleanup.extend(self.afterburns.take_removals());
    }

    fn update_chicken_digestion(&mut self, now: f64) {
        self.chickens.retain_round(self.controller.generation());
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return;
        };
        if !now.is_finite() || now < 0.0 {
            return;
        }
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let Some(task) = self.combat_task(player) else { continue };
            let digestion_needed = !config.eating.enabled || matches!(self.chickens.undigested(task), Ok(count) if count > 0);
            if digestion_needed {
                let _ = self.chickens.digest_due(task, now, config);
            }
        }
    }

    pub fn take_equipment_effect(&mut self, action: &EngineAction) -> Result<Option<PreparedEquipmentEffect>, Fault> {
        if !self.action_valid(action) {
            return Ok(None);
        }
        match action {
            EngineAction::Upgrade(task, player, fallback)
                if self
                    .effects
                    .get(&player.slot)
                    .is_some_and(|effect| effect.identity == task.player && effect.has_pending_loadout()) =>
            {
                self.engine_actions.push(EngineAction::Upgrade(*task, *player, fallback.clone()));
                return Ok(None);
            }
            EngineAction::ThirdPerson(task, player)
                if self
                    .effects
                    .get(&player.slot)
                    .is_some_and(|effect| effect.identity == task.player && effect.has_pending_loadout()) =>
            {
                self.engine_actions.push(EngineAction::ThirdPerson(*task, *player));
                return Ok(None);
            }
            _ => {}
        }
        let mode = self.controller.current_mode().expect("active mode").clone();
        let prepared = match action {
            EngineAction::Loadout(task, player, _) => {
                if self.effects.contains_key(&player.slot) {
                    return Ok(None);
                }
                let attributes = self
                    .combat
                    .get(&player.slot)
                    .filter(|(life_task, _)| life_task == task)
                    .map(|(_, life)| life.attributes())
                    .ok_or_else(|| Fault::cancelled("birth attributes missing before loadout"))?;
                Some(PreparedEquipmentEffect {
                    effect: Effect::capture(*player, task.player, task.round, &mode, attributes, self.diagnostics())?,
                    mode,
                    upgrade: None,
                })
            }
            EngineAction::Upgrade(task, player, fallback) => {
                if !mode.kill_loadout {
                    return Ok(None);
                }
                let Some(effect) = self.effects.get(&player.slot).filter(|effect| effect.identity == task.player) else {
                    return Ok(None);
                };
                let tracked = tracked_upgrade_weapon(effect, *player);
                let fallback = fallback.as_ref().and_then(|target| {
                    if target.attacker != task.player {
                        return None;
                    }
                    let valid = s2sdk::IsValidEntHandle(target.handle);
                    if !valid {
                        return None;
                    }
                    let owner = s2sdk::GetEntityOwner(target.handle);
                    let inventory = s2sdk::GetClientWeapons(player.slot).iter().copied().collect::<Vec<_>>();
                    let classname = s2sdk::GetEntityClassname(target.handle).to_string();
                    let item_definition_reader =
                        unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
                    let item_definition = unsafe { item_definition_reader(target.handle) };
                    kill_weapon_target_readback_matches(target, task.player, valid, owner, &inventory, &classname, item_definition)
                        .then(|| target.clone())
                });
                let Some(target) = tracked.or(fallback) else {
                    return Ok(None);
                };
                let Some(upgrade) = prepare_kill_loadout_upgrade(&mode, &mut self.rng, &target) else {
                    return Ok(None);
                };
                self.effects.remove(&player.slot).map(|effect| PreparedEquipmentEffect {
                    effect,
                    mode,
                    upgrade: Some(upgrade),
                })
            }
            EngineAction::ThirdPerson(task, player) => {
                if self.effects.get(&player.slot).is_none_or(|effect| effect.identity != task.player) {
                    return Ok(None);
                }
                self.effects
                    .remove(&player.slot)
                    .map(|effect| PreparedEquipmentEffect { effect, mode, upgrade: None })
            }
            _ => None,
        };
        if prepared.is_some() {
            self.equipment_in_flight += 1;
        }
        Ok(prepared)
    }

    pub fn take_rollbacks(&self) -> Vec<EngineAction> {
        self.effects
            .values()
            .filter(|effect| effect.rollback_ready())
            .filter_map(|effect| {
                let player = LivePlayer::resolve(effect.identity.slot)?;
                Some(EngineAction::Rollback(
                    PlayerTask {
                        player: effect.identity,
                        round: effect.round,
                    },
                    player,
                ))
            })
            .collect()
    }

    fn pending_loadout_settlement_actions(&self, tick: i32) -> Vec<EngineAction> {
        self.effects
            .values()
            .filter_map(|effect| {
                let pending = effect.pending_loadout_confirmation()?;
                if !effect.has_pending_loadout() || !equipment::confirmation_due(pending.issued_tick, tick) {
                    return None;
                }
                let slot = effect.identity.slot;
                let player = LivePlayer::resolve(slot)?;
                let task = PlayerTask {
                    player: effect.identity,
                    round: effect.round,
                };
                (self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
                    && self.controllers.get(&slot) == Some(&player.controller_handle)
                    && pending.team == s2sdk::GetClientTeam(slot) as i32)
                    .then_some(EngineAction::SettleLoadout(task, player, pending.handle))
            })
            .collect()
    }

    pub fn take_settle_loadout_effect(&mut self, action: &EngineAction) -> Option<Effect> {
        let EngineAction::SettleLoadout(task, player, handle) = action else {
            return None;
        };
        if !self.action_valid(action)
            || !self.effects.get(&task.player.slot).is_some_and(|effect| {
                effect
                    .pending_loadout_confirmation()
                    .is_some_and(|pending| pending.handle == *handle && equipment::confirmation_due(pending.issued_tick, s2sdk::GetGameTickCount()))
            })
        {
            return None;
        }
        let team = self.effects.get(&task.player.slot)?.pending_loadout_confirmation()?.team;
        let effect = self.effects.remove(&player.slot)?;
        self.settling_loadouts.push((*task, *handle, team));
        self.equipment_in_flight += 1;
        Some(effect)
    }

    pub fn take_rollback_effect(&mut self, action: &EngineAction) -> Option<Effect> {
        let EngineAction::Rollback(task, _) = action else { return None };
        if !self.action_valid(action) || !self.effects.get(&task.player.slot)?.rollback_ready() {
            return None;
        }
        let effect = self.effects.remove(&task.player.slot)?;
        self.equipment_in_flight += 1;
        Some(effect)
    }

    pub fn finish_equipment_effect(&mut self, action: &EngineAction, effect: Effect, result: Result<(), Fault>) {
        self.equipment_in_flight -= 1;
        let current = self.action_valid(action);
        if let EngineAction::SettleLoadout(task, _, handle) = action {
            self.settling_loadouts
                .retain(|(queued, queued_handle, _)| queued != task || queued_handle != handle);
        }
        let slot = effect.identity.slot;
        if current
            && result.is_ok()
            && let EngineAction::Loadout(_, _, equipment) = action
            && let Some(mode) = self.controller.current_mode()
            && !effect.has_pending_loadout()
        {
            log(&format!(
                "applied mode={} slot={slot} health={} armor={} gravity={} equipment={equipment:?}",
                mode.id, effect.attributes.health, effect.attributes.armor, mode.gravity
            ));
        }
        let same_life = self.players.identity(slot) == Some(effect.identity)
            && LivePlayer::resolve(slot).is_some_and(|player| player.pawn_handle as u32 == effect.identity.pawn_handle)
            && self
                .controllers
                .get(&slot)
                .is_some_and(|handle| *handle == s2sdk::PlayerSlotToEntHandle(slot));
        if same_life && !self.effects.contains_key(&slot) {
            self.effects.insert(slot, effect);
        } else {
            self.retired_effects.push(effect);
        }
        if !current {
            self.effects_ready = false;
        }
        if let Err(error) = result {
            self.handle_fault(error);
        }
    }

    pub fn combat_protected(&self, players: &game::DamagePlayers) -> bool {
        players.victim.and_then(|player| self.combat_task(player)).is_some_and(|task| {
            self.combat
                .get(&task.player.slot)
                .is_some_and(|(expected, life)| *expected == task && life.protected(s2sdk::GetGameTime()))
        })
    }

    pub fn record_native_player_hit(&mut self, damage: &crate::damage::Damage, players: &game::DamagePlayers) {
        // The native classifier covers bullets, buckshot and Zeus shock damage.
        if matches!(damage.weapon_kind, 1 | 3)
            && !damage.skip_plugin_rules
            && damage.amount.is_finite()
            && damage.amount > 0.0
            && let (Some(victim), Some(attacker)) = (players.victim, players.attacker)
            && victim != attacker
            && self.combat_task(victim).is_some()
        {
            self.mark_native_shot_hit(attacker, damage);
        }
    }

    pub fn suppress_hit_native_self_damage(&mut self, damage: &crate::damage::Damage, players: &game::DamagePlayers) -> bool {
        let (Some(victim), Some(attacker)) = (players.victim, players.attacker) else {
            return false;
        };
        if victim != attacker
            || !native_self_damage_signature(damage, victim)
            || !active_combat_phase(self.controller.phase())
            || !self
                .controller
                .current_mode()
                .is_some_and(|mode| matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if amount > 0.0))
        {
            return false;
        }
        let Some(task) = self.combat_task(victim) else { return false };
        if self.players.identity(victim.slot) != Some(task.player) || LivePlayer::resolve(victim.slot) != Some(victim) {
            return false;
        }
        let tick = s2sdk::GetGameTickCount();
        let matches = self
            .native_self_damage_shots
            .get(&victim.slot)
            .is_some_and(|shot| shot.task == task && shot.player == victim && shot.tick == tick);
        if !matches {
            return false;
        }
        self.native_self_damage_shots
            .remove(&victim.slot)
            .is_some_and(|shot| shot.player_hit && weapon_identity_is_current(victim, &shot.weapon))
    }

    fn mark_native_shot_hit(&mut self, player: LivePlayer, damage: &crate::damage::Damage) {
        let Some(task) = self.combat_task(player) else { return };
        let tick = s2sdk::GetGameTickCount();
        let Some(shot) = self.native_self_damage_shots.get_mut(&player.slot) else {
            return;
        };
        if weapon_identity_is_current(player, &shot.weapon) {
            shot.mark_player_hit(task, player, tick, damage);
        }
    }

    pub fn combat_damage(
        &mut self,
        damage: &crate::damage::Damage,
        players: &game::DamagePlayers,
        common_scale: f32,
        allow_reflection: bool,
    ) -> bool {
        let Some(victim) = players.victim else { return false };
        let Some(task) = self.combat_task(victim) else { return false };
        let Some((expected, life)) = self.combat.get_mut(&victim.slot).filter(|(expected, _)| *expected == task) else {
            return false;
        };
        let Some(mode) = self.controller.current_mode() else { return false };
        if players.attacker.is_some_and(|attacker| s2sdk::GetClientTeam(attacker.slot) as i32 == 1) {
            return true;
        }
        let eligible = players.attacker.is_some_and(|attacker| attacker != victim) && damage.damage_type & 32 == 0;
        let allowed_self =
            players.attacker == Some(victim) && self_damage_allowed(mode, damage, victim.pawn_handle, s2sdk::GetClientHealth(victim.slot));
        let knife = damage.weapon_kind == 2
            || damage
                .weapon_class
                .as_deref()
                .map(crate::damage::weapon_scale_class)
                .is_some_and(|class| matches!(class, "weapon_knife" | "weapon_knife_t"))
            || damage.weapon != -1
                && s2sdk::IsValidEntHandle(damage.weapon)
                && matches!(s2sdk::GetEntityClassname(damage.weapon).as_str(), "weapon_knife" | "weapon_knife_t");
        let incoming = damage.amount * common_scale;
        if !incoming.is_finite() {
            return true;
        }
        let (blocked, reflected) = funrounds_core::combat_hit(mode, life, eligible, allowed_self, knife, incoming);
        let suppress_lifesteal = mode.lifesteal;
        let source = *expected;
        let hud = life.armor_hud();
        // Record the HP path, not the post-hit shield: the breaking hit is fully absorbed.
        if numeric_self_damage_enters_health(allowed_self, blocked, incoming) && !self.low_health_finishes.contains(&task) {
            self.low_health_finishes.push(task);
        }
        if let Some(armor) = hud
            && let Err(error) = sync_shield_hud(victim, armor)
        {
            self.handle_fault(error);
            return true;
        }
        if let Some(amount) = reflected.filter(|_| allow_reflection)
            && let Some(attacker) = players.attacker
            && let Some(target) = self.combat_task(attacker)
        {
            self.reflections.push(crate::damage::Reflection {
                target,
                source,
                target_live: attacker,
                source_live: victim,
                inflictor: damage.inflictor,
                weapon: damage.weapon,
                weapon_class: damage.weapon_class.clone(),
                amount,
                damage_type: damage.damage_type,
                tick: s2sdk::GetGameTickCount(),
                suppress_lifesteal,
            });
        }
        blocked
    }

    pub fn prepare_delayed_damage_with_heal(
        &self,
        damage: &crate::damage::Damage,
        players: &game::DamagePlayers,
        amount: f32,
        headshot: bool,
        random_heal: i32,
    ) -> Option<crate::damage::DelayedDamage> {
        let mode = self.controller.current_mode()?;
        if !active_combat_phase(self.controller.phase()) || !mode.delayed_damage {
            return None;
        }
        let (target, source) = (players.victim?, players.attacker?);
        if s2sdk::GetClientTeam(source.slot) as i32 == 1
            || !crate::damage::delayed_damage_eligible(target.slot, source.slot, damage.damage_type, amount)
        {
            return None;
        }
        let target_task = self.combat_task(target)?;
        if self.controllers.get(&source.slot) != Some(&source.controller_handle) {
            return None;
        }
        let source_task = self
            .players
            .damage_source_task(source.slot, self.controller.generation(), source.pawn_handle as u32)?;
        let now = s2sdk::GetGameTime();
        if !now.is_finite() {
            return None;
        }
        let mut hit = crate::damage::DelayedDamage::new(
            target_task,
            source_task,
            target,
            source,
            damage,
            crate::damage::DamageOutcome { amount, headshot },
            now,
        );
        hit.set_random_heal(random_heal);
        Some(hit)
    }

    pub fn defer_damage(&mut self, hit: crate::damage::DelayedDamage) {
        self.delayed_damage.push(hit);
    }

    pub fn random_damage_roll(
        &mut self,
        damage: &crate::damage::Damage,
        players: &game::DamagePlayers,
        amount: f32,
    ) -> Option<funrounds_core::RandomDamage> {
        let mode = self.controller.current_mode()?;
        if !active_combat_phase(self.controller.phase()) || !mode.random_damage || damage.damage_type & crate::damage::DMG_FALL != 0 {
            return None;
        }
        let (victim, attacker) = (players.victim?, players.attacker?);
        if victim == attacker
            || self.combat_task(victim).is_none()
            || self.combat_task(attacker).is_none()
            || crate::damage::current_damage_frame_token().filter(|token| *token != 0).is_none()
        {
            return None;
        }
        funrounds_core::random_damage(amount, s2sdk::GetClientHealth(victim.slot), &mut self.rng)
    }

    pub fn queue_random_heal(&mut self, players: &game::DamagePlayers, amount: i32) -> Result<(), Fault> {
        if amount <= 0 {
            return Ok(());
        }
        let (Some(target_live), Some(source_live)) = (players.victim, players.attacker) else {
            return Ok(());
        };
        let (Some(target), Some(source)) = (self.combat_task(target_live), self.combat_task(source_live)) else {
            return Ok(());
        };
        let Some(frame_token) = crate::damage::current_damage_frame_token().filter(|token| *token != 0) else {
            return Err(Fault::mechanism(
                Mechanism::RandomDamage,
                "player damage frame token unavailable before random heal settlement",
            ));
        };
        self.random_heals.push(RandomHealReceipt {
            target,
            source,
            target_live,
            source_live,
            frame_token,
            amount,
        });
        Ok(())
    }

    fn settle_random_heal(&mut self, victim: i32, attacker: i32, event: usize) -> Option<i32> {
        if s2sdk::GetEventInt(event, &Str::from_str("dmg_health")) <= 0 {
            return None;
        }
        let receipt = if let Some(receipt) = crate::damage::internal_random_heal_hurt(victim, attacker) {
            Some(receipt)
        } else {
            let token = crate::damage::current_damage_frame_token().filter(|token| *token != 0)?;
            let index = self
                .random_heals
                .iter()
                .position(|receipt| receipt.target.player.slot == victim && receipt.source.player.slot == attacker && receipt.frame_token == token)?;
            Some(self.random_heals.remove(index))
        };
        let receipt = receipt?;
        if !self.controller.current_mode().is_some_and(|mode| mode.random_damage)
            || receipt.amount <= 0
            || receipt.target.round != self.controller.generation()
            || receipt.source.round != self.controller.generation()
            || !self
                .players
                .validates_damage_source(receipt.target, self.controller.generation(), receipt.target_live.pawn_handle as u32)
            || !self
                .players
                .validates(receipt.source, self.controller.generation(), receipt.source_live.pawn_handle as u32)
            || LivePlayer::resolve_present(receipt.target_live.slot) != Some(receipt.target_live)
            || LivePlayer::resolve(receipt.source_live.slot) != Some(receipt.source_live)
            || crate::damage::current_damage_frame_token() != Some(receipt.frame_token)
        {
            return None;
        }
        if let Err(error) = Effect::random_damage_heal(receipt.source_live, receipt.amount) {
            self.handle_fault(Fault::mechanism(Mechanism::RandomDamage, error));
            return None;
        }
        Some(attacker)
    }

    fn refresh_translucent(&mut self) {
        let generation = self.controller.generation();
        let mut failure = None;
        for effect in self.effects.values_mut().chain(self.retired_effects.iter_mut()) {
            if let Err(error) = effect.release_unowned_translucent_weapons(generation) {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
            return;
        }
        let Some(mode) = self.controller.current_mode() else {
            if let Err(error) = self.ghost.clear() {
                self.handle_fault(error);
            }
            return;
        };
        let now = s2sdk::GetGameTime();
        let ghost_settings = mode.ghost;
        let mut ghost_targets = Vec::new();
        let mut breath_sources = Vec::new();
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if effect.has_pending_loadout() || effect.round != generation || Some(effect.identity) != self.players.identity(slot) {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) = effect.update_translucent(mode, now, generation) {
                failure = Some(error);
                break;
            }
            if let Some(settings) = ghost_settings
                && settings.team.matches(s2sdk::GetClientTeam(slot) as i32)
            {
                if effect.is_fully_invisible_ghost() && player.pawn_handle != -1 {
                    ghost_targets.push((slot, player.pawn_handle));
                }
                breath_sources.push(crate::effects::GhostBreathSource {
                    identity: effect.identity,
                    round: generation,
                    player,
                    settings,
                });
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
            return;
        }
        if let Err(error) = self.ghost.publish_targets(&ghost_targets) {
            self.handle_fault(error);
            return;
        }
        if let Err(error) = self.ghost.sync_radar(&breath_sources) {
            self.handle_fault(error);
            return;
        }
        if let Err(error) = self.ghost.sync_breaths(&breath_sources, now) {
            self.handle_fault(error);
            return;
        }
        if let Err(error) = self.ghost.sync_identification(&breath_sources) {
            self.handle_fault(error);
        }
    }

    fn refresh_combat(&mut self) {
        if self.controller.current_mode().is_none() {
            return;
        }
        let now = s2sdk::GetGameTime();
        let mut failure = None;
        for (task, life) in self.combat.values_mut() {
            let Some(player) = LivePlayer::resolve(task.player.slot) else { continue };
            if !self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                || !self
                    .effects
                    .get(&player.slot)
                    .is_some_and(|effect| effect.identity == task.player && !effect.has_pending_loadout())
            {
                continue;
            }
            let health = s2sdk::GetClientHealth(player.slot);
            if let Some(next) = life.recharge(now, health)
                && next != health
            {
                s2sdk::SetClientHealth(player.slot, next);
                if s2sdk::GetClientHealth(player.slot) != next {
                    failure = Some(Fault::mechanism(Mechanism::EnergyShield, "shield health conversion write failed"));
                    break;
                }
            }
            if let Some(armor) = life.armor_hud()
                && let Err(error) = sync_shield_hud(player, armor)
            {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    fn refresh_classic(&mut self, slots: Option<&[i32]>) {
        if !self.classic_round() || !active_combat_phase(self.controller.phase()) {
            return;
        }
        let Some(mode) = self.controller.current_mode() else { return };
        let now = s2sdk::GetGameTime();
        let mut failure = None;
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if effect.has_pending_loadout()
                || slots.is_some_and(|slots| !slots.contains(&slot))
                || Some(effect.identity) != self.players.identity(slot)
            {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) = effect.update_classic(mode, now, self.suicides.active(player)) {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    fn refresh_view(&mut self) {
        if !self.classic_round() {
            return;
        }
        let generation = self.controller.generation();
        let now = s2sdk::GetGameTime();
        let mut failure = None;
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if effect.has_pending_loadout() || Some(effect.identity) != self.players.identity(slot) {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) = effect.release_inactive_scoped_weapon(generation)
                && error.scope != Scope::Cancelled
            {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
            return;
        }
        let Some(mode) = self.controller.current_mode() else { return };
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if effect.has_pending_loadout() || Some(effect.identity) != self.players.identity(slot) {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) = effect.update_view(mode, now, generation)
                && error.scope != Scope::Cancelled
            {
                failure = Some(error);
                break;
            }
            if effect.third_person_needs_update(mode) {
                self.engine_actions.push(EngineAction::ThirdPerson(
                    PlayerTask {
                        player: effect.identity,
                        round: generation,
                    },
                    player,
                ));
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    fn refresh_periodic_movement(&mut self) {
        if !self.classic_round() || !active_combat_phase(self.controller.phase()) {
            return;
        }
        let generation = self.controller.generation();
        let scale = self
            .periodic_movement
            .as_mut()
            .filter(|(round, _)| *round == generation)
            .and_then(|(_, movement)| {
                movement.update(s2sdk::GetGameTime(), &mut self.rng);
                movement.current()
            });
        let Some(scale) = scale else { return };
        let mut failure = None;
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if effect.has_pending_loadout() || Some(effect.identity) != self.players.identity(slot) {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) =
                effect.update_periodic_movement(scale, self.controller.current_mode().expect("active mode"), self.suicides.active(player))
            {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    fn send_welcomes(&mut self) {
        if !self.classic_enabled() {
            self.welcomes.clear();
            return;
        }
        let now = s2sdk::GetGameTime();
        for (identity, due) in std::mem::take(&mut self.welcomes) {
            if !self.players.validates_connection(identity)
                || !s2sdk::IsClientInGame(identity.slot)
                || self.controllers.get(&identity.slot) != Some(&s2sdk::PlayerSlotToEntHandle(identity.slot))
            {
                continue;
            }
            if now < due {
                self.welcomes.push((identity, due));
                continue;
            }
            if !s2sdk::IsFakeClient(identity.slot) {
                self.say(identity.slot, "welcome", &[("version", funrounds_core::VERSION)]);
                self.say(identity.slot, "help.body", &[]);
            }
        }
    }

    pub fn action_valid(&self, action: &EngineAction) -> bool {
        match action {
            EngineAction::Rollback(task, player) => {
                self.players.identity(player.slot) == Some(task.player)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && task.player.pawn_handle == player.pawn_handle as u32
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            EngineAction::SettleLoadout(task, player, handle) => {
                let expected_team = self
                    .effects
                    .get(&player.slot)
                    .filter(|effect| effect.identity == task.player && effect.round == task.round && effect.has_pending_loadout())
                    .and_then(Effect::pending_loadout_confirmation)
                    .filter(|pending| pending.handle == *handle)
                    .map(|pending| pending.team)
                    .or_else(|| {
                        self.settling_loadouts
                            .iter()
                            .find(|(queued, queued_handle, _)| queued == task && queued_handle == handle)
                            .map(|(_, _, team)| *team)
                    });
                self.classic_round()
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
                    && expected_team == Some(s2sdk::GetClientTeam(player.slot) as i32)
            }
            EngineAction::Loadout(task, player, _) | EngineAction::Upgrade(task, player, _) | EngineAction::ThirdPerson(task, player) => {
                self.classic_round()
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            EngineAction::GrantC4(task, player) => {
                self.classic_round()
                    && !self
                        .effects
                        .get(&player.slot)
                        .is_some_and(|effect| effect.identity == task.player && effect.round == task.round && effect.has_pending_loadout())
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            EngineAction::RemoveC4(task, player, _) => {
                self.classic_round()
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            EngineAction::Suicide(action) => match action {
                crate::suicide::Action::Purchase { start, .. } => {
                    self.c4_purchase_enabled(*start)
                        && self
                            .players
                            .validates(action.task(), self.controller.generation(), action.player().pawn_handle as u32)
                        && LivePlayer::resolve(action.player().slot) == Some(action.player())
                }
                crate::suicide::Action::Finish {
                    after_round_end: true,
                    task,
                    player,
                    ..
                } => round_end_finish_valid(
                    self.classic_enabled(),
                    self.controller.phase(),
                    *task,
                    *player,
                    self.players.identity(player.slot),
                    LivePlayer::resolve(player.slot),
                ),
                _ => {
                    self.suicide_enabled()
                        && self
                            .players
                            .validates(action.task(), self.controller.generation(), action.player().pawn_handle as u32)
                        && LivePlayer::resolve(action.player().slot) == Some(action.player())
                }
            },
            EngineAction::DelayedDamage(hit) => {
                self.classic_round()
                    && active_combat_phase(self.controller.phase())
                    && self.controller.current_mode().is_some_and(|mode| {
                        mode.delayed_damage
                            && delayed_damage_protection_valid(
                                mode.spawn_protection || mode.kill_protection,
                                hit.target,
                                self.combat
                                    .get(&hit.target.player.slot)
                                    .map(|(task, life)| (*task, life.protected(s2sdk::GetGameTime()))),
                            )
                    })
                    && hit.validates_players(
                        self.controller.generation(),
                        &self.players,
                        LivePlayer::resolve(hit.target.player.slot),
                        LivePlayer::resolve_present(hit.source.player.slot),
                    )
                    && hit.native_handles(s2sdk::IsValidEntHandle).is_some()
            }
            EngineAction::Reflection(hit) => {
                self.classic_round()
                    && self.controller.current_mode().is_some_and(|mode| mode.reflect_damage.is_some())
                    && LivePlayer::resolve(hit.target.player.slot) == Some(hit.target_live)
                    && LivePlayer::resolve_present(hit.source.player.slot) == Some(hit.source_live)
                    && self
                        .players
                        .validates(hit.target, self.controller.generation(), hit.target_live.pawn_handle as u32)
                    && self
                        .players
                        .validates_damage_source(hit.source, self.controller.generation(), hit.source_live.pawn_handle as u32)
                    && self
                        .combat
                        .get(&hit.target.player.slot)
                        .is_some_and(|(task, life)| *task == hit.target && !life.protected(s2sdk::GetGameTime()))
                    && hit.native_handles(s2sdk::IsValidEntHandle).is_some()
            }
            EngineAction::CenterDisplay { generation, .. } => {
                center_display_action_valid(*generation == self.controller.generation(), self.controller.phase(), self.effects_ready)
            }
            EngineAction::Projectile(action) => {
                self.projectiles.action_current(*action)
                    && match action.kind {
                        crate::projectiles::ActionKind::SpawnKnife { task, player } => {
                            self.effects_ready
                                && active_combat_phase(self.controller.phase())
                                && self.controller.current_mode().is_some_and(|mode| mode.throwing_knives)
                                && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
                                && LivePlayer::resolve(player.slot) == Some(player)
                        }
                        crate::projectiles::ActionKind::Remove { .. } => true,
                    }
            }
            EngineAction::HostageRescue(action) => {
                self.hostage_rescue_touch_enabled()
                    && self.controller.phase() == Phase::Running
                    && self
                        .players
                        .validates(action.task, self.controller.generation(), action.player.pawn_handle as u32)
                    && self.controllers.get(&action.player.slot) == Some(&action.player.controller_handle)
                    && LivePlayer::resolve(action.player.slot) == Some(action.player)
                    && crate::touch::owns_rescue_zone(action.trigger)
                    && s2sdk::GetClientTeam(action.player.slot) as i32 >= 2
                    && s2sdk::GetClientTeam(action.player.slot) as i32 <= 3
                    && crate::compat::hostage_rescue::eligible_hostage(action.trigger, action.player) == Some(action.hostage)
            }
            EngineAction::Bomb(action) => self.bombs.action_valid(*action, self.controller.generation(), &self.players),
            EngineAction::ChickenSpawn { round, .. } => {
                self.chicken_ready
                    && self.ordinary_chickens_enabled()
                    && self.controller.generation() == *round
                    && matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
            }
            EngineAction::ChickenBlast { round, chicken, .. } => {
                self.chicken_ready
                    && self.ordinary_chickens_enabled()
                    && self.controller.generation() == *round
                    && matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
                    && self.chicken_entities.owns(*chicken, *round)
            }
            EngineAction::ChickenRemove(_) => true,
            EngineAction::SuicideFeedbackRemove(_) => true,
            EngineAction::AfterburnIgnite { task, player } => self.afterburn_action_valid(*task, *player),
            EngineAction::AfterburnRemove(_) => true,
            EngineAction::ChickenPurchase { task, player, .. } => {
                self.chicken_ready
                    && self.ordinary_chickens_enabled()
                    && self.classic_enabled()
                    && active_combat_phase(self.controller.phase())
                    && self.chicken_purchases.get(&task.player.slot) == Some(task)
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            EngineAction::ChickenEat { task, player, chicken, .. } => {
                self.chicken_use_ready
                    && self.chicken_ready
                    && self.ordinary_chickens_enabled()
                    && self.controller.enabled()
                    && matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
                    && self.controller.current_mode().is_some_and(|mode| {
                        self.controller
                            .config()
                            .and_then(|config| config.chicken)
                            .is_some_and(|config| config.eating.enabled_for_mode(mode.chicken_eating))
                    })
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
                    && self.chicken_entities.owns(*chicken, task.round)
            }
            EngineAction::HealthshotUse { task, player, .. } => {
                self.healthshot_active()
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            EngineAction::HealthshotPurchase { task, player, .. } => self.healthshot_buyer_current(*task, *player) && !Self::owns_healthshot(*player),
            EngineAction::Equipment(action) => {
                self.refills.current(*action)
                    && match action {
                        equipment::Action::Refill(refill) => {
                            self.effects_ready
                                && self.controller.phase() != Phase::Idle
                                && self.controller.current_mode().is_some_and(|mode| mode.replenish.contains(&refill.kind))
                                && self
                                    .players
                                    .validates(refill.task, self.controller.generation(), refill.player.pawn_handle as u32)
                                && self.controllers.get(&refill.player.slot) == Some(&refill.player.controller_handle)
                                && self
                                    .effects
                                    .get(&refill.player.slot)
                                    .is_some_and(|effect| effect.identity == refill.task.player)
                                && LivePlayer::resolve(refill.player.slot) == Some(refill.player)
                        }
                        equipment::Action::Remove { .. } => true,
                    }
            }
            EngineAction::ProjectileImpact(_) => true,
            EngineAction::Respawn(task, player, team) => {
                respawn_window_open(
                    self.controller.phase(),
                    self.team_match.as_ref().map(|game| game.remaining(s2sdk::GetGameTime())),
                ) && self.players.validates_respawn(*task, self.controller.generation())
                    && respawn_target_valid(
                        *player,
                        LivePlayer::resolve_present(player.slot),
                        *team,
                        s2sdk::GetClientTeam(player.slot) as i32,
                        LivePlayer::resolve(player.slot).is_some(),
                    )
            }
            EngineAction::RoundRespawn(task, player, team) => {
                round_respawn_target_valid(
                    self.controller.current_mode().is_some_and(|mode| mode.round_respawn == RoundRespawn::All),
                    self.controller.phase(),
                    *player,
                    LivePlayer::resolve_present(task.player.slot),
                    *team,
                    s2sdk::GetClientTeam(task.player.slot) as i32,
                ) && round_respawn_task_valid(&self.players, *task, self.controller.generation(), *player)
                    && respawn_window_open(
                        self.controller.phase(),
                        self.team_match.as_ref().map(|game| game.remaining(s2sdk::GetGameTime())),
                    )
            }
            EngineAction::LowHealthFinish(task, player, _) => {
                self.controller.phase() != Phase::Idle
                    && self.controller.current_mode().is_some_and(|mode| mode.self_damage_finish_enabled())
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && funrounds_core::low_health_finish(s2sdk::GetClientHealth(player.slot))
            }
            EngineAction::EndRound(round, _) => {
                self.controller.phase() == Phase::Running && self.controller.generation() == *round && self.team_match.is_some()
            }
        }
    }

    pub fn defer_pending_loadout_c4_grant(&mut self, task: PlayerTask, player: LivePlayer) -> bool {
        let pending = self.effects.get(&player.slot).is_some_and(|effect| {
            effect.identity == task.player
                && effect.round == task.round
                && effect.has_pending_loadout()
                && effect
                    .pending_loadout_confirmation()
                    .is_some_and(|pending| pending.team == s2sdk::GetClientTeam(player.slot) as i32)
        });
        if !pending
            || !self.classic_round()
            || !self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
            || LivePlayer::resolve(player.slot) != Some(player)
            || self.controllers.get(&player.slot) != Some(&player.controller_handle)
        {
            return false;
        }
        if !self
            .engine_actions
            .iter()
            .any(|queued| matches!(queued, EngineAction::GrantC4(queued_task, queued_player) if *queued_task == task && *queued_player == player))
        {
            self.engine_actions.push(EngineAction::GrantC4(task, player));
        }
        true
    }

    pub fn cancel_bomb_action(&mut self, action: crate::bombs::Action) {
        self.bombs.action_cancelled(action);
    }

    pub fn finish_equipment_action(&mut self, action: equipment::Action, result: equipment::ActionResult) {
        let removed_entity = matches!(&action, equipment::Action::Remove { .. });
        if !self.action_valid(&EngineAction::Equipment(action)) {
            self.refills.cancel_action(action);
        }
        match self.refills.finish(action, result) {
            Ok(completion) => {
                if let Some((task, id)) = completion.retry {
                    self.queue(task.player, s2sdk::GetGameTickCount(), PendingAction::Grenade(id));
                }
                if let Some(diagnostic) = completion.diagnostic {
                    log(&diagnostic);
                }
            }
            Err(error) => self.handle_fault(error),
        }
        if removed_entity {
            self.retry_retired_effects();
        }
    }

    fn retry_retired_effects(&mut self) {
        let mut failure = None;
        for index in (0..self.retired_effects.len()).rev() {
            if let Err(error) = restore_or_retain(&mut self.retired_effects, index, |effect| {
                effect.restore_retired_state(&mut self.refills)?;
                Ok(effect.recovery_complete())
            }) {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(Fault::global(format!("retired effect cleanup failed after entity removal: {error}")));
        }
    }

    pub fn take_equipment_cleanup(&mut self) -> Vec<equipment::Action> {
        self.refills.cleanup_actions()
    }

    pub fn finish_bomb_action(&mut self, action: crate::bombs::Action, result: Result<crate::bombs::ActionResult, Fault>) {
        let accept = self.action_valid(&EngineAction::Bomb(action));
        if let Err(error) = self.bombs.action_finished(action, result, accept) {
            self.handle_fault(error);
        }
        self.restore_completed_native_bomb_scales();
    }

    pub fn prepare_carrier_disconnect(&mut self, slot: i32) -> Option<crate::bombs::Action> {
        let player = LivePlayer::resolve_present(slot)?;
        // Death retires the effect life, but cleanup must still release that connection's native pawn.
        self.players.connection_identity(slot)?;
        if self.controllers.get(&slot) != Some(&player.controller_handle) {
            return None;
        }
        self.bombs.prepare_carrier_disconnect(self.controller.generation(), player)
    }

    pub fn take_bomb_actions(&mut self) -> Vec<crate::bombs::Action> {
        self.bombs.take_actions()
    }

    pub fn take_prestart_context(&mut self) -> Option<(i32, bool)> {
        self.prestart_context.take()
    }

    pub fn finish_round_prestart(&mut self, (tick, warmup): (i32, bool)) {
        self.bombs.refresh_prestart_retirements();
        self.bombs.finish_round_prestart_cleanup();
        if warmup && self.controller.phase() == Phase::Warmup {
            self.cleanup_effects(false);
            self.restore_team_names();
            self.controller.stop_warmup();
            self.round = None;
            self.prestarted_round = None;
            self.warmup_activation_pending = self.warmup_active;
        }
        self.finish_round_transition_recovery();
        if tick < 0 || warmup {
            self.prestarted_round = None;
            return;
        }
        let key = self.controller.round_key(tick as u64);
        let ready_for_prestart_selection = self.effects_ready
            || (self.faults.recovery == Recovery::Ready
                && self.cleanup_complete_for_round_transition_without_bombs()
                && self.bombs.prestart_removals_ready());
        if self.select_round(key, warmup, ready_for_prestart_selection) {
            self.prestarted_round = Some(key);
            if let Err(error) = self.parameters.apply_prestart_purchase(self.controller.current_mode()) {
                self.handle_fault(error);
            }
        }
    }

    fn confirm_prestarted_round_reset(&mut self) -> bool {
        self.bombs.confirm_prestart_removals_after_round_reset();
        self.effects_ready = self.faults.recovery == Recovery::Ready
            && self.cleanup_complete_for_round_transition_except_pending_and_prestart_parameters_and_bombs()
            && self.pending_prestarted_loadouts_are_current()
            && self.bombs.is_clean();
        self.effects_ready
    }

    pub fn take_bomb_explosion(&mut self) -> Option<crate::bombs::Explosion> {
        self.bombs.take_explosion(self.controller.generation())
    }

    fn update_duck_speed(&mut self) {
        if !self.controller.enabled() {
            return;
        }
        let players = (0..s2sdk::GetMaxClients())
            .filter_map(|slot| {
                let player = LivePlayer::resolve_present(slot)?;
                duck_speed_update_candidate(player, LivePlayer::resolve(slot), s2sdk::GetClientTeam(slot) as i32).map(|speed| (player, speed))
            })
            .collect::<Vec<_>>();
        let current_handles = players
            .iter()
            .map(|(player, _)| player.pawn_handle)
            .collect::<std::collections::HashSet<_>>();
        for (player, current) in players {
            if let Some(value) = update_owned_minimum(&mut self.duck_speed, player.pawn_handle, current, 8.0)
                && !set_duck_speed_for_handle(player.pawn_handle, value)
            {
                log(&format!("failed to set crouch speed for pawn {}", player.pawn_handle));
            }
        }
        let stale = self
            .duck_speed
            .keys()
            .copied()
            .filter(|handle| !current_handles.contains(handle))
            .collect::<Vec<_>>();
        for handle in stale {
            let Some(owner) = self.duck_speed.get(&handle).copied() else { continue };
            if release_duck_speed(handle, owner) {
                self.duck_speed.remove(&handle);
            }
        }
    }

    fn restore_duck_speed(&mut self) {
        let entries = self.duck_speed.iter().map(|(handle, owner)| (*handle, *owner)).collect::<Vec<_>>();
        for (handle, owner) in entries {
            if release_duck_speed(handle, owner) {
                self.duck_speed.remove(&handle);
            }
        }
    }

    fn weapon_inventory_player_is_current(&self, player: LivePlayer) -> bool {
        let live_identity_matches = self
            .players
            .identity(player.slot)
            .is_some_and(|identity| identity.pawn_handle == player.pawn_handle as u32)
            || self.weapon_inventory.get(&player.slot).is_some_and(|previous| {
                previous.player == player
                    && self.players.validates_damage_source(
                        PlayerTask {
                            round: self.controller.generation(),
                            player: previous.identity,
                        },
                        self.controller.generation(),
                        player.pawn_handle as u32,
                    )
            });
        live_identity_matches
            && LivePlayer::resolve_present(player.slot) == Some(player)
            && player.pawn_controller() == Some(player.controller_handle)
    }

    fn weapon_services_for_inventory(&self, player: LivePlayer) -> Option<usize> {
        if !self.weapon_inventory_player_is_current(player) {
            return None;
        }
        let class = Str::from_str("CBasePlayerPawn");
        let field = Str::from_str("m_pWeaponServices");
        if s2sdk::GetSchemaOffset(&class, &field) < 0 {
            return None;
        }
        let pawn = s2sdk::EntHandleToEntPointer(player.pawn_handle);
        if pawn == 0 || s2sdk::EntPointerToEntHandle(pawn) != player.pawn_handle {
            return None;
        }
        let services = s2sdk::GetEntSchema2(pawn, &class, &field, 0) as usize;
        (services != 0 && self.weapon_inventory_player_is_current(player)).then_some(services)
    }

    fn read_client_weapon_inventory(&self, player: LivePlayer) -> Option<Vec<i32>> {
        let services = self.weapon_services_for_inventory(player)?;
        let weapons = s2sdk::GetClientWeapons(player.slot).iter().copied().collect();
        (self.weapon_services_for_inventory(player) == Some(services)).then_some(weapons)
    }

    fn refresh_weapon_inventory(&mut self) {
        let mut inventories = HashMap::new();
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve_present(slot) else { continue };
            if !self.weapon_inventory_player_is_current(player) {
                continue;
            }
            let Some(weapons) = self.read_client_weapon_inventory(player) else {
                // A live pawn without readable weapon services has no inventory
                // observation. Keep only its matching previous snapshot so an
                // unready tick cannot be mistaken for a genuine empty inventory.
                if let Some(previous) = self.weapon_inventory.get(&slot).filter(|previous| previous.player == player).cloned() {
                    inventories.insert(slot, previous);
                }
                continue;
            };
            let weapons = weapons
                .iter()
                .filter(|handle| s2sdk::IsValidEntHandle(**handle) && s2sdk::GetEntityOwner(**handle) == player.pawn_handle)
                .copied()
                .map(|handle| (handle, s2sdk::GetEntityClassname(handle).to_string()))
                .collect();
            let identity = self
                .players
                .identity(player.slot)
                .or_else(|| self.weapon_inventory.get(&player.slot).map(|previous| previous.identity));
            let Some(identity) = identity else { continue };
            inventories.insert(player.slot, WeaponInventory { player, identity, weapons });
        }
        // CS2 has no weapon_drop game event. Reconcile complete handles against the
        // preceding inventory and the engine owner, so deletion/consumption/transfer
        // is never mistaken for a world drop.
        let mut dropped = Vec::new();
        if self.controller.enabled() && Self::model_scale_imports_available() {
            for (slot, previous) in &self.weapon_inventory {
                let Some(current) = inventories.get_mut(slot).filter(|current| current.player == previous.player) else {
                    continue;
                };
                for (handle, classname) in &previous.weapons {
                    if current.weapons.contains_key(handle)
                        || !s2sdk::IsValidEntHandle(*handle)
                        || s2sdk::GetEntityClassname(*handle).as_str() != classname
                    {
                        continue;
                    }
                    match s2sdk::GetEntityOwner(*handle) {
                        -1 => dropped.push(*handle),
                        owner if owner == previous.player.pawn_handle => {
                            // Inventory removal can precede ownership release during a callback.
                            current.weapons.insert(*handle, classname.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
        self.weapon_inventory = inventories;
        self.model_scales.retain(|handle, _| s2sdk::IsValidEntHandle(*handle));
        self.model_scale_restore_pending.retain(|handle| self.model_scales.contains_key(handle));
        for handle in dropped {
            self.randomize_model_scale(handle);
        }
    }

    fn randomize_model_scale(&mut self, handle: i32) {
        if self.model_scales.contains_key(&handle) {
            return;
        }
        let Some(previous) = Self::entity_model_scale(handle) else { return };
        let installed = self.random_model_scale();
        if Self::set_entity_model_scale(handle, installed) {
            self.model_scales.insert(handle, OwnedFloat { previous, installed });
        }
    }

    fn entity_model_scale(handle: i32) -> Option<f32> {
        if !Self::model_scale_imports_available() || !s2sdk::IsValidEntHandle(handle) {
            return None;
        }
        let pointer = s2sdk::EntHandleToEntPointer(handle);
        if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
            return None;
        }
        let scale = s2sdk::GetEntityModelScale(handle);
        scale.is_finite().then_some(scale)
    }

    fn set_entity_model_scale(handle: i32, value: f32) -> bool {
        if !Self::model_scale_imports_available() || !s2sdk::IsValidEntHandle(handle) || !value.is_finite() {
            return false;
        }
        let pointer = s2sdk::EntHandleToEntPointer(handle);
        if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
            return false;
        }
        s2sdk::SetEntityModelScale(handle, value);
        Self::entity_model_scale(handle).is_some_and(|actual| (actual - value).abs() <= 0.0001)
    }

    fn model_scale_imports_available() -> bool {
        let mut probe = crate::capability::Probe::default();
        crate::capability::imports!(probe; __s2sdk_GetEntityModelScale, __s2sdk_SetEntityModelScale);
        probe.available()
    }

    fn random_model_scale(&mut self) -> f32 {
        match self.rng.random_range(1..=5) {
            5 => self.rng.random_range(3.0..=15.0),
            4 => self.rng.random_range(0.5..=0.75),
            _ => self.rng.random_range(0.75..=3.0),
        }
    }

    fn item_pickup(&mut self, slot: i32, event: usize) {
        let Some(player) = LivePlayer::resolve_present(slot) else { return };
        let Some(weapons) = self.read_client_weapon_inventory(player) else {
            return;
        };
        let item = s2sdk::GetEventString(event, &Str::from_str("item")).to_string();
        for handle in weapons {
            if !self.model_scales.contains_key(&handle) {
                continue;
            }
            if !s2sdk::IsValidEntHandle(handle)
                || s2sdk::GetEntityOwner(handle) != player.pawn_handle
                || !matches_weapon_item(s2sdk::GetEntityClassname(handle).as_str(), &item)
            {
                continue;
            }
            self.restore_model_scale(handle);
        }
    }

    fn restore_picked_up_weapon_scales(&mut self) {
        let picked_up = self
            .model_scales
            .keys()
            .filter_map(|handle| {
                let player_owns_it = self
                    .weapon_inventory
                    .values()
                    .any(|inventory| inventory.weapons.contains_key(handle) && s2sdk::GetEntityOwner(*handle) == inventory.player.pawn_handle);
                player_owns_it.then_some(*handle)
            })
            .collect::<Vec<_>>();
        for handle in picked_up {
            self.restore_model_scale(handle);
        }
    }

    fn restore_model_scale(&mut self, handle: i32) {
        let Some(owner) = self.model_scales.get(&handle).copied() else {
            self.model_scale_restore_pending.remove(&handle);
            return;
        };
        if release_model_scale(handle, owner) {
            self.model_scales.remove(&handle);
            self.model_scale_restore_pending.remove(&handle);
        } else {
            self.model_scale_restore_pending.insert(handle);
        }
    }

    fn restore_model_scales(&mut self) {
        let entries = self.model_scales.iter().map(|(handle, owner)| (*handle, *owner)).collect::<Vec<_>>();
        for (handle, owner) in entries {
            if release_model_scale(handle, owner) {
                self.model_scales.remove(&handle);
                self.model_scale_restore_pending.remove(&handle);
            } else {
                self.model_scale_restore_pending.insert(handle);
            }
        }
    }

    fn retry_model_scale_restores(&mut self) {
        let handles = self.model_scale_restore_pending.iter().copied().collect::<Vec<_>>();
        for handle in handles {
            self.restore_model_scale(handle);
        }
    }

    fn restore_completed_native_bomb_scales(&mut self) {
        for handle in self.bombs.take_native_model_scale_restores() {
            self.restore_model_scale(handle);
        }
    }

    fn warmup_activation_available(&self) -> bool {
        self.controller.enabled()
            && self.controller.config().is_some_and(|config| {
                config.warmup.as_ref().is_some_and(|warmup| {
                    warmup.enabled
                        && self
                            .controller
                            .capabilities()
                            .supports_warmup_with_chicken_config(&warmup.mode, config.chicken.is_some())
                })
            })
    }

    pub fn finish_bomb_explosion(&mut self, explosion: crate::bombs::Explosion, temporary: i32, result: Result<(), Fault>) {
        if self.bombs.finish_explosion(explosion, temporary)
            && let Err(error) = result
        {
            self.handle_fault(error);
        }
    }

    pub fn blocks_native_bomb_damage(&self, inflictor: i32) -> bool {
        self.bombs.blocks_native_explosion(inflictor)
    }

    pub fn take_chicken_cleanup(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.chicken_cleanup)
    }

    pub fn finish_chicken_purchase(&mut self, task: PlayerTask) {
        if self.chicken_purchases.get(&task.player.slot) == Some(&task) {
            self.chicken_purchases.remove(&task.player.slot);
        }
    }

    fn cancel_chicken_purchase(&mut self, slot: i32) {
        clear_chicken_purchase_state(slot, &mut self.chicken_purchases, &mut self.engine_actions);
    }

    pub(crate) fn chicken_risk_finish_current(&self, task: PlayerTask, player: LivePlayer) -> bool {
        // round_end advances the round generation and removes the chicken. The
        // synchronous risk-blast continuation retains only this exact live eater.
        self.classic_enabled()
            && self.players.identity(player.slot) == Some(task.player)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && LivePlayer::resolve(player.slot) == Some(player)
    }

    pub fn chicken_buyer_identity_current(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.on_game_thread()
            && self.players.validates_connection(task.player)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && LivePlayer::resolve_present(player.slot) == Some(player)
    }

    fn ordinary_chickens_enabled(&self) -> bool {
        self.controller.config().is_some_and(|config| config.chicken.is_some())
            && self.controller.current_mode().is_some_and(|mode| mode.ordinary_chickens)
    }

    fn cleanup_ordinary_chickens(&mut self) {
        self.chickens.clear_eating();
        self.chicken_use_buttons.clear();
        self.chicken_purchases.clear();
        self.pending_chicken_blast_kills.clear();
        self.engine_actions.retain(|action| {
            !matches!(
                action,
                EngineAction::ChickenSpawn { .. }
                    | EngineAction::ChickenBlast { .. }
                    | EngineAction::ChickenPurchase { .. }
                    | EngineAction::ChickenEat { .. }
            )
        });
        let chickens = self.chicken_entities.clear();
        for handle in &chickens {
            self.restore_model_scale(*handle);
        }
        self.chicken_cleanup.extend(chickens);
        self.chicken_spawn_round = None;
        crate::CHICKEN_DAMAGE_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
    }

    fn clear_chicken_round_work(&mut self) {
        self.chickens.clear_eating();
        self.chicken_use_buttons.clear();
        self.chicken_purchases.clear();
        self.pending_chicken_blast_kills.clear();
        self.engine_actions.retain(|action| {
            !matches!(
                action,
                EngineAction::ChickenSpawn { .. }
                    | EngineAction::ChickenBlast { .. }
                    | EngineAction::ChickenPurchase { .. }
                    | EngineAction::ChickenEat { .. }
            )
        });
        self.chicken_spawn_round = None;
    }

    fn advance_chicken_round(&mut self, round: RoundGeneration) {
        self.advance_chicken_round_if_alive(round, |handle| {
            if !s2sdk::IsValidEntHandle(handle) || s2sdk::GetEntityClassname(handle).as_str() != "chicken" {
                return false;
            }
            crate::chicken_entities::health(handle).is_some_and(|(health, life_state)| health > 0 && life_state == 0)
        });
    }

    fn advance_chicken_round_if_alive(&mut self, round: RoundGeneration, alive: impl FnMut(i32) -> bool) {
        self.pending_chicken_blast_kills.retain(|_, pending| pending.round == round);
        let stale = self.chicken_entities.advance_round(round, alive);
        for handle in stale {
            self.restore_model_scale(handle);
            self.chicken_cleanup.push(handle);
        }
        crate::CHICKEN_DAMAGE_ACTIVE.store(!self.chicken_entities.is_empty(), std::sync::atomic::Ordering::Release);
    }

    pub fn track_chicken(&mut self, round: RoundGeneration, handle: i32) -> bool {
        if !self.chicken_ready
            || !self.ordinary_chickens_enabled()
            || self.controller.generation() != round
            || !matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
            || !self.chicken_entities.track(handle, round)
        {
            return false;
        }
        self.randomize_model_scale(handle);
        crate::CHICKEN_DAMAGE_ACTIVE.store(true, std::sync::atomic::Ordering::Release);
        true
    }

    pub fn track_chicken_explosion(&mut self, round: RoundGeneration, handle: i32, source: i32) -> bool {
        self.ordinary_chickens_enabled()
            && self.controller.generation() == round
            && matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
            && self.chicken_entities.track_explosion(handle, round, source)
    }

    pub fn finish_chicken_explosion(&mut self, round: RoundGeneration, handle: i32) {
        self.chicken_entities.remove_explosion(handle, round);
    }

    pub fn chicken_blast_damage(&self, damage: &crate::damage::Damage) -> bool {
        let round = self.controller.generation();
        [damage.inflictor, damage.attacker]
            .into_iter()
            .any(|handle| self.chicken_entities.is_chicken_explosion(handle, round))
    }

    pub(crate) fn observe_chicken_blast_player_damage(&mut self, damage: &crate::damage::Damage, players: &game::DamagePlayers) {
        let Some(victim) = players.victim else { return };
        let Some(victim_identity) = self
            .players
            .identity(victim.slot)
            .filter(|identity| identity.pawn_handle == victim.pawn_handle as u32)
        else {
            self.pending_chicken_blast_kills.remove(&victim.slot);
            return;
        };
        let round = self.controller.generation();
        if !self.classic_round()
            || !active_combat_phase(self.controller.phase())
            || !self.controller.current_mode().is_some_and(|mode| mode.kill_loadout)
            || !self.chicken_blast_damage(damage)
        {
            self.pending_chicken_blast_kills.remove(&victim.slot);
            return;
        }
        let Some(source) = self
            .chicken_entities
            .blast_kill_weapon_source(&[damage.inflictor, damage.attacker, damage.weapon], round)
        else {
            self.pending_chicken_blast_kills.remove(&victim.slot);
            return;
        };
        if !chicken_weapon_source_is_current(&source, &self.players, &self.controllers) {
            self.pending_chicken_blast_kills.remove(&victim.slot);
            return;
        }
        self.pending_chicken_blast_kills.insert(
            victim.slot,
            PendingChickenBlastKill {
                round,
                victim: victim_identity,
                tick: s2sdk::GetGameTickCount(),
                damage_frame_token: crate::damage::current_damage_frame_token().filter(|token| *token != 0),
                source,
            },
        );
    }

    fn take_chicken_blast_kill_weapon(&mut self, victim_slot: i32, attacker_slot: i32) -> Option<crate::chicken_entities::KillWeaponSource> {
        let pending = self.pending_chicken_blast_kills.remove(&victim_slot)?;
        let victim = self.players.identity(victim_slot)?;
        let attacker = self.players.identity(attacker_slot)?;
        let frame = crate::damage::current_damage_frame_token().filter(|token| *token != 0);
        if pending.matches(self.controller.generation(), victim, attacker, s2sdk::GetGameTickCount(), frame)
            && attacker_slot == pending.source.attacker.slot
            && chicken_weapon_source_is_current(&pending.source, &self.players, &self.controllers)
        {
            Some(pending.source)
        } else {
            None
        }
    }

    fn clear_pending_chicken_blast_for_life(&mut self, identity: PlayerIdentity) {
        self.pending_chicken_blast_kills
            .retain(|slot, pending| *slot != identity.slot && pending.victim != identity && pending.source.attacker != identity);
    }

    fn clear_pending_chicken_blast_for_slot(&mut self, slot: i32) {
        self.pending_chicken_blast_kills
            .retain(|victim, pending| *victim != slot && pending.source.attacker.slot != slot);
    }

    pub fn chicken_damage_blocked(&mut self, damage: crate::chicken_entities::EntityDamageSource) -> bool {
        let round = self.controller.generation();
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return false;
        };
        let blocked = self.chicken_entities.damage_blocked(damage, round, config.damage_chain);
        if !blocked {
            let players = &self.players;
            let controllers = &self.controllers;
            self.chicken_entities
                .record_lethal_attacker(damage, round, || capture_chicken_damage_weapon_source(damage, players, controllers));
        }
        blocked
    }

    fn queue_chicken_population(&mut self) {
        if !self.chicken_ready || !self.classic_enabled() || !matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup) {
            return;
        }
        if !self.ordinary_chickens_enabled() {
            self.cleanup_ordinary_chickens();
            return;
        }
        if self.chicken_spawn_round == Some(self.controller.generation()) {
            return;
        }
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return;
        };
        let mut effective_players = 0_u32;
        let mut positions = Vec::new();
        for slot in 0..s2sdk::GetMaxClients() {
            if !s2sdk::IsClientInGame(slot) || !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
                continue;
            }
            effective_players = effective_players.saturating_add(1);
            if let Some(player) = LivePlayer::resolve(slot) {
                let origin = s2sdk::GetEntityAbsOrigin(player.pawn_handle);
                if origin.x.is_finite() && origin.y.is_finite() && origin.z.is_finite() {
                    positions.push(origin);
                }
            }
        }
        if positions.is_empty() {
            return;
        }
        let round = self.controller.generation();
        let count = config.spawn.target_count(effective_players);
        let owned = self.chicken_entities.owned_handles(round);
        let maximum = config.spawn.maximum_population(count);
        if owned.len() as u64 > u64::from(maximum) {
            let excess = (owned.len() as u64 - u64::from(maximum)) as usize;
            for handle in owned.into_iter().take(excess) {
                if self.chicken_entities.remove_chicken(handle, round) {
                    self.restore_model_scale(handle);
                    self.chicken_cleanup.push(handle);
                }
            }
            self.chicken_spawn_round = Some(round);
            crate::CHICKEN_DAMAGE_ACTIVE.store(!self.chicken_entities.is_empty(), std::sync::atomic::Ordering::Release);
            log(&format!("chicken population trim round={round:?} maximum={maximum} removed={excess}"));
            return;
        }
        let refill_threshold = config.spawn.refill_threshold(count);
        if owned.len() >= refill_threshold as usize {
            self.chicken_spawn_round = Some(round);
            return;
        }
        for index in 0..count {
            let player_origin = positions[index as usize % positions.len()];
            let turn = index as f32 * 2.399_963_1;
            let ring = (index as usize / positions.len()) as f32;
            let distance = 64.0 + ring * 24.0;
            self.engine_actions.push(EngineAction::ChickenSpawn {
                round,
                origin: plugify::Vec3 {
                    x: player_origin.x + turn.cos() * distance,
                    y: player_origin.y + turn.sin() * distance,
                    z: player_origin.z + 4.0,
                },
            });
        }
        self.chicken_spawn_round = Some(round);
        log(&format!(
            "chicken spawn plan round={round:?} count={count} effective_players={effective_players}"
        ));
    }

    fn poll_chickens(&mut self) -> Vec<EngineAction> {
        if !self.chicken_ready || !matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup) {
            return Vec::new();
        }
        if !self.ordinary_chickens_enabled() {
            self.cleanup_ordinary_chickens();
            return Vec::new();
        }
        let round = self.controller.generation();
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        for handle in self.chicken_entities.owned_handles(round) {
            let Some((health, life_state)) = crate::chicken_entities::health(handle) else {
                // Removal without a readable dead state is not a kill and must not blast.
                self.chicken_entities.remove_chicken(handle, round);
                self.restore_model_scale(handle);
                continue;
            };
            let attacker = self.chicken_entities.killer(handle, round);
            if (health <= 0 || life_state == 2)
                && let Some(origin) = crate::chicken_entities::position(handle)
                && self.chicken_entities.mark_dead(handle, round)
                && let Some(config) = self.chicken_entities.take_blast_config(handle, round, config.kill_blast)
            {
                actions.push(EngineAction::ChickenBlast {
                    round,
                    chicken: handle,
                    attacker,
                    origin,
                    config,
                });
            }
        }
        if self.chicken_entities.is_empty() {
            crate::CHICKEN_DAMAGE_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
        }
        actions
    }

    pub fn update(&mut self) -> Vec<EngineAction> {
        if let Err(error) = self.sync_healthshot_override() {
            self.healthshot_faulted = true;
            let message = error.to_string();
            self.faults.exclude(Mechanism::Healthshot, message.clone());
            self.controller.set_capabilities(self.faults.capabilities());
            log(&format!("healthshot capability disabled: {message}"));
        }
        if let Some(failures) = self.preferences.as_mut().map(crate::preferences::Preferences::check_worker) {
            for (account_id, error) in failures {
                log(&format!(
                    "player preference background operation failed: {error}; latest choice retained for retry"
                ));
                if let Some(account_id) = account_id
                    && let Some(slot) = (0..s2sdk::GetMaxClients()).find(|slot| crate::preferences::authenticated_id(*slot) == Some(account_id))
                {
                    self.say(slot, "preference.save_failed", &[("error", &error)]);
                }
            }
        }
        let tick = self.observe_tick(None, None);
        if let Err(error) = self.night_vision.update() {
            self.handle_fault(Fault::global(error));
        }
        let reverse = (tick >= 0 && self.classic_enabled() && active_combat_phase(self.controller.phase()))
            .then(|| self.controller.current_mode().and_then(|mode| mode.ground_reverse))
            .flatten();
        if let Err(error) = self.ground_reverse.apply(reverse) {
            self.handle_fault(error);
        }
        self.apply_pending_takeovers(tick);
        self.release_chicken_use_buttons();
        self.native_self_damage_shots.retain(|slot, shot| {
            shot.tick == tick && self.players.identity(*slot) == Some(shot.task.player) && LivePlayer::resolve(*slot) == Some(shot.player)
        });
        let frame = crate::damage::current_damage_frame_token().filter(|token| *token != 0);
        self.random_heals.retain(|receipt| Some(receipt.frame_token) == frame);
        let round = self.controller.generation();
        let players = &self.players;
        self.pending_chicken_blast_kills.retain(|slot, pending| {
            pending.tick == tick
                && pending.round == round
                && players.identity(*slot) == Some(pending.victim)
                && players.identity(pending.source.attacker.slot) == Some(pending.source.attacker)
        });
        self.recover_incomplete_effects();
        let warmup = game::warmup();
        self.sync_warmup(warmup, tick, None);
        self.update_duck_speed();
        self.refresh_weapon_inventory();
        self.restore_picked_up_weapon_scales();
        self.retry_model_scale_restores();
        if self.warmup_active && self.controller.phase() == Phase::Warmup && !self.warmup_activation_pending && self.effects_ready && tick >= 0 {
            self.reconcile_warmup_players(tick);
        }
        let now = s2sdk::GetGameTime();
        self.update_afterburns(f64::from(now));
        let center_display = self.send_pending_center(tick);
        let mut actions = self
            .take_afterburn_cleanup()
            .into_iter()
            .map(EngineAction::AfterburnRemove)
            .collect::<Vec<_>>();
        actions.extend(
            self.suicide_feedback
                .take_expired(now)
                .into_iter()
                .map(EngineAction::SuicideFeedbackRemove),
        );
        actions.extend(
            std::mem::take(&mut self.suicide_feedback_cleanup)
                .into_iter()
                .map(EngineAction::SuicideFeedbackRemove),
        );
        actions.extend(self.take_chicken_cleanup().into_iter().map(EngineAction::ChickenRemove));
        actions.extend(std::mem::take(&mut self.engine_actions));
        actions.extend(self.poll_chickens());
        self.queue_chicken_population();
        actions.extend(std::mem::take(&mut self.engine_actions));
        actions.extend(self.take_rollbacks());
        actions.extend(self.apply_pending(tick));
        actions.extend(self.pending_loadout_settlement_actions(tick));
        self.queue_c4_assignment();
        actions.extend(std::mem::take(&mut self.engine_actions));
        actions.extend(self.update_suicides(tick));
        self.refresh_translucent();
        self.refresh_combat();
        self.refresh_classic(None);
        self.refresh_view();
        self.refresh_periodic_movement();
        self.update_money_income();
        self.send_welcomes();
        let bomb_settings = self.controller.current_mode().and_then(|mode| mode.bomb.as_ref());
        let bounce_available = self.controller.enabled() && self.faults.capabilities().bomb_bounce;
        let bomb_update = self.bombs.update(
            bomb_settings,
            bounce_available,
            self.controller.generation(),
            &self.players,
            &mut self.rng,
        );
        self.restore_completed_native_bomb_scales();
        if let Some(handle) = self.bombs.take_native_model_scale_candidate()
            && bomb_update.is_ok()
            && self.controller.enabled()
        {
            self.randomize_model_scale(handle);
        }
        if let Err(error) = bomb_update {
            self.handle_fault(error);
        }
        actions.extend(center_display);
        actions.extend(self.take_equipment_cleanup().into_iter().map(EngineAction::Equipment));
        actions.extend(std::mem::take(&mut self.impacts).into_iter().map(EngineAction::ProjectileImpact));
        actions.extend(self.bombs.take_actions().into_iter().map(EngineAction::Bomb));
        for hit in std::mem::take(&mut self.reflections) {
            if tick > hit.tick {
                actions.push(EngineAction::Reflection(hit));
            } else {
                self.reflections.push(hit);
            }
        }
        self.update_chicken_digestion(f64::from(now));
        for hit in std::mem::take(&mut self.delayed_damage) {
            if hit.is_due(now) {
                actions.push(EngineAction::DelayedDamage(hit));
            } else {
                self.delayed_damage.push(hit);
            }
        }
        if active_combat_phase(self.controller.phase()) {
            if self.controller.current_mode().is_some_and(|mode| mode.dodgeball) {
                let touches = self.projectiles.discover_decoys(now, self.controller.generation(), &self.players);
                for handle in touches {
                    if let Err(error) = self.projectiles.register_dodgeball_touch(handle) {
                        self.handle_fault(Fault::mechanism(Mechanism::Dodgeball, error));
                        break;
                    }
                }
            }
            if self.controller.current_mode().is_some_and(|mode| mode.throwing_knives) {
                actions.extend(
                    self.projectiles
                        .take_due_knives(tick, self.controller.generation(), &self.players)
                        .into_iter()
                        .map(EngineAction::Projectile),
                );
            }
            self.projectiles.sweep(now, self.controller.generation(), &self.players);
        }
        actions.extend(self.take_projectile_cleanup().into_iter().map(EngineAction::Projectile));
        if let Some(result) = self.team_match.as_mut().and_then(|game| game.finish(now)) {
            let reason = match result.winner {
                Some(2) => s2sdk::CSRoundEndReason::TerroristWin,
                Some(3) => s2sdk::CSRoundEndReason::CTWin,
                _ => s2sdk::CSRoundEndReason::Draw,
            };
            self.broadcast(
                "team.finished",
                &[("t", &result.scores[0].to_string()), ("ct", &result.scores[1].to_string())],
            );
            self.respawns.clear();
            self.round_respawns.clear();
            actions.push(EngineAction::EndRound(self.controller.generation(), reason));
        }
        for (task, player, team, due) in std::mem::take(&mut self.respawns) {
            if !self.players.validates_respawn(task, self.controller.generation())
                || !respawn_window_open(self.controller.phase(), self.team_match.as_ref().map(|game| game.remaining(now)))
                || !respawn_target_valid(
                    player,
                    LivePlayer::resolve_present(task.player.slot),
                    team,
                    s2sdk::GetClientTeam(task.player.slot) as i32,
                    LivePlayer::resolve(task.player.slot).is_some(),
                )
            {
                continue;
            }
            if now < due {
                self.respawns.push((task, player, team, due));
            } else {
                log(&format!("respawn slot={} team={team}", task.player.slot));
                actions.push(EngineAction::Respawn(task, player, team));
            }
        }
        for (task, player, team, due) in std::mem::take(&mut self.round_respawns) {
            let all_respawn = self.controller.current_mode().is_some_and(|mode| mode.round_respawn == RoundRespawn::All);
            if !round_respawn_task_valid(&self.players, task, self.controller.generation(), player)
                || !respawn_window_open(self.controller.phase(), self.team_match.as_ref().map(|game| game.remaining(now)))
                || !round_respawn_target_valid(
                    all_respawn,
                    self.controller.phase(),
                    player,
                    LivePlayer::resolve_present(task.player.slot),
                    team,
                    s2sdk::GetClientTeam(task.player.slot) as i32,
                )
            {
                continue;
            }
            if now < due {
                self.round_respawns.push((task, player, team, due));
            } else {
                log(&format!("round respawn slot={} team={team}", task.player.slot));
                actions.push(EngineAction::RoundRespawn(task, player, team));
            }
        }
        for task in std::mem::take(&mut self.low_health_finishes) {
            if let Some(player) = LivePlayer::resolve(task.player.slot)
                && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
                && self.controller.current_mode().is_some_and(|mode| mode.self_damage_finish_enabled())
                && funrounds_core::low_health_finish(s2sdk::GetClientHealth(player.slot))
            {
                actions.push(EngineAction::LowHealthFinish(
                    task,
                    player,
                    self.controller.current_mode().is_some_and(|mode| mode.native_self_damage_enabled()),
                ));
            }
        }
        actions
    }

    fn update_money_income(&mut self) {
        let Some((generation, income, due)) = self.money_income else { return };
        if !active_combat_phase(self.controller.phase()) || self.controller.generation() != generation {
            self.money_income = None;
            return;
        }
        let now = s2sdk::GetGameTime();
        if !now.is_finite() {
            self.handle_fault(Fault::mechanism(Mechanism::Money, "game time is not finite during money income"));
            return;
        }
        if now < due {
            return;
        }
        let Some(next_due) = funrounds_core::next_money_income_due(income.interval, due, now) else {
            self.handle_fault(Fault::mechanism(Mechanism::Money, "money income schedule could not advance"));
            return;
        };
        self.money_income = Some((generation, income, next_due));
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
                continue;
            }
            if let Err(error) = crate::cash::grant_income(player, income.amount) {
                self.handle_fault(Fault::mechanism(Mechanism::Money, error));
                return;
            }
        }
    }

    fn start_money_income(&mut self) {
        self.money_income = None;
        let Some(income) = self.controller.current_mode().and_then(|mode| mode.money_income) else {
            return;
        };
        let now = s2sdk::GetGameTime();
        let due = now + income.interval;
        if now.is_finite() && due.is_finite() {
            self.money_income = Some((self.controller.generation(), income, due));
        } else {
            self.handle_fault(Fault::mechanism(Mechanism::Money, "money income schedule is not finite"));
        }
    }

    fn start_periodic_movement(&mut self) {
        self.periodic_movement = None;
        if !self
            .controller
            .current_mode()
            .is_some_and(|mode| mode.periodic_speed || mode.periodic_gravity)
        {
            return;
        }
        match funrounds_core::PeriodicMovement::start(s2sdk::GetGameTime()) {
            Ok(movement) => self.periodic_movement = Some((self.controller.generation(), movement)),
            Err(error) => self.handle_fault(Fault::mechanism(Mechanism::PeriodicMovement, error)),
        }
    }

    fn send_pending_center(&mut self, tick: i32) -> Option<EngineAction> {
        let (generation, scheduled) = self.center_display.take()?;
        let current_generation = self.controller.generation();
        let phase = self.controller.phase();
        let current = center_display_action_valid(generation == current_generation, phase, self.effects_ready);
        if center_display_due(current, phase, tick, scheduled) {
            Some(EngineAction::CenterDisplay { generation })
        } else if current {
            self.center_display = Some((generation, scheduled));
            None
        } else {
            None
        }
    }

    fn observe_tick(&mut self, event: Option<&str>, spawn_slot: Option<i32>) -> i32 {
        let tick = s2sdk::GetGameTickCount();
        let previous = self.last_tick;
        if observe_lifecycle_tick(&mut self.last_tick, tick, event) {
            log(&format!(
                "game lifecycle tick moved backwards previous={previous:?} current={tick}; discarding map state"
            ));
            // The new map may reuse old handles; old pawn properties no longer belong to us.
            self.third_person_map_detach_pending = true;
            if let Err(error) = self.retry_third_person_map_detach() {
                self.record_fault(Fault::global(error));
            }
            discard_map_effects(&mut self.effects, &mut self.retired_effects);
            self.model_scales.clear();
            self.model_scale_restore_pending.clear();
            self.chicken_entities.discard_map_state();
            self.chicken_cleanup.clear();
            self.bombs.discard_map_state();
            self.projectiles.discard_map_state();
            self.afterburns.discard_map_state();
            self.night_vision.discard_map_state();
            self.ghost.discard_map_state();
            self.suicide_feedback.discard_map_state();
            self.suicide_feedback_cleanup.clear();
            self.afterburn_cleanup.clear();
            self.refills.discard_map_state();
            self.impacts.clear();
            self.suicides.entries.clear();
            self.suicides.pending.clear();
            self.suicides.buying.clear();
            self.pending_takeovers.clear();
            self.players.clear();
            self.controllers.clear();
            self.welcomes.clear();
            self.preferences.as_mut().expect("preferences active").clear_sessions();
            self.last_name = None;
            self.restore_team_names();
            self.cleanup_effects(false);
            self.round_transition_waiting = false;
            self.controller.reset();
            self.round = None;
            self.prestarted_round = None;
            self.warmup_active = false;
            self.warmup_activation_pending = false;
        }
        self.sync_warmup(game::warmup(), tick, spawn_slot);
        tick
    }

    fn sync_warmup(&mut self, state: Option<bool>, tick: i32, spawn_slot: Option<i32>) {
        if state != Some(true) {
            if self.warmup_active {
                self.warmup_active = false;
                self.warmup_activation_pending = false;
                if self.controller.phase() == Phase::Warmup {
                    self.cleanup_effects(false);
                    self.restore_team_names();
                    self.controller.stop_warmup();
                    self.round = None;
                    self.prestarted_round = None;
                    self.finish_recovery();
                }
            } else if state.is_none() && self.controller.phase() != Phase::Idle && self.controller.phase() != Phase::Warmup {
                self.cleanup_effects(false);
                self.restore_team_names();
                self.controller.clear_active_round();
                self.round = None;
                self.prestarted_round = None;
                self.finish_recovery();
            }
            return;
        }

        if !self.warmup_active {
            self.warmup_active = true;
            self.warmup_activation_pending = true;
        } else if warmup_reactivation_needed(
            state,
            self.warmup_active,
            self.controller.phase(),
            self.warmup_activation_pending,
            self.warmup_activation_available(),
        ) {
            log("warmup is still active after the controller phase reset; reactivating");
            self.warmup_activation_pending = true;
        }
        if self.controller.phase() == Phase::Warmup {
            if self.warmup_activation_pending && self.effects_ready && tick >= 0 {
                self.activate_warmup(tick, spawn_slot);
                self.warmup_activation_pending = false;
            }
            return;
        }
        if !self.warmup_activation_pending {
            return;
        }

        if self.controller.phase() != Phase::Idle || self.round.is_some() || self.prestarted_round.is_some() {
            self.cleanup_effects(false);
            self.restore_team_names();
            self.controller.clear_active_round();
            self.round = None;
            self.prestarted_round = None;
            self.finish_recovery();
        }
        if !self.effects_ready || tick < 0 {
            return;
        }
        if !self.controller.start_warmup() {
            self.warmup_activation_pending = false;
            return;
        }
        if self.effects_ready && tick >= 0 {
            self.activate_warmup(tick, spawn_slot);
            self.warmup_activation_pending = false;
        }
    }

    fn activate_warmup(&mut self, tick: i32, skip_spawn: Option<i32>) {
        let Some(mode) = self.controller.current_mode().cloned() else { return };
        self.equipment = mode
            .select_equipment(&mut self.rng)
            .expect("validated warmup equipment")
            .into_iter()
            .map(str::to_owned)
            .collect();
        log(&format!("warmup equipment={:?}", self.equipment));
        if let Err(error) = self.parameters.apply(&mode) {
            self.handle_fault(error);
            return;
        }
        if mode.dodgeball {
            self.projectiles.baseline_decoys();
        }
        if let Some(floor) = mode.round_money_floor {
            for slot in 0..s2sdk::GetMaxClients() {
                if !s2sdk::IsClientInGame(slot) || !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
                    continue;
                }
                if let Err(error) = crate::cash::raise_to_floor(slot, floor) {
                    self.handle_fault(Fault::mechanism(Mechanism::Money, error));
                    return;
                }
            }
        }
        self.start_money_income();
        self.start_periodic_movement();
        for slot in 0..s2sdk::GetMaxClients() {
            if !s2sdk::IsClientInGame(slot) || !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
                continue;
            }
            if LivePlayer::resolve(slot).is_some() {
                if Some(slot) != skip_spawn {
                    self.spawn(slot, tick, true);
                }
            } else if let Some(respawn) = mode.respawn.filter(|respawn| respawn.team.matches(s2sdk::GetClientTeam(slot) as i32)) {
                self.queue_respawn(slot, respawn.delay_seconds);
            }
        }
        self.queue_c4_assignment();
    }

    fn reconcile_warmup_players(&mut self, tick: i32) {
        let respawn = self.controller.current_mode().and_then(|mode| mode.respawn);
        for slot in 0..s2sdk::GetMaxClients() {
            if !s2sdk::IsClientInGame(slot) {
                continue;
            }
            let team = s2sdk::GetClientTeam(slot) as i32;
            if !matches!(team, 2 | 3) {
                continue;
            }
            self.connect(slot);
            if let Some(player) = LivePlayer::resolve(slot) {
                if self.warmup_loadout_needed(player) {
                    self.spawn(slot, tick, true);
                }
            } else if respawn.is_some_and(|respawn| respawn.team.matches(team)) {
                let respawn = respawn.expect("checked above");
                self.queue_respawn(slot, respawn.delay_seconds);
            }
        }
    }

    fn warmup_loadout_needed(&self, player: LivePlayer) -> bool {
        let identity = self.players.identity(player.slot);
        let generation = self.controller.generation();
        let effect_applied = identity.is_some_and(|identity| {
            self.effects
                .get(&player.slot)
                .is_some_and(|effect| effect.identity == identity && effect.round == generation)
        });
        let loadout_pending = identity.is_some_and(|identity| {
            let task = PlayerTask {
                player: identity,
                round: generation,
            };
            self.pending
                .iter()
                .any(|(pending, _, action)| *pending == task && *action == PendingAction::Loadout)
        });
        warmup_spawn_needed(identity, player, effect_applied, loadout_pending)
    }

    fn apply_pending(&mut self, tick: i32) -> Vec<EngineAction> {
        let mut actions = Vec::new();
        let pending = std::mem::take(&mut self.pending);
        for (mut task, scheduled, action) in pending {
            if task.round != self.controller.generation() || self.controller.phase() == Phase::Idle {
                self.cancel_pending(action);
                continue;
            }
            if tick <= scheduled {
                self.pending.push((task, scheduled, action));
                continue;
            }
            let Some(player) = LivePlayer::resolve(task.player.slot) else {
                if self.players.identity(task.player.slot) == Some(task.player)
                    || self.players.pending_identity(task.player.slot) == Some(task.player)
                {
                    self.pending.push((task, scheduled, action));
                } else {
                    self.cancel_pending(action);
                }
                continue;
            };
            if self.controllers.get(&player.slot) != Some(&player.controller_handle) {
                self.cancel_pending(action);
                continue;
            }
            if task.player.pawn_handle == u32::MAX {
                let Some(identity) = self.players.bind_spawn(task.player, player.pawn_handle as u32) else {
                    continue;
                };
                let base_health = self.chickens.base_health(task).ok();
                self.chickens.clear_life(task);
                if let Some((combat_task, _)) = self.combat.get_mut(&player.slot)
                    && *combat_task == task
                {
                    combat_task.player = identity;
                }
                task.player = identity;
                if let Some(base_health) = base_health {
                    self.chickens.observe_life(task, base_health);
                }
            }
            if !self.players.validates(task, self.controller.generation(), player.pawn_handle as u32) {
                self.cancel_pending(action);
                continue;
            }
            if action == PendingAction::RecoilSettle {
                if !self.controller.current_mode().is_some_and(|mode| mode.no_recoil) {
                    continue;
                }
                let Some(effect) = self.effects.get(&player.slot).filter(|effect| effect.identity == task.player) else {
                    continue;
                };
                match effect.settle_recoil() {
                    Ok(diagnostic) => {
                        if let Some(diagnostic) = diagnostic {
                            log(&diagnostic);
                            self.queue(task.player, tick, PendingAction::RecoilObservation);
                        }
                    }
                    Err(error) => {
                        self.handle_fault(error.context("post-shot recoil cleanup failed"));
                        continue;
                    }
                }
                continue;
            }
            if action == PendingAction::RecoilObservation {
                if self.diagnostics()
                    && let Some(effect) = self.effects.get(&player.slot).filter(|effect| effect.identity == task.player)
                {
                    match effect.observe_recoil() {
                        Ok(diagnostic) => log(&diagnostic),
                        Err(error) => log(&format!("post-shot diagnostic unavailable: {error}")),
                    }
                }
                continue;
            }
            if let PendingAction::AmmoObservation(handle) = action {
                if self.diagnostics()
                    && self.controller.current_mode().is_some_and(|mode| mode.ammo == Ammo::Clip)
                    && let Some(effect) = self.effects.get(&player.slot).filter(|effect| effect.identity == task.player)
                {
                    match effect.observe_ammo(handle) {
                        Ok(diagnostic) => log(&diagnostic),
                        Err(error) => log(&format!("post-shot ammo unavailable: {error}")),
                    }
                }
                continue;
            }
            if let PendingAction::Grenade(id) = action {
                if let Some(action) = self.refills.start(id) {
                    actions.push(EngineAction::Equipment(action));
                }
                continue;
            }
            if self.effects.get(&player.slot).is_some_and(|effect| effect.identity == task.player) {
                continue;
            }
            let Some(mode) = self.controller.current_mode() else { continue };
            let equipment = if mode.random_loadout {
                mode.select_equipment(&mut self.rng)
                    .expect("validated individual loadout")
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            } else if mode.kill_loadout && mode.kill_loadout_start == Some(funrounds_core::KillLoadoutStart::Random) {
                random_kill_loadout_start(mode, &mut self.rng)
            } else {
                self.equipment.clone()
            };
            actions.push(EngineAction::Loadout(task, player, equipment));
        }
        actions
    }

    fn cleanup_before_new_round(&mut self, key: RoundKey) {
        if self.controller.is_new_round(key) {
            self.hostage_rescue.clear_attempts();
            if let Some(previous) = self.round {
                self.controller.end_round(previous);
            }
            self.round = None;
            self.cleanup_round_effects(false);
            self.restore_team_names();
        }
    }

    fn quickdefuse_wire(&mut self, slot: i32, wire: crate::bombs::WireColor) -> Result<String, String> {
        if !self.classic_enabled() || !active_combat_phase(self.controller.phase()) || self.controller.current_mode().is_none() {
            return Err("QuickDefuse is available only during enabled combat".into());
        }
        if !self.controller.capabilities().bomb_timing {
            return Err("QuickDefuse's native C4 schema capability is unavailable".into());
        }
        if slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) || !s2sdk::IsClientAlive(slot) {
            return Err("QuickDefuse requires a live in-game human player".into());
        }
        if s2sdk::IsFakeClient(slot) {
            return Err("QuickDefuse wire commands are for human players".into());
        }
        let round = self.controller.generation();
        let task = self
            .players
            .identity(slot)
            .map(|player| PlayerTask { player, round })
            .ok_or_else(|| "QuickDefuse requires the current player life".to_owned())?;
        let live = LivePlayer::resolve(slot).ok_or_else(|| "QuickDefuse requires a live player pawn".to_owned())?;
        if live.pawn_handle as u32 != task.player.pawn_handle {
            return Err("QuickDefuse player identity changed; start again with the current pawn".into());
        }
        let team = s2sdk::GetClientTeam(slot) as i32;
        if self.bombs.is_planting_wire_window(round, task) {
            self.bombs.select_plant_wire(round, task, wire).map_err(str::to_owned)?;
            return Ok(self.text(slot, "wire.plant_selected", &[("wire", wire.name())]));
        }
        if self.bombs.quick_defuser_slot() != Some(slot) {
            return Err(self.text(slot, "wire.no_defuse", &[]));
        }
        if team != 3 {
            return Err(self.text(slot, "wire.ct_required", &[]));
        }
        match self.bombs.quick_cut(
            crate::bombs::QuickCutRequest::new(round, live, task, team, wire),
            &self.players,
            &mut self.rng,
        ) {
            Ok(crate::bombs::QuickCutOutcome::Defused) => Ok(self.text(slot, "wire.defused", &[("wire", wire.name())])),
            Ok(crate::bombs::QuickCutOutcome::Exploded) => Ok(self.text(slot, "wire.exploded", &[("wire", wire.name())])),
            Err(crate::bombs::WireCommandError::Rejected(error)) => Err(error.into()),
            Err(crate::bombs::WireCommandError::Fault(fault)) => {
                let message = fault.to_string();
                self.handle_fault(fault);
                Err(message)
            }
        }
    }

    fn select_round(&mut self, key: RoundKey, warmup: bool, effects_ready: bool) -> bool {
        let had_pending = self.controller.pending_config().is_some();
        let selected = self
            .controller
            .start_round(key, round_selection_waits(warmup, effects_ready), &mut self.rng);
        if had_pending && self.controller.pending_config().is_none() {
            self.texts = self.pending_texts.take().expect("queued configuration and text catalog are paired");
        }
        if selected && !warmup {
            // RoundController advances the player-action generation once more when selecting
            // the round after prestart cleanup. Retag preserved live entities to that identity.
            self.advance_chicken_round(self.controller.generation());
            if self.controller.config().is_none_or(|config| config.chicken.is_none())
                || self.controller.current_mode().is_some_and(|mode| !mode.ordinary_chickens)
            {
                self.cleanup_ordinary_chickens();
            }
        }
        selected
    }

    fn prepare_round(&mut self, key: RoundKey, warmup: bool) -> bool {
        if self.controller.is_new_round(key) {
            self.cleanup_before_new_round(key);
            self.finish_round_transition_recovery();
        }
        self.select_round(key, warmup, self.effects_ready)
    }

    fn cancel_pending(&mut self, action: PendingAction) {
        if let PendingAction::Grenade(id) = action {
            self.refills.cancel(id);
        }
    }

    pub fn event(&mut self, name: &str, event: usize) {
        let spawn_slot = (name == "player_spawn").then(|| s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid")));
        self.observe_tick(Some(name), spawn_slot);
        if name != "item_pickup" {
            self.refresh_weapon_inventory();
        }
        match name {
            "item_pickup" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.item_pickup(slot, event);
                self.refresh_weapon_inventory();
            }
            "round_prestart" => {
                self.prestart_context = None;
                self.prestarted_round = None;
                self.bombs.begin_round_prestart_cleanup();
                self.pending_takeovers.clear();
                self.suicides.cancel_round_end_finishes();
                // Native C4 allocation precedes round_start, even when a restart omits round_end.
                if let Err(error) = self.parameters.restore_c4_assignment() {
                    self.handle_fault(Fault::global(error));
                }
                let tick = s2sdk::GetGameTickCount();
                if tick < 0 {
                    self.prestarted_round = None;
                    self.prestart_context = None;
                    if let Err(error) = self.bombs.cleanup() {
                        self.handle_fault(Fault::global(error));
                    }
                    self.bombs.finish_round_prestart_cleanup();
                    return;
                }
                let key = self.controller.round_key(tick as u64);
                if game::warmup() != Some(true) {
                    if self.controller.is_new_round(key) {
                        self.cleanup_before_new_round(key);
                    } else if let Err(error) = self.bombs.cleanup() {
                        self.handle_fault(Fault::global(error));
                    }
                }
                self.prestart_context = Some((tick, game::warmup() != Some(false)));
            }
            "bomb_exploded" => self.bombs.capture_explosion(self.controller.generation()),
            "bomb_beginplant" | "bomb_begindefuse" => {
                let Some(mode) = self.controller.current_mode() else { return };
                if !active_combat_phase(self.controller.phase()) {
                    return;
                }
                let has_bomb_timing = mode.bomb.is_some();
                let quick_defuse_available = self.controller.capabilities().bomb_timing;
                let round = self.controller.generation();
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                let task = self.players.identity(slot).map(|player| PlayerTask { player, round });
                let current_human = task.is_some_and(|task| {
                    slot >= 0
                        && s2sdk::IsClientInGame(slot)
                        && s2sdk::IsClientAlive(slot)
                        && !s2sdk::IsFakeClient(slot)
                        && LivePlayer::resolve(slot).is_some_and(|live| live.pawn_handle as u32 == task.player.pawn_handle)
                });
                if name == "bomb_beginplant" {
                    self.bombs.begin_plant(round, task);
                    if quick_defuse_available && current_human && self.bombs.is_planting_wire_window(round, task.expect("plant task checked")) {
                        self.say(slot, "wire.plant_help", &[]);
                    }
                } else if let Some(task) = task {
                    if has_bomb_timing && let Err(error) = self.bombs.begin_defuse(task, s2sdk::GetEventBool(event, &Str::from_str("haskit"))) {
                        self.handle_fault(error);
                        return;
                    }
                    if quick_defuse_available
                        && current_human
                        && s2sdk::GetClientTeam(slot) as i32 == 3
                        && self
                            .bombs
                            .begin_quick_defuse(round, task, s2sdk::GetEventBool(event, &Str::from_str("haskit")))
                    {
                        self.say(slot, "wire.defuse_help", &[]);
                    }
                }
            }
            "bomb_abortplant" => {
                if let Err(error) = self.bombs.abort_plant() {
                    self.handle_fault(error);
                }
            }
            "bomb_abortdefuse" => {
                self.bombs.abort_quick_defuse();
                if let Err(error) = self.bombs.abort_defuse() {
                    self.handle_fault(error);
                }
            }
            "bomb_planted" => {
                if active_combat_phase(self.controller.phase())
                    && let Some(mode) = self.controller.current_mode()
                {
                    let kind = mode.bomb.as_ref().map_or(funrounds_core::BombKind::Native, |settings| settings.kind);
                    let round = self.controller.generation();
                    let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                    let task = self.players.identity(slot).map(|player| PlayerTask { player, round });
                    let team = if slot >= 0 && s2sdk::IsClientInGame(slot) {
                        s2sdk::GetClientTeam(slot) as i32
                    } else {
                        -1
                    };
                    if let Err(error) = self.bombs.planted(round, task, team, s2sdk::GetGameTickCount(), kind, &mut self.rng) {
                        self.handle_fault(Fault::mechanism(Mechanism::CtBombNoDefuse, error));
                    }
                }
            }
            "round_start" => {
                let tick = s2sdk::GetGameTickCount();
                if tick < 0 {
                    return;
                }
                let (key, purchase_prepared) = self
                    .prestarted_round
                    .take()
                    .map_or((self.controller.round_key(tick as u64), false), |key| (key, true));
                let started = if purchase_prepared {
                    if self.confirm_prestarted_round_reset() {
                        self.round_transition_waiting = false;
                        true
                    } else {
                        self.controller.end_round(key);
                        let waiting = self.cleanup_after_round_start_wait();
                        self.restore_team_names();
                        if waiting {
                            self.finish_round_transition_recovery();
                        } else {
                            self.finish_recovery();
                        }
                        false
                    }
                } else {
                    self.prepare_round(key, game::warmup() != Some(false))
                };
                if started {
                    if self.controller.phase() != Phase::Idle {
                        self.round_transition_waiting = false;
                    }
                    self.round = Some(key);
                    if !self.classic_enabled() {
                        self.restore_team_names();
                        self.welcomes.clear();
                    }
                    if let Some(mode) = self.controller.current_mode() {
                        self.equipment = mode
                            .select_equipment(&mut self.rng)
                            .expect("active mode was validated")
                            .into_iter()
                            .map(str::to_owned)
                            .collect();
                        log(&format!(
                            "round mode={} name={} equipment={:?} headshots={:?}",
                            mode.id, mode.name, self.equipment, mode.headshots
                        ));
                        let apply = if purchase_prepared {
                            self.parameters.apply_after_prestart(mode)
                        } else {
                            self.parameters.apply(mode)
                        };
                        if let Err(error) = apply {
                            self.handle_fault(error);
                            return;
                        }
                        if let Some(settings) = mode.ghost {
                            log(&format!("ghost policy mode={} settings={settings:?}", mode.id));
                        }
                        if mode.dodgeball {
                            self.projectiles.baseline_decoys();
                        }
                        if parameters::has_convar("mp_teamname_1") && parameters::has_convar("mp_teamname_2") {
                            let ghost_names = match mode.ghost.map(crate::effects::ghost_team_names).transpose() {
                                Ok(names) => names.flatten(),
                                Err(error) => {
                                    self.handle_fault(error);
                                    return;
                                }
                            };
                            let ct_name = ghost_names.map_or_else(
                                || {
                                    mode.localized_name(&self.texts, self.texts.default_language())
                                        .expect("validated team name")
                                },
                                |(ct, _)| ct.to_owned(),
                            );
                            let t_name = ghost_names.map_or_else(
                                || {
                                    self.last_name
                                        .as_ref()
                                        .map_or("", |names| names.for_default(self.texts.default_language()))
                                },
                                |(_, t)| t,
                            );
                            let display = self
                                .team_names
                                .set_string("mp_teamname_1", &ct_name)
                                .and_then(|()| self.team_names.set_string("mp_teamname_2", t_name));
                            if let Err(error) = display {
                                self.handle_fault(error);
                                return;
                            }
                        }
                        for slot in 0..s2sdk::GetMaxClients() {
                            if !s2sdk::IsClientInGame(slot) {
                                continue;
                            }
                            let language = crate::i18n::language(slot, &self.texts);
                            for line in mode.localized_chat(&self.texts, &language).expect("validated mode announcement") {
                                crate::i18n::send_chat(slot, &line);
                            }
                            let console =
                                separate_console_output(mode.localized_console(&self.texts, &language).expect("validated console announcement"));
                            for part in crate::i18n::console_parts(&console) {
                                s2sdk::PrintToConsole(slot, &Str::from_str(part));
                            }
                        }
                        if let Some(floor) = mode.round_money_floor {
                            for slot in 0..s2sdk::GetMaxClients() {
                                if !s2sdk::IsClientInGame(slot) || !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
                                    continue;
                                }
                                if let Err(error) = crate::cash::raise_to_floor(slot, floor) {
                                    self.handle_fault(Fault::mechanism(Mechanism::Money, error));
                                    return;
                                }
                            }
                        }
                        self.center_display = Some((self.controller.generation(), tick));
                        self.last_name = Some(crate::i18n::ModeNames::capture(mode, &self.texts).expect("validated previous team name"));
                        let round_respawn = mode.round_respawn;
                        for slot in 0..s2sdk::GetMaxClients() {
                            if round_respawn == RoundRespawn::All {
                                self.queue_round_respawn(slot, tick, 0.1);
                            } else if LivePlayer::resolve(slot).is_some() {
                                self.spawn(slot, tick, true);
                            } else if round_respawn == RoundRespawn::Dead {
                                self.queue_respawn(slot, 0.1);
                            }
                        }
                    }
                }
            }
            "round_freeze_end" => {
                if let Some(round) = self.round {
                    let started = self.controller.freeze_end(round);
                    if started {
                        self.queue_c4_assignment();
                        self.start_money_income();
                        self.start_periodic_movement();
                    }
                    if started
                        && self
                            .controller
                            .current_mode()
                            .is_some_and(|mode| mode.win_condition == funrounds_core::WinCondition::TeamDeathmatch)
                    {
                        match game::round_duration().and_then(|duration| TeamDeathmatch::start(s2sdk::GetGameTime(), duration)) {
                            Ok(game) => {
                                log(&format!("team match started duration={}", game.remaining(s2sdk::GetGameTime())));
                                self.team_match = Some(game);
                            }
                            Err(error) => self.handle_fault(Fault::mechanism(Mechanism::TeamDeathmatch, error)),
                        }
                    }
                }
            }
            "round_end" => {
                if let Some(round) = self.round
                    && self.controller.end_round(round)
                {
                    self.cleanup_round_effects(true);
                }
            }
            "player_spawn" => self.spawn(
                s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid")),
                s2sdk::GetGameTickCount(),
                true,
            ),
            "grenade_thrown" => {
                let weapon = s2sdk::GetEventString(event, &Str::from_str("weapon")).to_string();
                let kind = match weapon.strip_prefix("weapon_").unwrap_or(&weapon) {
                    "hegrenade" => Some(equipment::GrenadeKind::He),
                    "flashbang" => Some(equipment::GrenadeKind::Flash),
                    "smokegrenade" => Some(equipment::GrenadeKind::Smoke),
                    "decoy" => Some(equipment::GrenadeKind::Decoy),
                    "molotov" => Some(equipment::GrenadeKind::Molotov),
                    "incgrenade" => Some(equipment::GrenadeKind::Incendiary),
                    _ => None,
                };
                let route = grenade_thrown_route(self.controller.current_mode(), kind);
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                if route.track_dodgeball_decoy
                    && let Some(identity) = self.players.identity(slot)
                {
                    let touches = self.projectiles.observe_decoy_throw(
                        PlayerTask {
                            player: identity,
                            round: self.controller.generation(),
                        },
                        s2sdk::GetGameTime(),
                    );
                    for handle in touches {
                        if let Err(error) = self.projectiles.register_dodgeball_touch(handle) {
                            self.handle_fault(Fault::mechanism(Mechanism::Dodgeball, error));
                            break;
                        }
                    }
                }
                if route.replenish
                    && let Some(kind) = kind
                    && let Some(identity) = self.players.identity(slot)
                    && let Some(player) = LivePlayer::resolve(slot)
                    && player.pawn_handle as u32 == identity.pawn_handle
                    && self.controllers.get(&slot) == Some(&player.controller_handle)
                {
                    let task = PlayerTask {
                        player: identity,
                        round: self.controller.generation(),
                    };
                    let (id, spent) = self.refills.queue(task, player, kind, self.diagnostics());
                    self.queue(identity, s2sdk::GetGameTickCount(), PendingAction::Grenade(id));
                    if self.diagnostics() {
                        log(&format!(
                            "grenade refill queued slot={slot} weapon={weapon} spent_handle={spent:?} transaction={id}"
                        ));
                    }
                }
            }
            "weapon_fire" => {
                let weapon = s2sdk::GetEventString(event, &Str::from_str("weapon")).to_string();
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                if matches!(weapon.as_str(), "knife" | "knife_t" | "weapon_knife" | "weapon_knife_t")
                    && active_combat_phase(self.controller.phase())
                    && self.controller.current_mode().is_some_and(|mode| mode.throwing_knives)
                {
                    self.native_self_damage_shots.remove(&slot);
                    if let Some(player) = self.players.identity(slot) {
                        self.projectiles.schedule_knife(
                            PlayerTask {
                                player,
                                round: self.controller.generation(),
                            },
                            s2sdk::GetGameTickCount(),
                        );
                    }
                    return;
                }
                if !crate::effects::firearm_event(&weapon) {
                    self.native_self_damage_shots.remove(&slot);
                    return;
                }
                let Some(player) = LivePlayer::resolve(slot) else {
                    self.native_self_damage_shots.remove(&slot);
                    return;
                };
                let Some(mode) = self.controller.current_mode() else {
                    self.native_self_damage_shots.remove(&slot);
                    return;
                };
                let identity = self.players.identity(slot);
                if active_combat_phase(self.controller.phase())
                    && matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if amount > 0.0)
                    && let Some(task) = self.combat_task(player)
                    && let Some(weapon) = fired_weapon(player, &weapon)
                {
                    self.native_self_damage_shots.insert(
                        slot,
                        NativeSelfDamageShot {
                            task,
                            player,
                            tick: s2sdk::GetGameTickCount(),
                            weapon,
                            player_hit: false,
                        },
                    );
                } else {
                    self.native_self_damage_shots.remove(&slot);
                }
                let knock = self
                    .preferences
                    .as_mut()
                    .expect("preferences active")
                    .value(slot, crate::preferences::authenticated_id(slot));
                if mode.self_push
                    && self.classic_round()
                    && matches!(self.controller.phase(), Phase::Running | Phase::Warmup)
                    && knock.permits(s2sdk::GetClientFlags(slot))
                    && self.controllers.get(&slot) == Some(&player.controller_handle)
                    && let Some(effect) = self.effects.get(&slot).filter(|effect| {
                        Some(effect.identity) == identity && effect.identity.pawn_handle == player.pawn_handle as u32 && !effect.has_pending_loadout()
                    })
                    && let Err(error) = effect.self_push(player, &weapon)
                {
                    self.handle_fault(error);
                    return;
                }
                if matches!(mode.self_damage, Some(SelfDamage::MissPenalty))
                    && funrounds_core::low_health_finish(s2sdk::GetClientHealth(slot))
                    && let Some(player) = identity
                {
                    let task = PlayerTask {
                        player,
                        round: self.controller.generation(),
                    };
                    if !self.low_health_finishes.contains(&task) {
                        self.low_health_finishes.push(task);
                    }
                }
                let diagnostics = self.diagnostics();
                let no_recoil = mode.no_recoil;
                let recoil_effect = no_recoil || mode.recoil_view.is_some();
                let observe_ammo = diagnostics && mode.ammo == Ammo::Clip;
                if !diagnostics && !recoil_effect {
                    return;
                }
                let Some(effect) = self
                    .effects
                    .get_mut(&slot)
                    .filter(|effect| Some(effect.identity) == identity && !effect.has_pending_loadout())
                else {
                    return;
                };
                match effect.weapon_fire(player, mode) {
                    Ok(diagnostic) => {
                        if let Some(diagnostic) = diagnostic {
                            log(&diagnostic);
                        }
                        if observe_ammo && let Some(identity) = identity {
                            self.queue(
                                identity,
                                s2sdk::GetGameTickCount(),
                                PendingAction::AmmoObservation(s2sdk::GetClientActiveWeapon(player.slot)),
                            );
                        }
                        if no_recoil && let Some(identity) = identity {
                            self.queue(identity, s2sdk::GetGameTickCount(), PendingAction::RecoilSettle);
                        }
                    }
                    Err(error) => {
                        if !recoil_effect {
                            log(&format!("shooting diagnostic unavailable: {error}"));
                            return;
                        }
                        self.handle_fault(error.context("shooting effect failed"));
                    }
                }
            }
            "player_disconnect" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.bombs.abandon_quick_defuse_player(slot);
                self.forget_player(slot);
            }
            "player_death" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                let attacker = s2sdk::GetEventPlayerSlot(event, &Str::from_str("attacker"));
                let chicken_weapon_source = self.take_chicken_blast_kill_weapon(slot, attacker);
                self.bombs.abandon_quick_defuse_player(slot);
                if let Some(identity) = self.players.identity(slot) {
                    self.clear_chicken_life(identity);
                }
                self.cancel_chicken_purchase(slot);
                self.native_self_damage_shots.remove(&slot);
                self.pending_takeovers.retain(|pending| pending.slot != slot);
                self.projectiles
                    .retire_player(slot, self.players.identity(slot).map(|player| player.pawn_handle));
                if self.diagnostics() && self.controller.current_mode().is_some_and(|mode| mode.headshots == Headshots::Only) {
                    log(&format!(
                        "death victim={slot} attacker={attacker} weapon={} headshot={}",
                        s2sdk::GetEventString(event, &Str::from_str("weapon")),
                        s2sdk::GetEventBool(event, &Str::from_str("headshot"))
                    ));
                }
                if active_combat_phase(self.controller.phase()) {
                    if self.controller.current_mode().is_some_and(|mode| mode.kill_protection)
                        && let Some(player) = LivePlayer::resolve(attacker)
                        && !s2sdk::IsFakeClient(attacker)
                        && let Some(task) = self.combat_task(player)
                        && let Some((expected, life)) = self.combat.get_mut(&attacker)
                        && *expected == task
                    {
                        life.protect(s2sdk::GetGameTime());
                    }
                    self.kill_loadout(
                        attacker,
                        slot,
                        &s2sdk::GetEventString(event, &Str::from_str("weapon")),
                        chicken_weapon_source,
                    );
                }
                if self.controller.phase() == Phase::Running
                    && attacker >= 0
                    && slot >= 0
                    && let Some(game) = self.team_match.as_mut()
                    && game.record_kill(
                        s2sdk::GetGameTime(),
                        s2sdk::GetClientTeam(attacker) as i32,
                        s2sdk::GetClientTeam(slot) as i32,
                        attacker != slot,
                    )
                {
                    log(&format!("team score T={} CT={}", game.scores()[0], game.scores()[1]));
                }
                if let Err(error) = self.suicides.forget(slot) {
                    self.record_fault(Fault::global(error));
                }
                self.players.death(slot);
                self.round_respawns.retain(|(task, ..)| task.player.slot != slot);
                self.combat.remove(&slot);
                self.delayed_damage.retain(|hit| hit.target.player.slot != slot);
                self.refills.cancel_player(slot);
                self.retire_effect(slot);
                self.pending.retain(|(task, _, _)| task.player.slot != slot);
                if let Some(respawn) = self.controller.current_mode().and_then(|mode| mode.respawn.as_ref())
                    && respawn.team.matches(s2sdk::GetClientTeam(slot) as i32)
                {
                    self.queue_respawn(slot, respawn.delay_seconds);
                }
            }
            "player_hurt" => {
                let victim = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                let attacker = s2sdk::GetEventPlayerSlot(event, &Str::from_str("attacker"));
                let weapon = s2sdk::GetEventString(event, &Str::from_str("weapon")).to_string();
                if self.diagnostics()
                    && self.controller.current_mode().is_some_and(|mode| {
                        mode.lifesteal
                            || matches!(mode.self_damage, Some(SelfDamage::MissPenalty))
                            || mode.headshots == Headshots::Only
                            || (mode.win_condition == funrounds_core::WinCondition::TeamDeathmatch)
                    })
                {
                    log(&format!(
                        "hurt victim={victim} attacker={attacker} weapon={} dmg_health={} dmg_armor={} health={} armor={} hitgroup={}",
                        weapon,
                        s2sdk::GetEventInt(event, &Str::from_str("dmg_health")),
                        s2sdk::GetEventInt(event, &Str::from_str("dmg_armor")),
                        s2sdk::GetEventInt(event, &Str::from_str("health")),
                        s2sdk::GetEventInt(event, &Str::from_str("armor")),
                        s2sdk::GetEventInt(event, &Str::from_str("hitgroup"))
                    ));
                }
                self.suicide_hurt(event, victim, attacker);
                let healed = match crate::damage::internal_damage_hurt(victim, attacker) {
                    Ok(true) => None,
                    Ok(false) => self.lifesteal(event, attacker, victim),
                    Err(error) => {
                        self.handle_fault(error);
                        None
                    }
                };
                let random_healed = self.settle_random_heal(victim, attacker, event);
                self.refresh_classic(Some(&[victim, random_healed.or(healed).unwrap_or(-1)]));
            }
            "player_connect_full" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.connect(slot);
                if self.controller.phase() == Phase::Warmup {
                    if let Some(player) = LivePlayer::resolve(slot) {
                        if self.warmup_loadout_needed(player) {
                            self.spawn(slot, s2sdk::GetGameTickCount(), true);
                        }
                    } else if let Some(respawn) = self.controller.current_mode().and_then(|mode| mode.respawn)
                        && respawn.team.matches(s2sdk::GetClientTeam(slot) as i32)
                    {
                        self.queue_respawn(slot, respawn.delay_seconds);
                    }
                }
            }
            "bot_takeover" => {
                self.queue_bot_takeover(s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid")));
            }
            "player_team" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.bombs.abandon_quick_defuse_player(slot);
                if let Some(identity) = self.players.identity(slot) {
                    self.clear_chicken_life(identity);
                }
                self.cancel_chicken_purchase(slot);
                self.native_self_damage_shots.remove(&slot);
                self.pending_takeovers.retain(|pending| pending.slot != slot);
                if let Err(error) = self.suicides.forget(slot) {
                    self.handle_fault(Fault::global(error));
                }
                self.players.death(slot);
                self.combat.remove(&slot);
                self.reflections
                    .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
                self.delayed_damage
                    .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
                self.random_heals
                    .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
                self.refills.cancel_player(slot);
                self.pending.retain(|(task, ..)| task.player.slot != slot);
                self.retire_effect(slot);
                self.respawns.retain(|(task, _, _, _)| task.player.slot != slot);
                self.round_respawns.retain(|(task, ..)| task.player.slot != slot);
            }
            _ => {}
        }
    }

    pub(crate) fn handle_fault(&mut self, fault: impl Into<Fault>) {
        let fault = fault.into();
        if fault.scope == Scope::Cancelled {
            log(&format!("action cancelled: {fault}"));
            return;
        }
        self.record_fault(fault);
        self.cleanup_effects(false);
        self.restore_team_names();
    }

    fn record_fault(&mut self, fault: Fault) {
        self.round_transition_waiting = false;
        if fault.scope == Scope::Global {
            crate::legacy_textmsg::set_enabled(false);
        }
        let changed = self.faults.record(&fault);
        self.prestarted_round = None;
        self.controller.set_capabilities(self.faults.capabilities());
        if let Some(round) = self.round {
            self.controller.end_round(round);
        }
        self.effects_ready = false;
        self.welcomes.clear();
        if changed {
            log(&format!("fault scope={:?} recovery={:?}: {fault}", fault.scope, self.faults.recovery));
        }
    }

    fn cleanup_complete(&self) -> bool {
        self.cleanup_complete_without_bombs()
            && self.bombs.is_clean()
            && !crate::CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING.load(std::sync::atomic::Ordering::Acquire)
    }

    fn cleanup_complete_without_bombs(&self) -> bool {
        self.cleanup_complete_except_prestart_parameters_and_bombs()
            && self.parameters.is_restored()
            // Healthshot ConVars are configuration-scoped; an in-sync active override is not unfinished round cleanup.
            && healthshot_override_settled(
                self.healthshot_active(),
                self.healthshot_override_active,
                self.healthshot_parameters.is_restored(),
            )
    }

    fn cleanup_complete_for_round_transition_without_bombs(&self) -> bool {
        self.cleanup_complete_for_round_transition_except_prestart_parameters_and_bombs()
            && self.parameters.is_restored()
            // Healthshot ConVars are configuration-scoped; an in-sync active override is not unfinished round cleanup.
            && healthshot_override_settled(
                self.healthshot_active(),
                self.healthshot_override_active,
                self.healthshot_parameters.is_restored(),
            )
    }

    fn cleanup_complete_except_prestart_parameters_and_bombs(&self) -> bool {
        self.cleanup_complete_except_pending_and_prestart_parameters_and_bombs() && self.pending.is_empty()
    }

    fn cleanup_complete_for_round_transition_except_prestart_parameters_and_bombs(&self) -> bool {
        self.cleanup_complete_for_round_transition_except_pending_and_prestart_parameters_and_bombs() && self.pending.is_empty()
    }

    fn cleanup_complete_except_pending_and_prestart_parameters_and_bombs(&self) -> bool {
        self.cleanup_complete_except_pending_and_prestart_parameters_and_bombs_with_round_lifetime(false)
    }

    fn cleanup_complete_for_round_transition_except_pending_and_prestart_parameters_and_bombs(&self) -> bool {
        self.cleanup_complete_except_pending_and_prestart_parameters_and_bombs_with_round_lifetime(true)
    }

    fn cleanup_complete_except_pending_and_prestart_parameters_and_bombs_with_round_lifetime(&self, round_transition: bool) -> bool {
        !self.third_person_map_detach_pending
            && self.ground_reverse.is_clean()
            && self.ghost.is_clean()
            && self.night_vision.is_clean()
            && public_property_cleanup_complete(self.controller.enabled(), self.duck_speed.is_empty(), self.model_scales.is_empty())
            && self.equipment_in_flight == 0
            && self.suicides.entries.is_empty()
            && self.suicides.pending.is_empty()
            && self.suicides.buying.is_empty()
            && (self.chicken_entities.is_empty() || (round_transition && self.chicken_entities.has_only_active_chickens()))
            && self.chicken_cleanup.is_empty()
            && (self.suicide_feedback.is_empty() || (round_transition && self.suicide_feedback.has_only_unexpired(s2sdk::GetGameTime())))
            && self.suicide_feedback_cleanup.is_empty()
            && self.afterburns.is_clean()
            && self.afterburn_cleanup.is_empty()
            && self.suicides.ff.is_restored()
            && self.effects.is_empty()
            && self.retired_effects.is_empty()
            && self.team_names.is_restored()
            && self.refills.is_clean()
            && self.projectiles.is_clean()
            && self.impacts.is_empty()
    }

    fn pending_prestarted_loadouts_are_current(&self) -> bool {
        let generation = self.controller.generation();
        self.pending.iter().all(|(task, _, action)| {
            let live_player = LivePlayer::resolve(task.player.slot);
            let pawn_controller = live_player.and_then(LivePlayer::pawn_controller);
            prestarted_loadout_is_current(
                *task,
                *action,
                generation,
                self.players.identity(task.player.slot),
                live_player,
                self.controllers.get(&task.player.slot).copied(),
                pawn_controller,
            )
        })
    }

    fn finish_recovery(&mut self) {
        let complete = self.cleanup_complete();
        if self.faults.finish_cleanup(complete) {
            log("owned cleanup complete; quarantined capabilities remain excluded; waiting for a new round");
        }
        self.effects_ready = complete && self.faults.recovery == Recovery::Ready;
    }

    fn finish_round_transition_recovery(&mut self) {
        let complete = self.cleanup_complete_for_round_transition_without_bombs()
            && self.bombs.is_clean()
            && !crate::CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING.load(std::sync::atomic::Ordering::Acquire);
        if self.faults.finish_cleanup(complete) {
            log("owned cleanup complete; quarantined capabilities remain excluded; waiting for a new round");
        }
        self.effects_ready = complete && self.faults.recovery == Recovery::Ready;
    }

    fn recover_incomplete_effects(&mut self) {
        if !self.effects_ready && self.prestarted_round.is_none() {
            let waiting = self.cleanup_after_round_start_wait();
            self.restore_team_names();
            if !self.controller.enabled() {
                self.restore_duck_speed();
                self.restore_model_scales();
            }
            if waiting {
                self.finish_round_transition_recovery();
            } else {
                self.finish_recovery();
            }
        }
    }

    fn lifesteal(&mut self, event: usize, attacker: i32, victim: i32) -> Option<i32> {
        if self.controller.phase() == Phase::Idle || !self.controller.current_mode().is_some_and(|mode| mode.lifesteal) {
            return None;
        }
        let player = LivePlayer::resolve(attacker);
        if !lifesteal_eligible(attacker, victim, player.is_some()) {
            return None;
        }
        let player = player.expect("eligible attacker is alive");
        if !self.effects.get(&attacker).is_some_and(|effect| {
            Some(effect.identity) == self.players.identity(attacker)
                && effect.identity.pawn_handle == player.pawn_handle as u32
                && !effect.has_pending_loadout()
        }) || self.controllers.get(&attacker) != Some(&player.controller_handle)
        {
            return None;
        }
        let health = s2sdk::GetEventInt(event, &Str::from_str("dmg_health"));
        let armor = s2sdk::GetEventInt(event, &Str::from_str("dmg_armor"));
        let energy_shield = self.controller.current_mode().is_some_and(|mode| mode.energy_shield);
        let effect = self.effects.get_mut(&attacker)?;
        if let Err(error) = effect.lifesteal(player, health, armor, energy_shield) {
            self.handle_fault(error);
            return None;
        }
        if self.diagnostics() {
            log(&format!(
                "lifesteal slot={attacker} dmg_health={health} dmg_armor={armor} health={} armor={} helmet={:?}",
                s2sdk::GetClientHealth(attacker),
                s2sdk::GetClientArmor(attacker),
                player.helmet()
            ));
        }
        Some(attacker)
    }

    fn kill_loadout(&mut self, attacker: i32, victim: i32, event_weapon: &str, chicken_source: Option<crate::chicken_entities::KillWeaponSource>) {
        if attacker == victim || victim < 0 {
            return;
        }
        let Some(player) = LivePlayer::resolve(attacker) else { return };
        if self.controllers.get(&attacker) != Some(&player.controller_handle) {
            return;
        }
        if !self.controller.current_mode().is_some_and(|mode| mode.kill_loadout) {
            return;
        }
        let Some(effect) = self
            .effects
            .get(&attacker)
            .filter(|effect| Some(effect.identity) == self.players.identity(attacker))
        else {
            return;
        };
        if effect.identity.pawn_handle != player.pawn_handle as u32 {
            return;
        }
        if effect.round != self.controller.generation() {
            return;
        }
        let tracked = tracked_upgrade_weapon(effect, player).map(|target| target.handle);
        let chicken_fallback = chicken_source
            .filter(|source| source.attacker.slot == attacker && self.players.identity(attacker) == Some(source.attacker))
            .filter(|source| chicken_weapon_source_is_current(source, &self.players, &self.controllers))
            .filter(|source| crate::effects::equipment::kill_loadout_source_weapon(&source.classname))
            .map(|source| KillWeaponTarget {
                attacker: source.attacker,
                handle: source.handle,
                classname: source.classname,
                item_definition: source.item_definition,
            });
        let fallback = chicken_fallback.or_else(|| event_weapon_snapshot(player, effect.identity, event_weapon));
        if tracked.is_none() && fallback.is_none() {
            return;
        }
        let task = PlayerTask {
            player: effect.identity,
            round: effect.round,
        };
        self.engine_actions.push(EngineAction::Upgrade(task, player, fallback));
    }

    fn spawn(&mut self, slot: i32, tick: i32, apply_effects: bool) {
        self.bombs.abandon_quick_defuse_player(slot);
        if slot < 0 || tick < 0 || !s2sdk::IsClientInGame(slot) {
            return;
        }
        self.connect(slot);
        let controller = s2sdk::PlayerSlotToEntHandle(slot);
        if !s2sdk::IsValidEntHandle(controller) {
            return;
        }
        let previous = self.players.identity(slot).or_else(|| self.players.pending_identity(slot));
        let pawn = LivePlayer::resolve(slot).map_or(u32::MAX, |player| player.pawn_handle as u32);
        self.players.spawn(slot, tick as u64, pawn);
        let identity = self.players.identity(slot).or_else(|| self.players.pending_identity(slot));
        if previous != identity {
            if let Some(previous) = previous {
                self.clear_chicken_life(previous);
            }
            self.chicken_use_buttons.remove(&slot);
            self.healthshot_purchases.remove(&slot);
            self.engine_actions
                .retain(|action| !matches!(action, EngineAction::HealthshotPurchase { task, .. } if task.player.slot == slot));
            self.cancel_chicken_purchase(slot);
            self.round_respawns.retain(|(task, ..)| task.player.slot != slot);
            self.native_self_damage_shots.remove(&slot);
            self.suicides.entries.remove(&slot);
            self.suicides.buying.remove(&slot);
            self.suicides.pending.retain(|action| action.task().player.slot != slot);
            self.refills.cancel_player(slot);
            self.retire_effect(slot);
            self.pending.retain(|(task, _, _)| task.player.slot != slot);
            self.delayed_damage
                .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
        }
        if apply_effects
            && self.controller.phase() != Phase::Idle
            && let Some(identity) = identity
        {
            if let Some(mode) = self.controller.current_mode() {
                let task = PlayerTask {
                    player: identity,
                    round: self.controller.generation(),
                };
                if !self.combat.get(&slot).is_some_and(|(expected, _)| *expected == task) {
                    self.combat
                        .insert(slot, (task, funrounds_core::CombatLife::new(mode, s2sdk::GetGameTime(), &mut self.rng)));
                }
            }
            self.queue(identity, tick, PendingAction::Loadout);
        }
        if let Some(identity) = identity.filter(|_| {
            (tracks_chicken_life(self.controller.phase()) && self.controller.config().is_some_and(|config| config.chicken.is_some()))
                || self.controller.config().is_some_and(|config| config.healthshot.is_some())
        }) {
            let task = PlayerTask {
                player: identity,
                round: self.controller.generation(),
            };
            // Loadout writes happen on a later tick; the engine may still report the old HP.
            let base_health = self
                .combat
                .get(&slot)
                .filter(|(expected, _)| *expected == task)
                .map_or_else(|| s2sdk::GetClientHealth(slot), |(_, life)| i32::from(life.attributes().health));
            if base_health > 0 {
                self.chickens.observe_life(task, base_health);
            } else {
                self.chickens.clear_slot(slot);
            }
        } else {
            self.chickens.clear_slot(slot);
        }
    }

    fn connect(&mut self, slot: i32) {
        if slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) {
            return;
        }
        let controller = s2sdk::PlayerSlotToEntHandle(slot);
        if !s2sdk::IsValidEntHandle(controller) || self.controllers.get(&slot) == Some(&controller) {
            return;
        }
        self.forget_player(slot);
        self.controllers.insert(slot, controller);
        self.players.connect(slot);
        if self.classic_enabled()
            && !s2sdk::IsFakeClient(slot)
            && let Some(identity) = self.players.connection_identity(slot)
        {
            self.welcomes.push((identity, s2sdk::GetGameTime() + 15.0));
        }
    }

    fn queue_bot_takeover(&mut self, slot: i32) {
        if slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) {
            return;
        }
        self.connect(slot);
        let controller_handle = s2sdk::PlayerSlotToEntHandle(slot);
        if !s2sdk::IsValidEntHandle(controller_handle) || self.controllers.get(&slot) != Some(&controller_handle) {
            return;
        }
        let Some(expected) = self.players.connection_identity(slot) else {
            return;
        };
        let event_tick = s2sdk::GetGameTickCount();
        if event_tick < 0 {
            return;
        }
        self.pending_takeovers.retain(|pending| pending.slot != slot);
        self.pending_takeovers.push(PendingTakeover {
            slot,
            controller_handle,
            expected,
            round: self.controller.generation(),
            event_tick,
        });
    }

    fn apply_pending_takeovers(&mut self, tick: i32) {
        let pending = std::mem::take(&mut self.pending_takeovers);
        for takeover in pending {
            if takeover.round != self.controller.generation()
                || self.controllers.get(&takeover.slot) != Some(&takeover.controller_handle)
                || s2sdk::PlayerSlotToEntHandle(takeover.slot) != takeover.controller_handle
                || self.players.connection_identity(takeover.slot) != Some(takeover.expected)
            {
                continue;
            }
            if tick <= takeover.event_tick {
                self.pending_takeovers.push(takeover);
                continue;
            }
            let player = LivePlayer::resolve(takeover.slot).filter(|player| {
                player.controller_handle == takeover.controller_handle && player.pawn_controller() == Some(takeover.controller_handle)
            });
            let Some(player) = player else {
                self.pending_takeovers.push(takeover);
                continue;
            };
            let Some(identity) = self.players.takeover_identity(takeover.expected, player.pawn_handle as u32) else {
                continue;
            };
            let task = PlayerTask {
                player: identity,
                round: takeover.round,
            };
            let mut owners = self.tracked_pawn_owners(player.pawn_handle as u32);
            owners.sort_unstable();
            owners.dedup();
            let effect_sources = owners
                .iter()
                .copied()
                .filter(|slot| {
                    self.effects.get(slot).is_some_and(|effect| {
                        effect.round == takeover.round
                            && effect.identity.pawn_handle == player.pawn_handle as u32
                            && self.players.identity(effect.identity.slot) == Some(effect.identity)
                    })
                })
                .collect::<Vec<_>>();
            let combat_sources = owners
                .iter()
                .copied()
                .filter(|slot| {
                    self.combat.get(slot).is_some_and(|(task, _)| {
                        task.round == takeover.round
                            && task.player.pawn_handle == player.pawn_handle as u32
                            && self.players.identity(task.player.slot) == Some(task.player)
                    })
                })
                .collect::<Vec<_>>();
            let suicide_sources = owners
                .iter()
                .copied()
                .filter(|slot| {
                    let entry_owns_pawn = self.suicides.entries.get(slot).is_some_and(|entry| {
                        entry.task.round == takeover.round
                            && entry.task.player.pawn_handle == player.pawn_handle as u32
                            && entry.player.pawn_handle as u32 == player.pawn_handle as u32
                            && self.players.identity(entry.task.player.slot) == Some(entry.task.player)
                    });
                    let pending_finish_owns_pawn = self.suicides.pending.iter().any(|action| {
                        matches!(
                            action,
                            crate::suicide::Action::Finish {
                                task: queued,
                                player: queued_player,
                                after_round_end: true,
                                ..
                            } if queued.player.slot == *slot
                                && queued.player.pawn_handle == player.pawn_handle as u32
                                && queued_player.slot == *slot
                                && queued_player.pawn_handle == player.pawn_handle
                                && self.players.identity(queued.player.slot) == Some(queued.player)
                        )
                    });
                    entry_owns_pawn || pending_finish_owns_pawn
                })
                .collect::<Vec<_>>();
            let previous_target_pawn = self.players.identity(takeover.slot).map(|current| current.pawn_handle);
            let mut failures = Vec::new();
            let mut retired = Vec::new();
            let suicide_source = match self.suicides.takeover(&suicide_sources, player, task) {
                Ok(source) => source,
                Err(error) => {
                    failures.push(Fault::global(format!("suicide ownership transfer failed: {error}")));
                    None
                }
            };
            self.cancel_takeover_work(&owners, previous_target_pawn, task, player, suicide_source);
            if self.effects.get(&takeover.slot).is_some_and(|effect| {
                effect.identity.pawn_handle != player.pawn_handle as u32 || self.players.identity(effect.identity.slot) != Some(effect.identity)
            }) && let Some(mut effect) = self.effects.remove(&takeover.slot)
            {
                if let Err(error) = effect.restore_retired_state(&mut self.refills) {
                    failures.push(Fault::global(format!("effect restoration deferred for slot={}: {error}", takeover.slot)));
                    retired.push((takeover.slot, effect));
                } else if !effect.recovery_complete() {
                    retired.push((takeover.slot, effect));
                }
            }

            let effect_source = effect_sources
                .iter()
                .copied()
                .find(|slot| *slot == takeover.slot)
                .or_else(|| effect_sources.first().copied());
            if let Some(source) = effect_source
                && let Some(mut effect) = self.effects.remove(&source)
            {
                if let Some(mode) = self.controller.current_mode() {
                    if let Err(error) = effect.takeover(player, identity, takeover.round, mode) {
                        failures.push(if error.scope == Scope::Cancelled {
                            Fault::global(format!("effect ownership transfer failed: {error}"))
                        } else {
                            error
                        });
                    }
                } else {
                    failures.push(Fault::global("active pawn effect has no current mode during bot takeover"));
                }
                self.effects.insert(takeover.slot, effect);
            }
            for source in effect_sources.into_iter().filter(|source| Some(*source) != effect_source) {
                failures.push(Fault::global(format!(
                    "multiple active effects own pawn={} during bot_takeover; slot={} was retired",
                    player.pawn_handle, source
                )));
                if let Some(effect) = self.effects.remove(&source) {
                    retired.push((source, effect));
                }
            }

            let combat_source = combat_sources
                .iter()
                .copied()
                .find(|slot| *slot == takeover.slot)
                .or_else(|| combat_sources.first().copied());
            let source_combat = combat_source.and_then(|source| self.combat.remove(&source));
            for source in combat_sources {
                if Some(source) != combat_source {
                    self.combat.remove(&source);
                }
            }
            if combat_source != Some(takeover.slot) {
                self.combat.remove(&takeover.slot);
            }
            if let Some((old_task, life)) = source_combat
                && old_task.round == takeover.round
                && old_task.player.pawn_handle == identity.pawn_handle
                && combat_source.is_some_and(|source| self.players.identity(source) == Some(old_task.player))
            {
                self.combat.insert(takeover.slot, (task, life));
            }

            if self.players.takeover(takeover.expected, player.pawn_handle as u32) != Some(identity) {
                failures.push(Fault::global("player identity changed while applying bot_takeover"));
            } else {
                self.chickens.takeover_pawn(task);
                self.afterburns.takeover_pawn(task, player);
            }
            for (slot, mut effect) in retired {
                match effect.restore_retired_state(&mut self.refills) {
                    Ok(()) if !effect.recovery_complete() => self.retired_effects.push(effect),
                    Ok(()) => {}
                    Err(error) => {
                        self.retired_effects.push(effect);
                        failures.push(Fault::global(format!("effect restoration deferred for slot={slot}: {error}")));
                    }
                }
            }
            for error in failures {
                self.handle_fault(error);
            }
        }
    }

    fn tracked_pawn_owners(&self, pawn_handle: u32) -> Vec<i32> {
        self.players.pawn_owners(pawn_handle)
    }

    fn cancel_takeover_work(
        &mut self,
        owners: &[i32],
        previous_target_pawn: Option<u32>,
        takeover_task: PlayerTask,
        takeover_player: LivePlayer,
        suicide_source: Option<i32>,
    ) {
        let target = takeover_task.player.slot;
        let pawn = takeover_task.player.pawn_handle;
        let mut slots = owners.to_vec();
        slots.push(target);
        slots.sort_unstable();
        slots.dedup();
        self.native_self_damage_shots.retain(|slot, _| !slots.contains(slot));
        let mut pending = std::mem::take(&mut self.pending);
        pending.retain(|(task, _, action)| {
            if slots.contains(&task.player.slot) {
                self.cancel_pending(*action);
                false
            } else {
                true
            }
        });
        self.pending = pending;
        self.reflections
            .retain(|hit| !slots.contains(&hit.target.player.slot) && !slots.contains(&hit.source.player.slot));
        self.delayed_damage
            .retain(|hit| !slots.contains(&hit.target.player.slot) && !slots.contains(&hit.source.player.slot));
        self.random_heals
            .retain(|receipt| !slots.contains(&receipt.target.player.slot) && !slots.contains(&receipt.source.player.slot));
        self.impacts
            .retain(|impact| !slots.contains(&impact.task.player.slot) && !slots.contains(&impact.victim_task.player.slot));
        self.low_health_finishes.retain(|task| !slots.contains(&task.player.slot));
        self.respawns.retain(|(task, ..)| !slots.contains(&task.player.slot));
        self.round_respawns.retain(|(task, ..)| !slots.contains(&task.player.slot));
        self.chicken_purchases.retain(|slot, _| !slots.contains(slot));
        self.engine_actions.retain_mut(|action| match action {
            EngineAction::Rollback(task, _)
            | EngineAction::Loadout(task, _, _)
            | EngineAction::SettleLoadout(task, _, _)
            | EngineAction::Upgrade(task, _, _)
            | EngineAction::ThirdPerson(task, _)
            | EngineAction::GrantC4(task, _)
            | EngineAction::RemoveC4(task, _, _)
            | EngineAction::Respawn(task, _, _)
            | EngineAction::RoundRespawn(task, _, _)
            | EngineAction::LowHealthFinish(task, _, _)
            | EngineAction::HealthshotUse { task, .. }
            | EngineAction::HealthshotPurchase { task, .. }
            | EngineAction::AfterburnIgnite { task, .. }
            | EngineAction::ChickenEat { task, .. } => !slots.contains(&task.player.slot),
            EngineAction::ChickenPurchase { task, .. } => !slots.contains(&task.player.slot),
            EngineAction::Suicide(suicide) => {
                let old = suicide.task();
                if !slots.contains(&old.player.slot) {
                    return true;
                }
                if matches!(suicide, crate::suicide::Action::Purchase { .. })
                    || suicide_source != Some(old.player.slot)
                    || old.round != takeover_task.round
                    || old.player.pawn_handle != pawn
                {
                    return false;
                }
                match suicide {
                    crate::suicide::Action::Start { task: queued, owned } => {
                        *queued = takeover_task;
                        owned.player = takeover_player;
                        true
                    }
                    crate::suicide::Action::Blast {
                        task: queued,
                        player: queued_player,
                    }
                    | crate::suicide::Action::Finish {
                        task: queued,
                        player: queued_player,
                        ..
                    } => {
                        *queued = takeover_task;
                        *queued_player = takeover_player;
                        true
                    }
                    crate::suicide::Action::Purchase { .. } => false,
                }
            }
            EngineAction::Reflection(hit) => !slots.contains(&hit.target.player.slot) && !slots.contains(&hit.source.player.slot),
            EngineAction::DelayedDamage(hit) => !slots.contains(&hit.target.player.slot) && !slots.contains(&hit.source.player.slot),
            EngineAction::ProjectileImpact(impact) => !slots.contains(&impact.task.player.slot) && !slots.contains(&impact.victim_task.player.slot),
            EngineAction::Projectile(action) => match action.kind {
                crate::projectiles::ActionKind::SpawnKnife { task, .. } => !slots.contains(&task.player.slot),
                crate::projectiles::ActionKind::Remove { .. } => true,
            },
            EngineAction::HostageRescue(action) => !slots.contains(&action.task.player.slot),
            EngineAction::Equipment(equipment::Action::Refill(refill)) => !slots.contains(&refill.task.player.slot),
            EngineAction::Equipment(equipment::Action::Remove { .. })
            | EngineAction::Bomb(_)
            | EngineAction::ChickenSpawn { .. }
            | EngineAction::ChickenBlast { .. }
            | EngineAction::ChickenRemove(_)
            | EngineAction::SuicideFeedbackRemove(_)
            | EngineAction::AfterburnRemove(_)
            | EngineAction::EndRound(_, _)
            | EngineAction::CenterDisplay { .. } => true,
        });
        for slot in slots {
            self.refills.cancel_player(slot);
            let owned_pawn = if slot == target {
                previous_target_pawn.or(Some(pawn))
            } else {
                Some(pawn)
            };
            self.projectiles.retire_player(slot, owned_pawn);
        }
    }

    fn queue(&mut self, player: PlayerIdentity, tick: i32, action: PendingAction) {
        let task = PlayerTask {
            player,
            round: self.controller.generation(),
        };
        if !self.pending.iter().any(|(pending, _, queued)| *pending == task && *queued == action) {
            self.pending.push((task, tick, action));
        }
    }

    fn queue_respawn(&mut self, slot: i32, delay: f32) {
        if self.controller.phase() == Phase::Idle || LivePlayer::resolve(slot).is_some() {
            return;
        }
        self.connect(slot);
        let Some(player) = LivePlayer::resolve_present(slot) else { return };
        let team = s2sdk::GetClientTeam(slot) as i32;
        if !matches!(team, 2 | 3) {
            return;
        }
        let Some(task) = self.players.respawn_task(slot, self.controller.generation()) else {
            return;
        };
        if !self.respawns.iter().any(|(pending, ..)| *pending == task) {
            self.respawns.push((task, player, team, s2sdk::GetGameTime() + delay));
        }
    }

    fn queue_round_respawn(&mut self, slot: i32, tick: i32, delay: f32) {
        if self.controller.phase() == Phase::Idle || !self.controller.current_mode().is_some_and(|mode| mode.round_respawn == RoundRespawn::All) {
            return;
        }
        let team = s2sdk::GetClientTeam(slot) as i32;
        if !matches!(team, 2 | 3) {
            return;
        }
        self.connect(slot);
        let Some(player) = LivePlayer::resolve_present(slot) else { return };
        if LivePlayer::resolve(slot).is_some()
            && self
                .players
                .identity(slot)
                .is_none_or(|identity| identity.pawn_handle != player.pawn_handle as u32)
        {
            // Register an alive player's current life without applying effects; its real spawn follows ForceRespawnClient.
            self.spawn(slot, tick, false);
        }
        let Some(task) = round_respawn_task(&self.players, self.controller.generation(), player) else {
            return;
        };
        if self.controllers.get(&slot) != Some(&player.controller_handle) {
            return;
        }
        let now = s2sdk::GetGameTime();
        if !now.is_finite() || !delay.is_finite() || delay < 0.0 {
            return;
        }
        if !self.round_respawns.iter().any(|(pending, ..)| *pending == task) {
            self.round_respawns.push((task, player, team, now + delay));
        }
    }

    fn queue_c4_assignment(&mut self) {
        if !active_combat_phase(self.controller.phase())
            || self.c4_assigned
            || self.equipment_in_flight > 0
            || self.pending.iter().any(|(_, _, action)| *action == PendingAction::Loadout)
            || !self.settling_loadouts.is_empty()
            || self.effects.values().any(Effect::has_pending_loadout)
        {
            return;
        }
        let Some(count) = self.controller.current_mode().and_then(|mode| mode.c4_count) else {
            return;
        };
        let count = usize::from(count);
        let phase = self.controller.phase();
        let mut players = Vec::new();
        let mut candidates = Vec::new();
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let Some(task) = self.combat_task(player) else { continue };
            let native = s2sdk::GetClientWeapons(slot)
                .iter()
                .copied()
                .filter(|handle| crate::suicide::real_c4(player, *handle) && !self.bombs.excludes_suicide_c4(player, *handle))
                .collect();
            players.push((task, player, native));
            if s2sdk::GetClientTeam(slot) as i32 == 2 {
                candidates.push((task, player));
            }
        }
        if phase == Phase::Warmup && count > 0 && candidates.is_empty() {
            return;
        }
        self.c4_assigned = true;
        self.engine_actions.extend(
            players
                .into_iter()
                .map(|(task, player, native)| EngineAction::RemoveC4(task, player, native)),
        );
        candidates.shuffle(&mut self.rng);
        self.engine_actions.extend(
            candidates
                .into_iter()
                .take(count)
                .map(|(task, player)| EngineAction::GrantC4(task, player)),
        );
    }

    fn forget_player(&mut self, slot: i32) {
        self.clear_pending_chicken_blast_for_slot(slot);
        if let Some(identity) = self.players.identity(slot) {
            self.afterburns.clear_life(PlayerTask {
                round: self.controller.generation(),
                player: identity,
            });
            self.afterburn_cleanup.extend(self.afterburns.take_removals());
        }
        self.cancel_chicken_purchase(slot);
        self.healthshot_purchases.remove(&slot);
        self.engine_actions
            .retain(|action| !matches!(action, EngineAction::HealthshotPurchase { task, .. } if task.player.slot == slot));
        self.chickens.clear_slot(slot);
        self.chicken_use_buttons.remove(&slot);
        self.native_self_damage_shots.remove(&slot);
        self.pending_takeovers.retain(|pending| pending.slot != slot);
        if let Err(error) = self.suicides.forget(slot) {
            self.record_fault(Fault::global(error));
        }
        self.combat.remove(&slot);
        self.delayed_damage
            .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
        self.random_heals
            .retain(|receipt| receipt.target.player.slot != slot && receipt.source.player.slot != slot);
        self.reflections
            .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
        self.refills.cancel_player(slot);
        self.projectiles
            .retire_player(slot, self.players.identity(slot).map(|player| player.pawn_handle));
        self.retire_effect(slot);
        self.players.disconnect(slot);
        self.controllers.remove(&slot);
        if let Some(preferences) = self.preferences.as_mut() {
            preferences.forget(slot);
        }
        self.welcomes.retain(|(identity, _)| identity.slot != slot);
        self.pending.retain(|(task, _, _)| task.player.slot != slot);
        self.respawns.retain(|(task, _, _, _)| task.player.slot != slot);
        self.round_respawns.retain(|(task, ..)| task.player.slot != slot);
    }

    fn retire_effect(&mut self, slot: i32) {
        if let Err(error) = self.night_vision.remove_player(slot) {
            self.record_fault(Fault::global(error));
        }
        if let Err(error) = self.ghost.remove_player(slot) {
            self.handle_fault(error);
        }
        let has_pending_loadout = self.effects.get(&slot).is_some_and(Effect::has_pending_loadout);
        if has_pending_loadout && self.controller.current_mode().is_some_and(|mode| mode.c4_count.is_some()) {
            self.c4_assigned = false;
            self.engine_actions
                .retain(|action| !matches!(action, EngineAction::GrantC4(task, _) if task.player.slot == slot));
        }
        let Some(effect) = self.effects.remove(&slot) else { return };
        let index = self.retired_effects.len();
        self.retired_effects.push(effect);
        if let Err(error) = restore_or_retain(&mut self.retired_effects, index, |effect| {
            effect.restore_retired_state(&mut self.refills)?;
            Ok(effect.recovery_complete())
        }) {
            self.handle_fault(Fault::global(format!("effect restoration deferred for slot={slot}: {error}")));
        }
    }

    fn retry_third_person_map_detach(&mut self) -> Result<(), String> {
        if self.third_person_map_detach_pending {
            crate::effects::discard_third_person_map_targets()?;
            self.third_person_map_detach_pending = false;
        }
        Ok(())
    }

    fn cleanup_effects(&mut self, round_end: bool) {
        self.round_transition_waiting = false;
        self.cleanup_effects_with_round_lifetime(round_end, false);
    }

    fn cleanup_after_round_start_wait(&mut self) -> bool {
        let waiting_for_selection = self.round_transition_waiting
            && self.faults.recovery == Recovery::Ready
            && self.controller.enabled()
            && self.controller.phase() == Phase::Idle
            && self.controller.current_mode().is_none();
        if waiting_for_selection {
            self.cleanup_round_effects(false);
        } else {
            self.cleanup_effects(false);
        }
        waiting_for_selection
    }

    fn cleanup_round_effects(&mut self, round_end: bool) {
        self.round_transition_waiting = true;
        self.cleanup_effects_with_round_lifetime(round_end, true);
    }

    fn cleanup_effects_with_round_lifetime(&mut self, round_end: bool, round_transition: bool) {
        self.settling_loadouts.clear();
        if let Err(error) = self.retry_third_person_map_detach() {
            self.record_fault(Fault::global(error));
        }
        self.weapon_inventory.clear();
        if let Err(error) = self.night_vision.cleanup_all() {
            self.record_fault(Fault::global(error));
        }
        if round_transition {
            self.clear_chicken_round_work();
            self.advance_chicken_round(self.controller.generation());
            self.suicide_feedback_cleanup
                .extend(self.suicide_feedback.take_expired(s2sdk::GetGameTime()));
        } else {
            self.cleanup_ordinary_chickens();
        }
        self.healthshot_purchases.clear();
        self.afterburns.clear_all();
        self.afterburn_cleanup.extend(self.afterburns.take_removals());
        if let Err(error) = self.ground_reverse.apply(None) {
            self.record_fault(error);
        }
        if let Err(error) = self.ghost.clear() {
            self.record_fault(error);
        }
        self.pending_takeovers.clear();
        let suicide_cleanup = if round_end {
            self.suicides.cleanup_after_round_end()
        } else {
            self.suicides.cleanup()
        };
        if let Err(error) = suicide_cleanup {
            self.record_fault(Fault::global(error));
        }
        if !round_transition {
            self.suicide_feedback_cleanup.extend(self.suicide_feedback.take_all());
        }
        self.native_self_damage_shots.clear();
        self.combat.clear();
        self.reflections.clear();
        self.delayed_damage.clear();
        self.random_heals.clear();
        self.center_display = None;
        self.refills.cancel_all();
        self.impacts.clear();
        self.chicken_impacts.clear();
        self.projectiles.clear();
        self.pending.clear();
        self.team_match = None;
        self.periodic_movement = None;
        self.money_income = None;
        self.c4_assigned = false;
        self.engine_actions.clear();
        self.respawns.clear();
        self.round_respawns.clear();
        self.low_health_finishes.clear();
        let bomb_cleanup = if round_end {
            self.bombs.cleanup_after_round_end(self.controller.generation())
        } else {
            self.bombs.cleanup()
        };
        self.restore_completed_native_bomb_scales();
        if let Err(error) = bomb_cleanup {
            self.record_fault(Fault::global(error));
        }
        let slots = self.effects.keys().copied().collect::<Vec<_>>();
        for slot in slots {
            let effect = self.effects.get_mut(&slot).expect("effect slot exists");
            let result = if self.players.identity(effect.identity.slot) == Some(effect.identity) {
                effect.restore(&mut self.refills)
            } else {
                effect.restore_retired_state(&mut self.refills)
            };
            match result {
                Ok(()) if effect.recovery_complete() => {
                    self.effects.remove(&slot);
                }
                Ok(()) => {}
                Err(error) => self.record_fault(Fault::global(error)),
            }
        }
        for index in (0..self.retired_effects.len()).rev() {
            if let Err(error) = restore_or_retain(&mut self.retired_effects, index, |effect| {
                effect.restore_retired_state(&mut self.refills)?;
                Ok(effect.recovery_complete())
            }) {
                self.record_fault(Fault::global(error));
            }
        }
        if let Err(error) = self.parameters.restore() {
            self.record_fault(Fault::global(error));
        }
    }
    pub fn shutdown(&mut self) -> Result<(), String> {
        // Native hook cleanup runs before Runtime shutdown, so a configured healthshot override is no longer wanted.
        self.healthshot_hooks_ready = false;
        self.ground_reverse.retry_cleanup();
        self.restore_duck_speed();
        self.restore_model_scales();
        self.welcomes.clear();
        self.bombs.acknowledge_shutdown_removals();
        self.cleanup_effects(false);
        self.restore_team_names();
        if let Err(error) = self.healthshot_parameters.set_healthshot_override(false) {
            self.record_fault(error);
        }
        self.healthshot_override_active = false;
        if self.cleanup_complete() && self.duck_speed.is_empty() && self.model_scales.is_empty() {
            Ok(())
        } else {
            Err("owned properties or ConVars remain unrestored; snapshots retained for shutdown retry".into())
        }
    }

    pub fn track_suicide_feedback(&mut self, handle: i32, now: f32) -> bool {
        (now + crate::suicide::FEEDBACK_LIFETIME).is_finite() && self.suicide_feedback.track(handle, now + crate::suicide::FEEDBACK_LIFETIME)
    }

    pub fn take_suicide_feedback_cleanup(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.suicide_feedback_cleanup)
    }

    fn restore_team_names(&mut self) {
        if let Err(error) = self.team_names.restore() {
            self.record_fault(Fault::global(error));
        }
    }

    fn suicide_enabled(&self) -> bool {
        self.classic_enabled() && active_combat_phase(self.controller.phase()) && self.controller.current_mode().is_some_and(|mode| mode.suicide)
    }

    fn c4_purchase_enabled(&self, start: bool) -> bool {
        self.classic_enabled()
            && active_combat_phase(self.controller.phase())
            && self
                .controller
                .current_mode()
                .is_some_and(|mode| if start { mode.suicide } else { mode.c4_buy })
    }

    fn qualifying_c4(&self, player: LivePlayer) -> Option<i32> {
        s2sdk::GetClientWeapons(player.slot)
            .iter()
            .copied()
            .find(|weapon| crate::suicide::real_c4(player, *weapon) && !self.bombs.excludes_suicide_c4(player, *weapon))
    }

    fn request_chicken(&mut self, slot: i32) -> Result<String, String> {
        if !self.chicken_ready || !self.classic_enabled() || !active_combat_phase(self.controller.phase()) || !self.ordinary_chickens_enabled() {
            return Err(self.text(slot, "chicken.unavailable", &[]));
        }
        let player = LivePlayer::resolve(slot).ok_or(self.text(slot, "player.alive_required", &[]))?;
        if !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
            return Err(self.text(slot, "chicken.team_required", &[]));
        }
        let task = self.combat_task(player).ok_or(self.text(slot, "player.life_pending", &[]))?;
        if self.chicken_purchases.contains_key(&slot) {
            return Err(self.text(slot, "chicken.pending", &[]));
        }
        let config = self.controller.config().ok_or(self.text(slot, "config.unavailable", &[]))?;
        let price = config.prices.chicken_cash;
        let player_origin = s2sdk::GetEntityAbsOrigin(player.pawn_handle);
        if !player_origin.x.is_finite() || !player_origin.y.is_finite() || !player_origin.z.is_finite() {
            return Err(self.text(slot, "chicken.position_invalid", &[]));
        }
        self.chicken_purchases.insert(slot, task);
        self.engine_actions.push(EngineAction::ChickenPurchase {
            task,
            player,
            price,
            origin: plugify::Vec3 {
                x: player_origin.x + 64.0,
                y: player_origin.y,
                z: player_origin.z + 4.0,
            },
        });
        Ok(self.text(slot, "chicken.request", &[("price", &price.to_string())]))
    }

    fn request_suicide(&mut self, slot: i32, start: bool) -> Result<String, String> {
        if !self.c4_purchase_enabled(start) {
            return Err(self.text(slot, "c4.unavailable", &[]));
        }
        let player = LivePlayer::resolve(slot).ok_or(self.text(slot, "player.alive_required", &[]))?;
        let task = self.combat_task(player).ok_or(self.text(slot, "player.life_pending", &[]))?;
        if self.suicides.buying.get(&slot) == Some(&task) {
            return Err(self.text(slot, "c4.pending", &[]));
        }
        if self.suicides.active(player) {
            return Err(self.text(slot, "suicide.started", &[]));
        }
        if let Some(c4) = self.qualifying_c4(player) {
            if self.suicide_enabled() {
                self.suicides.qualify(task, player, c4);
            }
            if start {
                self.suicides.begin(slot, s2sdk::GetGameTickCount(), s2sdk::GetGameTime())?;
            }
            return Ok(if self.suicide_enabled() {
                self.text(slot, "c4.owned_suicide", &[])
            } else {
                self.text(slot, "c4.owned", &[])
            });
        }
        if self.suicides.pending.iter().any(|action| action.task() == task) {
            return Err(self.text(slot, "c4.pending", &[]));
        }
        if s2sdk::GetClientTeam(slot) as i32 != 2 {
            return Err(self.text(slot, "c4.t_required", &[]));
        }
        let price = self.controller.config().map_or(10_000, |config| config.prices.c4_cash);
        self.suicides.buying.insert(slot, task);
        self.suicides
            .pending
            .push(crate::suicide::Action::Purchase { task, player, start, price });
        Ok(self.text(slot, "c4.request", &[("price", &price.to_string())]))
    }

    fn update_suicides(&mut self, tick: i32) -> Vec<EngineAction> {
        if !self.suicide_enabled() {
            let mut due = Vec::new();
            for action in std::mem::take(&mut self.suicides.pending) {
                match action {
                    crate::suicide::Action::Purchase { .. } => due.push(EngineAction::Suicide(action)),
                    crate::suicide::Action::Finish {
                        after_round_end: true,
                        tick: queued,
                        ..
                    } if self.action_valid(&EngineAction::Suicide(action)) => {
                        if round_end_finish_due(tick, queued) {
                            due.push(EngineAction::Suicide(action));
                        } else {
                            self.suicides.pending.push(action);
                        }
                    }
                    _ => {}
                }
            }
            return due;
        }
        let now = s2sdk::GetGameTime();
        let mut failures = Vec::new();
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let Some(task) = self.combat_task(player) else { continue };
            if let Some(entry) = self.suicides.entries.get(&slot)
                && entry.task != task
            {
                // A new life may reuse the same pawn; its fresh properties are not ours.
                self.suicides.entries.remove(&slot);
            }
            if let Some(c4) = self.qualifying_c4(player) {
                self.suicides.qualify(task, player, c4);
            }
            let Some(entry) = self.suicides.entries.get_mut(&slot) else { continue };
            if let Some(sequence) = entry.sequence.as_mut() {
                if sequence.take_due(tick, now) {
                    self.suicides.pending.push(crate::suicide::Action::Blast { task, player });
                }
            } else if !crate::suicide::real_c4(player, entry.c4) {
                self.suicides.entries.remove(&slot);
            } else {
                let look = crate::funrounds_native::GetLookAtWeaponButtonState(player.pawn_handle);
                let edge = look == 1 && !entry.look_down;
                entry.look_down = look == 1;
                if edge
                    && s2sdk::GetClientActiveWeapon(slot) == entry.c4
                    && let Err(error) = self.suicides.begin(slot, tick, now)
                {
                    failures.push(error);
                }
            }
        }
        if let Some(error) = failures.pop() {
            self.handle_fault(Fault::mechanism(Mechanism::Suicide, error));
        }
        let pending = std::mem::take(&mut self.suicides.pending);
        let mut due = Vec::new();
        for action in pending {
            if matches!(action, crate::suicide::Action::Finish { tick: queued, .. } if tick <= queued) {
                self.suicides.pending.push(action);
            } else {
                due.push(EngineAction::Suicide(action));
            }
        }
        due
    }

    pub fn blocks_suicide_pickup(&self, pawn: i32, controller: i32) -> bool {
        self.suicide_enabled()
            && self.suicides.entries.values().any(|entry| {
                entry.player.pawn_handle == pawn
                    && entry.player.controller_handle == controller
                    && entry.sequence.is_some()
                    && self.combat_task(entry.player) == Some(entry.task)
            })
    }

    pub fn existing_suicide_c4(&self, player: LivePlayer) -> Option<i32> {
        self.qualifying_c4(player)
    }

    pub fn suicide_buyer_current(&self, action: crate::suicide::Action) -> bool {
        self.on_game_thread()
            && self.players.validates_connection(action.task().player)
            && self.controllers.get(&action.player().slot) == Some(&action.player().controller_handle)
    }

    pub fn finish_suicide_purchase(&mut self, action: crate::suicide::Action, result: Result<i32, crate::cash::PurchaseError>) {
        let crate::suicide::Action::Purchase { task, player, start, .. } = action else {
            return;
        };
        if self.suicides.buying.get(&player.slot) == Some(&task) {
            self.suicides.buying.remove(&player.slot);
        }
        if !self.action_valid(&EngineAction::Suicide(action)) {
            return;
        }
        let message = match result {
            Ok(c4) => {
                if self.suicide_enabled() {
                    self.suicides.qualify(task, player, c4);
                }
                if start && let Err(error) = self.suicides.begin(player.slot, s2sdk::GetGameTickCount(), s2sdk::GetGameTime()) {
                    self.handle_fault(Fault::mechanism(Mechanism::Suicide, error));
                }
                crate::i18n::Message::new(if start {
                    "c4.purchased_start"
                } else if self.suicide_enabled() {
                    "c4.purchased_inspect"
                } else {
                    "c4.purchased"
                })
            }
            Err(error) => self.purchase_error(player.slot, error),
        };
        crate::i18n::send_chat(player.slot, &self.message(player.slot, message));
    }

    pub fn prepare_suicide_blast(&mut self, action: crate::suicide::Action) -> Result<Vec<(PlayerTask, LivePlayer, f32)>, Fault> {
        let source = action.player();
        let origin = s2sdk::GetEntityAbsOrigin(source.pawn_handle);
        let tick = s2sdk::GetGameTickCount();
        let self_amount = s2sdk::GetClientHealth(source.slot) as f32 * 10.0;
        self.suicides.pending.push(crate::suicide::Action::Finish {
            task: action.task(),
            player: source,
            amount: self_amount,
            tick,
            after_round_end: false,
        });
        let mut targets = Vec::new();
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot).filter(|player| *player != source) else {
                continue;
            };
            let Some(task) = self.combat_task(player) else { continue };
            let position = s2sdk::GetEntityAbsOrigin(player.pawn_handle);
            let distance = ((origin.x - position.x).powi(2) + (origin.y - position.y).powi(2) + (origin.z - position.z).powi(2)).sqrt();
            let amount = funrounds_core::suicide_blast_damage(s2sdk::GetClientHealth(slot), distance);
            if amount <= 0.0 {
                continue;
            }
            if let Some(sequence) = self.suicides.entries.get_mut(&slot).and_then(|entry| entry.sequence.as_mut()) {
                sequence.chain_next_tick(tick);
                continue;
            }
            targets.push((task, player, amount));
        }
        for name in [
            "mp_friendlyfire",
            "ff_damage_reduction_bullets",
            "ff_damage_reduction_grenade",
            "ff_damage_reduction_other",
        ] {
            self.suicides.ff.set_number(name, 1.0)?;
        }
        Ok(targets)
    }

    pub fn prepare_suicide_finish(&self, action: crate::suicide::Action) -> Option<f32> {
        let crate::suicide::Action::Finish { player, amount, .. } = action else {
            return None;
        };
        if !self.action_valid(&EngineAction::Suicide(action)) {
            return None;
        }
        lethal_suicide_finish_amount(amount, s2sdk::GetClientHealth(player.slot))
    }

    pub fn suicide_target_valid(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.suicide_enabled() && self.combat_task(player) == Some(task) && LivePlayer::resolve(player.slot) == Some(player)
    }

    fn suicide_hurt(&mut self, event: usize, victim: i32, attacker: i32) {
        if !crate::damage::internal_suicide_hurt(victim, attacker) {
            return;
        }
        let Some(player) = LivePlayer::resolve_present(victim) else { return };
        let Some(task) = self.combat_task(player) else { return };
        if let Some((expected, life)) = self.combat.get_mut(&victim).filter(|(expected, _)| *expected == task) {
            let _ = expected;
            life.lose_native_armor(s2sdk::GetEventInt(event, &Str::from_str("dmg_armor")));
        }
    }

    pub fn finish_suicide_blast(&mut self) {
        if let Err(error) = self.suicides.ff.restore() {
            self.handle_fault(Fault::global(error));
        }
    }

    pub(crate) fn text(&self, slot: i32, key: &'static str, args: &[(&str, &str)]) -> String {
        self.texts
            .render(&crate::i18n::language(slot, &self.texts), key, args)
            .expect("validated message template and arguments")
    }

    pub(crate) fn message(&self, slot: i32, message: crate::i18n::Message) -> String {
        message.render(&self.texts, &crate::i18n::language(slot, &self.texts))
    }

    pub(crate) fn say(&self, slot: i32, key: &'static str, args: &[(&str, &str)]) {
        crate::i18n::send_chat(slot, &self.text(slot, key, args));
    }

    pub(crate) fn broadcast(&self, key: &'static str, args: &[(&str, &str)]) {
        for slot in 0..s2sdk::GetMaxClients() {
            if s2sdk::IsClientInGame(slot) {
                self.say(slot, key, args);
            }
        }
    }

    fn team_match_text(&self, slot: i32) -> String {
        self.team_match.as_ref().map_or_else(
            || self.text(slot, "team.waiting", &[]),
            |game| {
                self.text(
                    slot,
                    "team.info",
                    &[
                        ("t", &game.scores()[0].to_string()),
                        ("ct", &game.scores()[1].to_string()),
                        ("seconds", &format!("{:.0}", game.remaining(s2sdk::GetGameTime()))),
                    ],
                )
            },
        )
    }

    pub(crate) fn purchase_error(&self, slot: i32, error: crate::cash::PurchaseError) -> crate::i18n::Message {
        use crate::{
            cash::{PurchaseError, RefundStatus},
            i18n::Message,
        };
        match error {
            PurchaseError::Insufficient { item, price } => Message::new("purchase.insufficient")
                .arg("item", self.text(slot, item.key(), &[]))
                .arg("price", price),
            PurchaseError::Debit(error) => Message::new("purchase.debit").arg("error", error),
            PurchaseError::Rejected(key) => Message::new(key),
            PurchaseError::Delivery { detail, refund } => match refund {
                RefundStatus::Refunded => Message::new("purchase.refunded").arg("error", detail),
                RefundStatus::OwnershipChanged => Message::new("purchase.ownership_changed").arg("error", detail),
                RefundStatus::RefundFailed(error) => Message::new("purchase.refund_failed").arg("error", format!("{detail}; {error}")),
            },
        }
    }

    pub(crate) fn center(&self) {
        let Some(mode) = self.controller.current_mode() else { return };
        for slot in 0..s2sdk::GetMaxClients() {
            if s2sdk::IsClientInGame(slot) {
                let language = crate::i18n::language(slot, &self.texts);
                let name = mode.localized_name(&self.texts, &language).expect("validated mode name");
                s2sdk::PrintCentreHtml(slot, &Str::from_str(&escape_html_text(&name)), 5);
            }
        }
    }

    pub fn command(&mut self, command: Command, slot: i32) -> Result<String, String> {
        match command {
            Command::Status => {
                let summary = format!(
                    "[FunRounds] {} enabled={} phase={:?} current={} next={} pending_reload={} effects_ready={} diagnostics={}",
                    funrounds_core::VERSION,
                    self.controller.enabled(),
                    self.controller.phase(),
                    self.controller.current_mode().map_or("ordinary", |mode| mode.id.as_str()),
                    self.controller.forced_next().unwrap_or("random"),
                    self.controller.pending_config().is_some(),
                    self.effects_ready,
                    self.diagnostics(),
                );
                let (hostage_rescue_available, hostage_rescue_reason) = self.hostage_rescue_status();
                let details = format!(
                    "config={}\ncapabilities={:?}\nexcluded={}\ncapability_reasons={}\nhostage_rescue_compat_enabled={}\nhostage_rescue_compat_available={}\nhostage_rescue_compat_reason={}",
                    self.config_path.display(),
                    self.controller.capabilities(),
                    self.controller
                        .config()
                        .map(|config| config
                            .modes
                            .iter()
                            .filter(|mode| mode.enabled)
                            .filter_map(|mode| {
                                let missing = self
                                    .controller
                                    .capabilities()
                                    .missing_for_with_chicken_config(mode, config.chicken.is_some());
                                (!missing.is_empty()).then(|| format!("{}:[{}]", mode.id, missing.join(",")))
                            })
                            .collect::<Vec<_>>()
                            .join(";"))
                        .unwrap_or_default(),
                    self.capability_reasons.join(";"),
                    self.controller.hostage_rescue_compat_enabled(),
                    hostage_rescue_available,
                    hostage_rescue_reason,
                );
                Ok(format_status(
                    &summary,
                    &self.faults,
                    !self.effects_ready && !self.cleanup_complete(),
                    &details,
                ))
            }
            Command::List => Ok(self.controller.config().map_or_else(
                || self.text(slot, "command.no_config", &[]),
                |config| {
                    config
                        .modes
                        .iter()
                        .map(|mode| {
                            format!(
                                "{} enabled={} {}",
                                mode.id,
                                mode.enabled,
                                mode.localized_name(&self.texts, self.texts.default_language())
                                    .expect("validated mode name")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                },
            )),
            Command::Next(next) => {
                self.controller.force_next(next)?;
                Ok(self.text(slot, "command.next", &[("mode", self.controller.forced_next().unwrap_or("random"))]))
            }
            Command::Reload => {
                let source = std::fs::read_to_string(&self.config_path).map_err(|error| format!("{}: {error}", self.config_path.display()))?;
                let candidate = Config::parse(&source)?;
                let texts = crate::i18n::load(&self.i18n_path, &candidate.default_language, true, Some(&candidate))?;
                texts.validate_config(&candidate)?;
                self.controller.queue_reload(&source)?;
                self.pending_texts = Some(texts);
                Ok(self.text(slot, "command.reload", &[]))
            }
            Command::Enable(enabled) => {
                let was_enabled = self.controller.enabled();
                if was_enabled != enabled {
                    self.prestarted_round = None;
                    self.warmup_activation_pending = enabled && self.warmup_active;
                }
                if !enabled {
                    self.ground_reverse.retry_cleanup();
                    self.cleanup_effects(false);
                    self.restore_team_names();
                    self.welcomes.clear();
                    self.hostage_rescue.clear_attempts();
                }
                if enabled && !was_enabled {
                    self.restore_duck_speed();
                    self.restore_model_scales();
                }
                self.controller.set_enabled(enabled);
                if !enabled {
                    self.restore_duck_speed();
                    self.restore_model_scales();
                    self.finish_recovery();
                }
                Ok(self.text(slot, "command.enabled", &[("enabled", &enabled.to_string())]))
            }
            Command::Info => {
                let mut information = format!("FunRounds {}\n", funrounds_core::VERSION);
                let language = crate::i18n::language(slot, &self.texts);
                if let Some(mode) = self.controller.current_mode() {
                    let phase = self.text(
                        slot,
                        match self.controller.phase() {
                            Phase::Preparing => "phase.preparing",
                            Phase::Running => "phase.running",
                            Phase::Warmup => "phase.warmup",
                            Phase::Idle => "phase.ordinary",
                        },
                        &[],
                    );
                    let name = mode.localized_name(&self.texts, &language)?;
                    let description = mode.localized_description(&self.texts, &language)?;
                    information += &self.text(
                        slot,
                        "info.mode",
                        &[("phase", &phase), ("id", &mode.id), ("name", &name), ("description", &description)],
                    );
                } else {
                    information += &self.text(slot, "info.ordinary", &[]);
                }
                if self
                    .controller
                    .current_mode()
                    .is_some_and(|mode| mode.win_condition == funrounds_core::WinCondition::TeamDeathmatch)
                {
                    information += &self.team_match_text(slot);
                }
                if let Some(shield) = self
                    .combat
                    .get(&slot)
                    .and_then(|(task, life)| (Some(task.player) == self.players.identity(slot)).then(|| life.shield()).flatten())
                {
                    information += &self.text(slot, "info.shield", &[("shield", &shield.to_string())]);
                }
                Ok(information)
            }
            Command::C4 => self.request_suicide(slot, false),
            Command::Suicide => self.request_suicide(slot, true),
            Command::Chicken => self.request_chicken(slot),
            Command::Healthshot => self.request_healthshot(slot),
            Command::NightVision => {
                if !self.controller.enabled() || self.faults.recovery != Recovery::Ready {
                    return Err(self.text(slot, "public.unavailable", &[]));
                }
                let player = LivePlayer::resolve(slot).ok_or(self.text(slot, "nvg.alive_required", &[]))?;
                let enabled = self.night_vision.toggle(player)?;
                Ok(if enabled {
                    self.text(slot, "nvg.enabled", &[])
                } else {
                    self.text(slot, "nvg.disabled", &[])
                })
            }
            Command::Wire(wire) => self.quickdefuse_wire(slot, wire),
            Command::Help => Ok(self.text(slot, "help.body", &[])),
            Command::Knock(enabled) => {
                if !self.classic_enabled() {
                    return Err(self.text(slot, "public.unavailable", &[]));
                }
                self.connect(slot);
                if self.players.connection_identity(slot).is_none() {
                    return Err("fr_knock requires an in-game player".into());
                }
                let feedback = self.preferences.as_mut().ok_or_else(|| "player preferences are closing".to_owned())?.set(
                    slot,
                    crate::preferences::authenticated_id(slot),
                    enabled,
                );
                let value = self.text(slot, feedback.value.key(), &[]);
                use crate::preferences::PreferencePersistence;
                Ok(match feedback.persistence {
                    PreferencePersistence::SessionOnly => self.text(slot, "preference.session", &[("value", &value)]),
                    PreferencePersistence::Unchanged => self.text(slot, "preference.unchanged", &[("value", &value)]),
                    PreferencePersistence::Queued => self.text(slot, "preference.queued", &[("value", &value)]),
                    PreferencePersistence::Failed(error) => self.text(slot, "preference.failed", &[("value", &value), ("error", &error)]),
                })
            }
        }
    }
}

fn sync_shield_hud(player: LivePlayer, armor: i32) -> Result<(), Fault> {
    if s2sdk::GetClientArmor(player.slot) != armor {
        s2sdk::SetClientArmor(player.slot, armor);
        if s2sdk::GetClientArmor(player.slot) != armor {
            return Err(Fault::mechanism(Mechanism::EnergyShield, "shield armor HUD write failed"));
        }
    }
    Ok(())
}

fn event_weapon_snapshot(player: LivePlayer, attacker: PlayerIdentity, event_weapon: &str) -> Option<KillWeaponTarget> {
    if LivePlayer::resolve(player.slot) != Some(player) {
        return None;
    }
    let item_definition_reader = unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
    let (handle, classname, item_definition) = unique_event_weapon_details(
        event_weapon,
        s2sdk::GetClientWeapons(player.slot)
            .iter()
            .copied()
            .filter(|handle| s2sdk::IsValidEntHandle(*handle) && s2sdk::GetEntityOwner(*handle) == player.pawn_handle)
            .map(|handle| {
                let classname = s2sdk::GetEntityClassname(handle).to_string();
                let item_definition = unsafe { item_definition_reader(handle) };
                (handle, classname, item_definition)
            }),
    )?;
    Some(KillWeaponTarget {
        attacker,
        handle,
        classname,
        item_definition,
    })
}

fn unique_event_weapon_details(event_weapon: &str, weapons: impl IntoIterator<Item = (i32, String, u16)>) -> Option<(i32, String, u16)> {
    if !upgrade_event_weapon(event_weapon) {
        return None;
    }
    let mut matching = weapons.into_iter().filter(|(_, classname, item_definition)| {
        crate::effects::equipment::kill_loadout_source_weapon(classname) && event_weapon_matches(event_weapon, classname, *item_definition)
    });
    let matched = matching.next()?;
    matching.next().is_none().then_some(matched)
}

fn upgrade_event_weapon(event_weapon: &str) -> bool {
    let name = event_weapon.strip_prefix("weapon_").unwrap_or(event_weapon);
    !name.is_empty() && crate::effects::equipment::kill_loadout_source_weapon(&format!("weapon_{name}"))
}

fn event_weapon_matches(event_weapon: &str, classname: &str, item_definition: u16) -> bool {
    let event_weapon = event_weapon_name(event_weapon);
    crate::effects::equipment::give_name_for_original(classname, item_definition)
        .is_some_and(|give_name| event_weapon_name(&give_name) == event_weapon)
}

fn event_weapon_name(name: &str) -> &str {
    match name.strip_prefix("weapon_").unwrap_or(name) {
        "knife" | "knife_t" => "knife",
        name => name,
    }
}

fn fired_weapon(player: LivePlayer, event_weapon: &str) -> Option<FiredWeapon> {
    if LivePlayer::resolve(player.slot) != Some(player) || !crate::effects::firearm_event(event_weapon) {
        return None;
    }
    let handle = s2sdk::GetClientActiveWeapon(player.slot);
    if !s2sdk::IsValidEntHandle(handle) || s2sdk::GetEntityOwner(handle) != player.pawn_handle {
        return None;
    }
    let classname = s2sdk::GetEntityClassname(handle).to_string();
    let item_definition_reader = unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
    let item_definition = unsafe { item_definition_reader(handle) };
    event_weapon_matches(event_weapon, &classname, item_definition).then_some(FiredWeapon {
        handle,
        classname,
        item_definition,
    })
}

fn weapon_identity_is_current(player: LivePlayer, weapon: &FiredWeapon) -> bool {
    if LivePlayer::resolve(player.slot) != Some(player)
        || s2sdk::GetClientActiveWeapon(player.slot) != weapon.handle
        || !s2sdk::IsValidEntHandle(weapon.handle)
        || s2sdk::GetEntityOwner(weapon.handle) != player.pawn_handle
        || s2sdk::GetEntityClassname(weapon.handle).as_str() != weapon.classname
    {
        return false;
    }
    let item_definition_reader = unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
    (unsafe { item_definition_reader(weapon.handle) }) == weapon.item_definition
}

fn format_status(summary: &str, faults: &Faults, cleanup_pending: bool, details: &str) -> String {
    format!(
        "{summary} recovery={:?} cleanup_pending={cleanup_pending}\nfaults={}\n{details}",
        faults.recovery,
        faults.reasons(),
    )
}

fn read_config(path: &std::path::Path) -> Result<Config, String> {
    let source = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Config::parse(&source).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn remove_native_c4(player: LivePlayer, handles: &[i32]) -> Result<(), Fault> {
    if LivePlayer::resolve(player.slot) != Some(player) {
        return Err(Fault::cancelled("player identity changed before native C4 cleanup"));
    }
    for handle in handles.iter().copied() {
        if !s2sdk::IsValidEntHandle(handle) || s2sdk::GetEntityClassname(handle).as_str() != "weapon_c4" {
            continue;
        }
        if s2sdk::GetEntityOwner(handle) != player.pawn_handle {
            return Err(Fault::cancelled("native C4 ownership changed before cleanup"));
        }
        s2sdk::RemoveWeapon(player.slot, handle);
        if LivePlayer::resolve(player.slot) != Some(player) {
            return Err(Fault::cancelled("player identity changed during native C4 cleanup"));
        }
        if s2sdk::GetClientWeapons(player.slot).iter().any(|current| *current == handle) {
            return Err(Fault::mechanism(Mechanism::NoC4, "engine retained the native C4 after removal"));
        }
        if s2sdk::IsValidEntHandle(handle) {
            if s2sdk::GetEntityClassname(handle).as_str() != "weapon_c4"
                || !matches!(s2sdk::GetEntityOwner(handle), owner if owner == -1 || owner == player.pawn_handle)
            {
                return Err(Fault::cancelled("native C4 ownership changed during cleanup"));
            }
            s2sdk::RemoveEntity(handle);
            if !crate::game::engine_removal_acknowledged(handle) {
                return Err(Fault::mechanism(Mechanism::C4Assignment, "native C4 entity survived cleanup"));
            }
        }
    }
    Ok(())
}

fn mode65_self_damage(damage: &crate::damage::Damage, pawn: i32, health: i32) -> bool {
    let damage_type = damage.damage_type;
    let native_self = damage.inflictor == pawn;
    // mp_weapon_self_inflict_amount uses pawn-attributed DMG_GENERIC, not DMG_BULLET.
    native_self && (damage_type == 0 || damage_type == 0x0200_0000 && health <= 1 && damage.amount == 1.0)
        || damage.weapon_kind == 1 && damage_type & crate::damage::DMG_BULLET != 0
}

fn self_damage_allowed(mode: &funrounds_core::Mode, damage: &crate::damage::Damage, pawn: i32, health: i32) -> bool {
    if damage.damage_type & 32 != 0 {
        return false;
    }
    match mode.self_damage {
        Some(SelfDamage::MissPenalty) => mode65_self_damage(damage, pawn, health),
        Some(SelfDamage::Native { amount }) => amount > 0.0,
        None => false,
    }
}

fn numeric_self_damage_enters_health(allowed_self: bool, blocked: bool, incoming: f32) -> bool {
    allowed_self && !blocked && incoming.is_finite() && incoming > 0.0
}

fn delayed_damage_protection_valid(protection_enabled: bool, target: PlayerTask, current: Option<(PlayerTask, bool)>) -> bool {
    !protection_enabled || current.is_some_and(|(task, protected)| task == target && !protected)
}

fn observe_lifecycle_tick(last: &mut Option<i32>, tick: i32, event: Option<&str>) -> bool {
    // Damage callbacks can rewind ticks; round boundaries can precede the first new-map frame.
    if event.is_some_and(|name| !matches!(name, "round_prestart" | "round_start")) {
        return false;
    }
    last.replace(tick).is_some_and(|previous| tick < previous)
}

fn prestarted_loadout_is_current(
    task: PlayerTask,
    action: PendingAction,
    round: RoundGeneration,
    current_identity: Option<PlayerIdentity>,
    live_player: Option<LivePlayer>,
    controller_handle: Option<i32>,
    pawn_controller: Option<i32>,
) -> bool {
    let Some(player) = live_player else { return false };
    task.round == round
        && action == PendingAction::Loadout
        && current_identity == Some(task.player)
        && player.slot == task.player.slot
        && player.pawn_handle as u32 == task.player.pawn_handle
        && controller_handle == Some(player.controller_handle)
        && pawn_controller == Some(player.controller_handle)
}

fn round_selection_waits(game_warmup: bool, effects_ready_after_cleanup: bool) -> bool {
    game_warmup || !effects_ready_after_cleanup
}

fn healthshot_override_settled(wanted: bool, active: bool, parameters_restored: bool) -> bool {
    wanted == active && (wanted || parameters_restored)
}

fn public_property_cleanup_complete(enabled: bool, duck_speed_empty: bool, model_scales_empty: bool) -> bool {
    enabled || (duck_speed_empty && model_scales_empty)
}

fn warmup_reactivation_needed(state: Option<bool>, warmup_active: bool, phase: Phase, activation_pending: bool, activation_available: bool) -> bool {
    state == Some(true) && warmup_active && phase != Phase::Warmup && !activation_pending && activation_available
}

fn discard_map_effects<T>(effects: &mut HashMap<i32, T>, retired_effects: &mut Vec<T>) {
    effects.clear();
    retired_effects.clear();
}

fn restore_or_retain<T>(retained: &mut Vec<T>, index: usize, restore: impl FnOnce(&mut T) -> Result<bool, String>) -> Result<(), String> {
    if restore(&mut retained[index])? {
        retained.remove(index);
    }
    Ok(())
}

fn log(message: &str) {
    s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] {message}")));
}

fn respawn_target_valid(expected: LivePlayer, current: Option<LivePlayer>, team: i32, current_team: i32, alive: bool) -> bool {
    current == Some(expected) && matches!(team, 2 | 3) && current_team == team && !alive
}

fn random_kill_loadout_start(mode: &funrounds_core::Mode, rng: &mut impl rand::Rng) -> Vec<String> {
    // Resample the same configured equipment categories at each life, not just at round start.
    mode.select_equipment(rng)
        .expect("validated random kill-loadout start")
        .into_iter()
        .map(str::to_owned)
        .collect()
}
fn warmup_spawn_needed(identity: Option<PlayerIdentity>, player: LivePlayer, effect_applied: bool, loadout_pending: bool) -> bool {
    identity.is_none_or(|identity| identity.pawn_handle != player.pawn_handle as u32) || !(effect_applied || loadout_pending)
}

fn round_respawn_task(players: &PlayerTracker, round: RoundGeneration, player: LivePlayer) -> Option<PlayerTask> {
    if let Some(identity) = players
        .identity(player.slot)
        .filter(|identity| identity.pawn_handle == player.pawn_handle as u32)
    {
        return Some(PlayerTask { player: identity, round });
    }
    players.respawn_task(player.slot, round)
}

fn round_respawn_task_valid(players: &PlayerTracker, task: PlayerTask, round: RoundGeneration, player: LivePlayer) -> bool {
    task.player.slot == player.slot
        && if task.player.pawn_handle == u32::MAX {
            players.validates_respawn(task, round)
        } else {
            players.validates(task, round, player.pawn_handle as u32)
        }
}

fn round_respawn_target_valid(
    all_respawn: bool,
    phase: Phase,
    expected: LivePlayer,
    current: Option<LivePlayer>,
    team: i32,
    current_team: i32,
) -> bool {
    all_respawn && phase != Phase::Idle && current == Some(expected) && matches!(team, 2 | 3) && current_team == team
}

fn lifesteal_eligible(attacker: i32, victim: i32, attacker_alive: bool) -> bool {
    attacker >= 0 && victim >= 0 && attacker != victim && attacker_alive
}

fn respawn_window_open(phase: Phase, remaining: Option<f32>) -> bool {
    phase == Phase::Preparing || phase == Phase::Warmup || (phase == Phase::Running && remaining.is_none_or(|seconds| seconds > 0.0))
}

fn center_display_due(current: bool, phase: Phase, tick: i32, scheduled: i32) -> bool {
    current && phase != Phase::Idle && tick > scheduled
}

fn center_display_action_valid(generation_matches: bool, phase: Phase, effects_ready: bool) -> bool {
    generation_matches && phase != Phase::Idle && effects_ready
}

fn escape_html_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn separate_console_output(message: String) -> String {
    format!("\n{message}")
}

fn grenade_thrown_route(mode: Option<&funrounds_core::Mode>, kind: Option<equipment::GrenadeKind>) -> GrenadeThrownRoute {
    let Some((mode, kind)) = mode.zip(kind) else {
        return GrenadeThrownRoute::default();
    };

    GrenadeThrownRoute {
        track_dodgeball_decoy: kind == equipment::GrenadeKind::Decoy && mode.dodgeball,
        replenish: mode.replenish.contains(&kind),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stopped_status_puts_recovery_and_fault_before_a_large_excluded_list() {
        let mut faults = crate::fault::Faults::new(funrounds_core::Capabilities::default());
        faults.record(&crate::fault::Fault::global("created action failed after cancellation"));
        let details = format!(
            "capabilities={:?}\nexcluded={}\ncapability_reasons=",
            faults.capabilities(),
            "sample:[equipment,health,armor,gravity,speed];".repeat(40)
        );
        assert!(details.len() > 2047);
        let status = super::format_status(
            "[FunRounds] enabled=false phase=Idle current=ordinary effects_ready=false",
            &faults,
            false,
            &details,
        );
        let parts = crate::commands::status_reply_parts(&status).collect::<Vec<_>>();
        assert!(parts[0].ends_with("recovery=Stopped cleanup_pending=false"));
        assert_eq!(parts[1], "faults=Global:[created action failed after cancellation]");
        assert!(parts[2].starts_with("capabilities="));
        assert_eq!(parts[3..parts.len() - 1].concat().matches("sample:[").count(), 40);
        assert_eq!(parts.last(), Some(&"capability_reasons="));
    }

    #[test]
    fn center_mode_name_waits_for_the_next_tick_and_cancels_when_stale() {
        use funrounds_core::Phase;

        assert!(!super::center_display_due(true, Phase::Preparing, 10, 10));
        assert!(super::center_display_due(true, Phase::Preparing, 11, 10));
        assert!(!super::center_display_due(false, Phase::Preparing, 11, 10));
        assert!(!super::center_display_due(true, Phase::Idle, 11, 10));
    }

    #[test]
    fn center_mode_name_is_rendered_as_text_not_html() {
        assert_eq!(super::escape_html_text("示例 & <example>"), "示例 &amp; &lt;example&gt;");
    }

    #[test]
    fn center_action_revalidates_generation_phase_and_effect_readiness() {
        use funrounds_core::Phase;

        assert!(super::center_display_action_valid(true, Phase::Preparing, true));
        assert!(!super::center_display_action_valid(false, Phase::Preparing, true));
        assert!(!super::center_display_action_valid(true, Phase::Idle, true));
        assert!(!super::center_display_action_valid(true, Phase::Preparing, false));
    }

    #[test]
    fn console_output_starts_on_a_separate_line() {
        assert_eq!(super::separate_console_output("[FunRounds] mode".into()), "\n[FunRounds] mode");
    }

    #[test]
    fn miss_penalty_respects_native_self_damage_and_final_health_point() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        mode.self_damage = Some(funrounds_core::SelfDamage::MissPenalty);
        let mut damage = crate::damage::Damage {
            victim: 42,
            attacker: 42,
            inflictor: 42,
            weapon_kind: 0,
            hit_group: -1,
            traced_head: false,
            amount: 36.0,
            damage_type: 0,
            weapon: -1,
            weapon_class: None,
            skip_plugin_rules: false,
        };
        assert!(super::self_damage_allowed(&mode, &damage, 42, 200));
        assert!(!super::self_damage_allowed(&mode, &damage, 43, 200));
        for damage_type in [8, 64, 256, 0x0200_0000] {
            damage.damage_type = damage_type;
            assert!(!super::self_damage_allowed(&mode, &damage, 42, 200));
        }
        damage.amount = 1.0;
        assert!(super::self_damage_allowed(&mode, &damage, 42, 1));
        assert!(!super::self_damage_allowed(&mode, &damage, 42, 2));
        damage.damage_type = 0;
        assert!(super::self_damage_allowed(&mode, &damage, 42, 1));
        damage.damage_type = 2;
        assert!(!super::self_damage_allowed(&mode, &damage, 42, 200));
        damage.weapon_kind = 1;
        assert!(super::self_damage_allowed(&mode, &damage, 42, 200));
    }

    #[test]
    fn localized_team_info_distinguishes_waiting_and_running_clocks() {
        let texts = funrounds_core::TextBundle::embedded("zh-CN").unwrap();
        assert_eq!(
            texts.render("zh-CN", "team.waiting", &[]).unwrap(),
            "\n比分：T 0 : 0 CT\n剩余时间：计时未开始（冻结结束后开始）"
        );
        let game = funrounds_core::TeamDeathmatch::start(10.0, 60.0).unwrap();
        let seconds = format!("{:.0}", game.remaining(12.0));
        assert_eq!(
            texts
                .render("zh-CN", "team.info", &[("t", "0"), ("ct", "0"), ("seconds", &seconds)])
                .unwrap(),
            "\n比分：T 0 : 0 CT\n剩余时间：58s"
        );
    }
    use super::discard_map_effects;
    use std::collections::HashMap;

    #[test]
    fn event_tick_rewinds_do_not_discard_map_state_but_frame_rewinds_do() {
        let mut last = None;
        assert!(!super::observe_lifecycle_tick(&mut last, 1294, None));
        assert!(!super::observe_lifecycle_tick(&mut last, 1293, Some("player_hurt")));
        assert!(!super::observe_lifecycle_tick(&mut last, 1295, Some("weapon_fire")));
        assert_eq!(last, Some(1294));
        assert!(!super::observe_lifecycle_tick(&mut last, 1294, None));
        assert!(!super::observe_lifecycle_tick(&mut last, 1295, None));
        assert!(super::observe_lifecycle_tick(&mut last, 0, None));
        assert_eq!(last, Some(0));
    }

    #[test]
    fn new_map_round_boundaries_before_frame_discard_old_state_and_reach_running() {
        use funrounds_core::{Capabilities, Config, Phase, RoundController};
        use rand::{SeedableRng, rngs::StdRng};

        let config = Config::parse(include_str!("../../../config/funrounds.example.toml")).unwrap();
        let capabilities = Capabilities {
            equipment: true,
            health: true,
            armor: true,
            gravity: true,
            speed: true,
            helmet: true,
            clear_weapons: true,
            give_weapons: true,
            ct_bomb_no_defuse: true,
            ..Capabilities::default()
        };
        for first_event in ["round_prestart", "round_start"] {
            let mut controller = RoundController::new(Some(config.clone()), capabilities).unwrap();
            let mut rng = StdRng::seed_from_u64(1);
            let mut last = Some(10000);
            let old_key = controller.round_key(10000);
            assert!(controller.start_round(old_key, false, &mut rng));
            assert!(controller.freeze_end(old_key));

            assert!(super::observe_lifecycle_tick(&mut last, 1, Some(first_event)));
            controller.reset();
            let new_key = controller.round_key(1);
            assert_ne!(new_key, old_key);
            assert!(!super::observe_lifecycle_tick(&mut last, 1, Some("round_start")));
            assert!(controller.start_round(new_key, false, &mut rng));
            assert_eq!(controller.phase(), Phase::Preparing);
            assert!(!super::observe_lifecycle_tick(&mut last, 1, None));
            assert!(controller.freeze_end(new_key));
            assert_eq!(controller.phase(), Phase::Running);
        }
    }

    #[test]
    fn lifesteal_excludes_self_world_and_dead_attackers() {
        assert!(super::lifesteal_eligible(1, 2, true));
        assert!(!super::lifesteal_eligible(1, 1, true));
        assert!(!super::lifesteal_eligible(-1, 2, true));
        assert!(!super::lifesteal_eligible(1, -1, true));
        assert!(!super::lifesteal_eligible(1, 2, false));
    }

    #[test]
    fn team_respawn_works_during_freeze_without_starting_the_score_timer() {
        use funrounds_core::Phase;
        assert!(super::respawn_window_open(Phase::Preparing, None));
        assert!(super::respawn_window_open(Phase::Running, Some(3.0)));
        assert!(!super::respawn_window_open(Phase::Running, Some(0.0)));
        assert!(super::respawn_window_open(Phase::Running, None));
        assert!(!super::respawn_window_open(Phase::Idle, Some(30.0)));
    }

    #[test]
    fn respawn_rejects_changed_handles_team_and_external_resurrection() {
        let player = crate::game::LivePlayer {
            slot: 1,
            controller_handle: 0x4001,
            pawn_handle: 0x4002,
        };
        assert!(super::respawn_target_valid(player, Some(player), 2, 2, false));
        assert!(!super::respawn_target_valid(player, Some(player), 2, 3, false));
        assert!(!super::respawn_target_valid(player, Some(player), 2, 2, true));
        assert!(!super::respawn_target_valid(player, None, 2, 2, false));
        assert!(!super::respawn_target_valid(
            player,
            Some(crate::game::LivePlayer {
                pawn_handle: 0x8002,
                ..player
            }),
            2,
            2,
            false
        ));
        assert!(!super::respawn_target_valid(
            player,
            Some(crate::game::LivePlayer {
                controller_handle: 0x8001,
                ..player
            }),
            2,
            2,
            false
        ));
    }

    #[test]
    fn map_change_discards_active_and_retired_entity_snapshots() {
        let mut effects = HashMap::from([(1, "active")]);
        let mut retired_effects = vec!["retired"];
        discard_map_effects(&mut effects, &mut retired_effects);
        assert!(effects.is_empty());
        assert!(retired_effects.is_empty());
    }

    #[test]
    fn failed_retired_restoration_keeps_its_snapshot_for_retry() {
        let mut retained = vec!["sample color"];
        let result = super::restore_or_retain(&mut retained, 0, |_| Err("color readback failed".into()));
        assert_eq!(result, Err("color readback failed".into()));
        assert_eq!(retained, ["sample color"]);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::restore_or_retain(&mut retained, 0, |_| panic!("restoration panic"))
            }))
            .is_err()
        );
        assert_eq!(retained, ["sample color"]);
        super::restore_or_retain(&mut retained, 0, |_| Ok(false)).unwrap();
        assert_eq!(retained, ["sample color"]);
        super::restore_or_retain(&mut retained, 0, |_| Ok(true)).unwrap();
        assert!(retained.is_empty());
    }
}
