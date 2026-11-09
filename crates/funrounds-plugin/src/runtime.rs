// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

pub(crate) mod afterburn;
pub(crate) mod chickens;
pub(crate) mod combat;
mod composed;
pub(crate) mod grapple;
pub(crate) mod healthshot;
pub(crate) mod proximity;
mod public_properties;
mod regions;
pub(crate) mod respawn;
pub(crate) mod shooting;
pub(crate) mod suicide;
pub(crate) mod teamkill;
pub(crate) use combat::RandomHealReceipt;
pub(crate) use composed::execute as execute_composed;

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
use rand::{RngExt, SeedableRng, rngs::StdRng};
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
    pub smoke_events: bool,
    pub spawn_listener_available: bool,
    pub weapon_fire: bool,
    pub bullet_impact: bool,
    pub player_hurt: bool,
    pub player_blind: bool,
    pub damage: bool,
    pub chicken_damage: bool,
    pub chicken_follow_block_support: bool,
    pub combat_events: bool,
    pub bot_takeover: crate::capability::Probe,
    pub round_prestart: bool,
    pub bombs: crate::capability::Probe,
    pub grapple_bomb_use: bool,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FiredWeapon {
    handle: i32,
    classname: String,
    item_definition: u16,
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

fn tracks_chicken_life(phase: Phase) -> bool {
    matches!(phase, Phase::Preparing | Phase::Running | Phase::Warmup)
}

fn mode_configured(config: &funrounds_core::Config, predicate: impl Fn(&funrounds_core::Mode) -> bool) -> bool {
    config.modes.iter().chain(config.warmup.iter().map(|warmup| &warmup.mode)).any(predicate)
}

fn active_combat_phase(phase: Phase) -> bool {
    matches!(phase, Phase::Running | Phase::Warmup)
}

fn wall_hang_capability(reasons: &mut Vec<String>) -> bool {
    let mut probe = crate::capability::Probe::default();
    crate::capability::imports!(probe;
        __s2sdk_GetEntityMoveType, __s2sdk_GetClientFlags, __s2sdk_GetClientButtons,
        __s2sdk_GetClientAbsOrigin, __s2sdk_GetEntityAbsVelocity, __s2sdk_SetEntityAbsVelocity,
        __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer, __s2sdk_EntIndexToEntHandle,
        __s2sdk_GetEntityClassname, __s2sdk_GetSchemaOffset, __s2sdk_GetEntSchema2,
        __s2sdk_GetEntSchemaVector3D2, __s2sdk_GetClientGravity, __s2sdk_SetClientGravity,
    );
    let native = unsafe { std::ptr::addr_of!(crate::funrounds_native::__funrounds_native_HasPlayerMovementHullSupport).read() };
    probe.require(unsafe { native.is_some_and(|support| support()) }, "native:HasPlayerMovementHullSupport");
    for (class, field) in [
        ("CBasePlayerPawn", "m_pMovementServices"),
        ("CPlayer_MovementServices", "m_nQueuedButtonDownMask"),
        ("CPlayer_MovementServices", "m_nQueuedButtonChangeMask"),
        ("CPlayer_MovementServices", "m_nButtons"),
        ("CInButtonState", "m_pButtonStates"),
        ("CBaseEntity", "m_pCollision"),
        ("CCollisionProperty", "m_vecMins"),
        ("CCollisionProperty", "m_vecMaxs"),
    ] {
        probe.schema(class, field);
    }
    probe.record("wall_hang", reasons)
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

// Execute after releasing the runtime lock: these engine calls can fire synchronous events.
pub enum EngineAction {
    Composed(composed::Action),
    Shooting(shooting::Action),
    Proximity(proximity::Action),
    Shockwave(proximity::ShockwaveAction),
    HeGuidance(proximity::GuidanceAction),
    HeGuidanceFuseRestore(proximity::GuidanceFuseRestoreAction),
    Teamkill(teamkill::Action),
    Rollback(PlayerTask, LivePlayer),
    Loadout(PlayerTask, LivePlayer, Vec<String>),
    SettleLoadout(PlayerTask, LivePlayer, i32),
    Upgrade(PlayerTask, LivePlayer, Option<KillWeaponTarget>),
    ThirdPerson(PlayerTask, LivePlayer),
    Suicide(suicide::Action),
    Respawn(respawn::Action),
    EndRound(RoundGeneration, s2sdk::CSRoundEndReason),
    CenterDisplay { generation: RoundGeneration },
    Projectile(crate::projectiles::Action),
    HostageRescue(crate::compat::hostage_rescue::ForwardAction),
    Bomb(crate::bombs::Action),
    Equipment(equipment::Action),
    ProjectileImpact(crate::projectiles::Impact),
    Combat(combat::Action),
    Chicken(chickens::Action),
    Healthshot(healthshot::Action),
    Afterburn(afterburn::Action),
    Grapple(grapple::Action),
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
    healthshot: healthshot::State,
    ground_reverse: crate::ground_reverse::GroundReverse,
    air_strafe: crate::autostrafe::AirStrafe,
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
    respawning: respawn::State,
    bombs: crate::bombs::Bombs,
    projectiles: crate::projectiles::Projectiles,
    hostage_rescue: crate::compat::hostage_rescue::State,
    chickens: crate::chicken_adapter::ChickenAdapter,
    ordinary_chickens: chickens::State,
    use_target_hook_ready: bool,
    grapple_bomb_use_ready: bool,
    use_target_hook_requested: bool,
    use_target_probe_retry_at: f32,
    grapple: grapple::State,
    he_guidance_spawn_listener_available: bool,
    he_guidance_spawn_listener_requested: bool,
    he_guidance_spawn_listener_ready: bool,
    he_guidance_probe_ready: bool,
    composed: composed::State,
    regions: regions::State,
    shooting: shooting::State,
    proximity: proximity::State,
    teamkills: teamkill::State,
    afterburns: crate::afterburn::Afterburns,
    impacts: Vec<crate::projectiles::Impact>,
    chicken_impacts: Vec<crate::projectiles::ChickenImpact>,
    combat: combat::State,
    suicides: crate::suicide::Suicides,
    death_explosives: suicide::DeathExplosions,
    engine_actions: Vec<EngineAction>,
    equipment_in_flight: usize,
    settling_loadouts: Vec<(PlayerTask, i32, i32)>,
    prestart_context: Option<(i32, bool)>,
    warmup_active: bool,
    warmup_activation_pending: bool,
    warmup_round_faulted: bool,
    round_transition_waiting: bool,
    public_properties: public_properties::State,
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
        let mut autostrafe = crate::capability::Probe::default();
        autostrafe.require(crate::autostrafe::available(), "native:air_move");
        autostrafe.require(events.combat_events, "events:player_spawn/player_death");
        capabilities.autostrafe = autostrafe.record("autostrafe", &mut capability_reasons);
        capabilities.healthshot = events.healthshot;
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
        let mut death_explosion = crate::suicide::blast_probe();
        death_explosion.require(events.damage, "hook:entity_damage");
        death_explosion.require(events.player_hurt, "event:player_hurt");
        death_explosion.require(events.combat_events, "events:player_spawn/player_death");
        death_explosion.require(crate::damage::reflection_available(), "import:TakeEntityDamageWithWeapon");
        death_explosion.require(
            crate::KNIFE_DEATH_PRE_HOOKED.load(std::sync::atomic::Ordering::Acquire),
            "hook:player_death_pre",
        );
        death_explosion.require(unsafe { s2sdk::__s2sdk_SetEventString }.is_some(), "import:SetEventString");
        capabilities.death_explosion = death_explosion.record("death_explosion", &mut capability_reasons);
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
            events.bombs.clone(),
            events.bomb_plant_tracking.clone(),
            events.moving_blast.clone(),
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
        capabilities.wall_hang = wall_hang_capability(&mut capability_reasons);
        capabilities.grapple = false;
        if config.as_ref().is_ok_and(|config| mode_configured(config, |mode| mode.grapple.is_some())) {
            capability_reasons.push("grapple:[waiting for a live pawn and the shared use-target observer]".into());
        }
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
        composed::probe(&events, &mut capabilities, &mut capability_reasons);
        let regions = regions::probe(&events, &mut capabilities, &mut capability_reasons);
        shooting::probe(&events, &mut capabilities, &mut capability_reasons);
        proximity::probe(&events, &mut capabilities, &mut capability_reasons);
        let he_guidance_probe_ready = capabilities.he_guidance;
        capabilities.he_guidance = false;
        if he_guidance_probe_ready
            && config
                .as_ref()
                .is_ok_and(|config| mode_configured(config, |mode| mode.he_guidance.is_some()))
        {
            capability_reasons.push("he_guidance:[waiting for entity spawn listener registration]".into());
        }
        teamkill::probe(&events, &mut capabilities, &mut capability_reasons);
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
            healthshot: healthshot::State::default(),
            ground_reverse: crate::ground_reverse::GroundReverse::default(),
            air_strafe: crate::autostrafe::AirStrafe::default(),
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
            respawning: respawn::State::default(),
            bombs: crate::bombs::Bombs::default(),
            projectiles: crate::projectiles::Projectiles::default(),
            hostage_rescue: crate::compat::hostage_rescue::State::new(),
            chickens: crate::chicken_adapter::ChickenAdapter::default(),
            ordinary_chickens: chickens::State::new(chicken_ready, events.chicken_follow_block_support),
            use_target_hook_ready: false,
            grapple_bomb_use_ready: events.grapple_bomb_use,
            use_target_hook_requested: false,
            use_target_probe_retry_at: 0.0,
            grapple: grapple::State::default(),
            he_guidance_spawn_listener_available: events.spawn_listener_available,
            he_guidance_spawn_listener_requested: false,
            he_guidance_spawn_listener_ready: false,
            he_guidance_probe_ready,
            composed: crate::runtime::composed::State::default(),
            regions,
            shooting: crate::runtime::shooting::State::default(),
            proximity: crate::runtime::proximity::State::default(),
            teamkills: crate::runtime::teamkill::State::default(),
            afterburns: crate::afterburn::Afterburns::default(),
            impacts: Vec::new(),
            chicken_impacts: Vec::new(),
            combat: combat::State::default(),
            suicides: crate::suicide::Suicides::default(),
            death_explosives: suicide::DeathExplosions::default(),
            engine_actions: Vec::new(),
            equipment_in_flight: 0,
            settling_loadouts: Vec::new(),
            prestart_context: None,
            warmup_active: false,
            warmup_activation_pending: false,
            warmup_round_faulted: false,
            round_transition_waiting: false,
            public_properties: public_properties::State::default(),
        })
    }

    fn he_guidance_configured(&self) -> bool {
        self.controller
            .config()
            .is_some_and(|config| mode_configured(config, |mode| mode.he_guidance.is_some()))
    }

    pub(crate) fn take_he_guidance_spawn_listener_request(&mut self) -> bool {
        let requested = self.he_guidance_configured() && self.he_guidance_spawn_listener_available && !self.he_guidance_spawn_listener_requested;
        self.he_guidance_spawn_listener_requested |= requested;
        requested
    }

    pub(crate) fn he_guidance_spawn_listener_ready(&mut self) {
        self.he_guidance_spawn_listener_ready = true;
        if !self.he_guidance_probe_ready || !self.faults.enable_he_guidance() {
            return;
        }
        self.capability_reasons
            .retain(|reason| !reason.starts_with("he_guidance:[waiting for entity spawn listener registration]"));
        self.controller.set_capabilities(self.faults.capabilities());
        log("HE guidance capability became available after the entity spawn listener was registered");
    }

    pub(crate) fn sync_he_guidance_spawn_epoch(&self) {
        crate::publish_he_guidance_spawn_active(self.he_guidance_active());
    }

    fn he_guidance_active(&self) -> bool {
        self.he_guidance_spawn_listener_ready
            && self.classic_round()
            && active_combat_phase(self.controller.phase())
            && self.faults.capabilities().he_guidance
            && self.controller.current_mode().is_some_and(|mode| mode.he_guidance.is_some())
    }

    pub(crate) fn take_interaction_hook_requests(&mut self) -> (bool, bool) {
        self.bind_pending_warmup_spawns();
        let healthshot = self.take_healthshot_hook_request();
        let chicken = self.wants_use_target_hook()
            && !self.use_target_hook_requested
            && s2sdk::GetGameTime() >= self.use_target_probe_retry_at
            && self.use_target_probe_ready();
        self.use_target_hook_requested |= chicken;
        (healthshot, chicken)
    }

    fn bind_pending_warmup_spawns(&mut self) {
        if !self.warmup_active || self.controller.phase() != Phase::Idle || !self.grapple_configured() {
            return;
        }

        for slot in 0..s2sdk::GetMaxClients() {
            let Some(expected) = self.players.pending_identity(slot) else { continue };
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if !self.players.validates_connection(expected)
                || self.controllers.get(&slot) != Some(&player.controller_handle)
                || player.pawn_controller() != Some(player.controller_handle)
            {
                continue;
            }

            self.players.bind_spawn(expected, player.pawn_handle as u32);
        }
    }

    pub(crate) fn chicken_use_hook_ready(&mut self) {
        self.use_target_hook_ready = true;
        self.grapple_use_hook_ready();
    }

    pub(crate) fn chicken_use_hook_failure(&mut self, reason: impl Into<String>) {
        self.use_target_hook_ready = false;
        let reason = reason.into();
        if self.chicken_use_configured() {
            self.capability_reasons.push(format!("chicken_eating:[{reason}]"));
            log(&format!("chicken eating capability unavailable: {reason}"));
        }
        if self.grapple_configured() {
            self.grapple_use_hook_failure(reason);
        }
    }

    pub(crate) fn chicken_use_hook_deferred(&mut self) {
        self.use_target_hook_requested = false;
        let now = s2sdk::GetGameTime();
        self.use_target_probe_retry_at = if now.is_finite() { now + 1.0 } else { 0.0 };
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

    fn air_strafe_active(&self) -> bool {
        self.classic_enabled() && active_combat_phase(self.controller.phase()) && self.controller.current_mode().is_some_and(|mode| mode.autostrafe)
    }

    pub(crate) fn air_strafe_allowed(&self, slot: i32, pawn: i32, controller: i32) -> bool {
        if !self.on_game_thread() || !self.air_strafe_active() {
            return false;
        }
        let player = LivePlayer {
            slot,
            controller_handle: controller,
            pawn_handle: pawn,
        };
        let Some(task) = self.combat_task(player) else { return false };
        !self
            .effects
            .get(&slot)
            .is_some_and(|effect| effect.identity == task.player && effect.round == task.round && effect.temporary_movement_owner().is_some())
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
        self.clear_region_player_life(identity.slot);
        self.clear_composed_life(identity);
        self.ordinary_chickens.clear_pending_chicken_blast_for_life(identity);
        self.chickens.clear_life(PlayerTask {
            round: self.controller.generation(),
            player: identity,
        });
        self.afterburns.clear_life(PlayerTask {
            round: self.controller.generation(),
            player: identity,
        });
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
                    .lives
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

    fn wall_hang_active(&self) -> bool {
        active_combat_phase(self.controller.phase())
            && self.faults.capabilities().wall_hang
            && self.controller.current_mode().is_some_and(|mode| mode.wall_hang)
    }

    fn refresh_movement(&mut self) {
        let enabled = self.wall_hang_active();
        let movement_requested = enabled || self.grapple_mode_active();
        let generation = self.controller.generation();
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
            let result = if self.suicides.active(player) && (movement_requested || effect.temporary_movement_owner().is_some()) {
                effect.interrupt_temporary_movement()
            } else {
                effect.update_wall_hang(enabled)
            };
            if let Err(error) = result
                && error.scope != Scope::Cancelled
            {
                failure = Some(error.context(format!("wall hang slot={slot} round={generation:?}")));
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    fn refresh_wall_hang_capability(&mut self) {
        if self.faults.capabilities().wall_hang || self.faults.recovery == Recovery::Stopped {
            return;
        }
        let mut reasons = Vec::new();
        if !wall_hang_capability(&mut reasons) || !self.faults.enable_wall_hang() {
            return;
        }
        self.capability_reasons.retain(|reason| !reason.starts_with("wall_hang:"));
        self.controller.set_capabilities(self.faults.capabilities());
        log("wall_hang capability became available after the map movement receiver initialized");
    }

    pub(crate) fn interrupt_movement_for_impulse(&mut self, player: LivePlayer) -> Result<(), Fault> {
        if let Some(identity) = self
            .players
            .identity(player.slot)
            .filter(|identity| identity.pawn_handle == player.pawn_handle as u32)
        {
            self.cancel_grapple(identity, grapple::StopReason::Impulse);
        }
        let movement_requested = self.wall_hang_active() || self.grapple_mode_active();
        let Some(effect) = self.effects.get_mut(&player.slot).filter(|effect| {
            !effect.has_pending_loadout()
                && effect.identity.pawn_handle == player.pawn_handle as u32
                && Some(effect.identity) == self.players.identity(player.slot)
                && self.controllers.get(&player.slot) == Some(&player.controller_handle)
                && effect.round == self.controller.generation()
        }) else {
            return Ok(());
        };
        if effect.temporary_movement_owner().is_none() && !movement_requested {
            return Ok(());
        }
        effect.interrupt_temporary_movement()
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
            EngineAction::Composed(action) => self.composed_action_valid(action),
            EngineAction::Shooting(action) => self.shooting_action_valid(action),
            EngineAction::Proximity(action) => self.proximity_action_valid(*action),
            EngineAction::HeGuidance(action) => self.he_guidance_action_valid(*action),
            EngineAction::HeGuidanceFuseRestore(action) => self.he_guidance_fuse_restore_action_valid(*action),
            EngineAction::Grapple(action) => self.grapple_action_valid(action),
            EngineAction::Shockwave(action) => self.shockwave_action_valid(*action),
            EngineAction::Teamkill(action) => self.teamkill_action_valid(*action),
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
            EngineAction::Suicide(action) => self.suicide_action_current(action),
            EngineAction::Combat(action) => self.combat_action_current(action),
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
                    && matches!(s2sdk::GetClientTeam(action.player.slot) as i32, 2 | 3)
                    && crate::compat::hostage_rescue::eligible_hostage(action.trigger, action.player) == Some(action.hostage)
            }
            EngineAction::Bomb(action) => self.bombs.action_valid(*action, self.controller.generation(), &self.players),
            EngineAction::Chicken(action) => self.chickens_action_current(action),
            EngineAction::Afterburn(action) => self.afterburn_action_current(action),
            EngineAction::Healthshot(action) => self.healthshot_action_current(action),
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
            EngineAction::Respawn(action) => self.respawn_action_current(action),
            EngineAction::EndRound(round, _) => {
                self.controller.phase() == Phase::Running && self.controller.generation() == *round && self.team_match.is_some()
            }
        }
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
        if let Some((bomb, chicken)) = self.bombs.take_chicken_model_scale_candidate()
            && self.controller.enabled()
            && let Err(error) = self.public_properties.scale_chicken_bomb(bomb, chicken, &mut self.rng)
        {
            self.handle_fault(error);
        }
    }

    pub(crate) fn prepare_bomb_action_scales(&mut self, action: crate::bombs::Action) -> Result<(), Fault> {
        if let Some(chicken) = action.cleanup_chicken() {
            // Restore the parent while the C4 is still attached. Detaching preserves
            // world scale, so restoring afterwards would leave a scaled C4 behind.
            if !self.public_properties.restore_model_scale(chicken) {
                return Err(Fault::global("chicken/C4 group scale restoration pending before detach"));
            }
        }
        Ok(())
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
        self.on_region_map_ready();
        self.refresh_wall_hang_capability();
        self.refresh_grapple_capability();
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

    fn restore_completed_native_bomb_scales(&mut self) {
        for handle in self.bombs.take_native_model_scale_restores() {
            self.public_properties.restore_model_scale(handle);
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

    pub(crate) fn update_with_spawned_entities(&mut self, spawned_entities: Vec<proximity::SpawnObservation>) -> Vec<EngineAction> {
        self.update_healthshot();
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
        let air_strafe = tick >= 0 && self.air_strafe_active();
        if let Err(error) = self.air_strafe.apply(air_strafe) {
            self.handle_fault(error);
        }
        self.release_chicken_use_buttons();
        self.combat.expire_combat_receipts(tick, &self.players);
        self.ordinary_chickens
            .expire_chicken_blast_kills(tick, self.controller.generation(), &self.players);
        self.recover_incomplete_effects();
        let warmup = game::warmup();
        self.sync_warmup(warmup, tick, None);
        self.arm_death_explosives();
        self.public_properties
            .update(self.controller.enabled(), &self.players, self.controller.generation(), &mut self.rng);
        if self.warmup_active && self.controller.phase() == Phase::Warmup && !self.warmup_activation_pending && self.effects_ready && tick >= 0 {
            self.reconcile_warmup_players(tick);
        }
        let now = s2sdk::GetGameTime();
        self.update_proximity_with_spawned_entities(tick, &spawned_entities);
        self.update_teamkills();
        self.settle_shooting(tick);
        self.update_composed(f64::from(now), tick);
        self.update_afterburns(f64::from(now));
        let center_display = self.send_pending_center(tick);
        let mut actions = self
            .take_afterburn_cleanup()
            .into_iter()
            .map(|handle| EngineAction::Afterburn(afterburn::Action::Remove(handle)))
            .collect::<Vec<_>>();
        self.suicides.feedback.expire(now);
        actions.extend(
            self.take_suicide_feedback_cleanup()
                .into_iter()
                .map(|handle| EngineAction::Suicide(suicide::Action::RemoveFeedback(handle))),
        );
        actions.extend(
            self.take_chicken_cleanup()
                .into_iter()
                .map(|handle| EngineAction::Chicken(chickens::Action::Remove(handle))),
        );
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
        self.refresh_movement();
        self.observe_grapple_buttons(tick);
        actions.extend(std::mem::take(&mut self.engine_actions));
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
            self.public_properties.randomize_model_scale(handle, &mut self.rng);
        }
        if let Err(error) = bomb_update {
            self.handle_fault(error);
        }
        actions.extend(center_display);
        actions.extend(self.take_equipment_cleanup().into_iter().map(EngineAction::Equipment));
        actions.extend(std::mem::take(&mut self.impacts).into_iter().map(EngineAction::ProjectileImpact));
        actions.extend(self.bombs.take_actions().into_iter().map(EngineAction::Bomb));
        self.combat.collect_reflections(tick, &mut actions);
        self.update_chicken_digestion(f64::from(now));
        self.combat.collect_delayed_damage(now, &mut actions);
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
            self.respawning.clear_respawns();
            actions.push(EngineAction::EndRound(self.controller.generation(), reason));
        }
        self.collect_respawn_actions(now, &mut actions);
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
        let Some((speed, gravity)) = self
            .controller
            .current_mode()
            .map(|mode| (mode.periodic_speed.clone(), mode.periodic_gravity.clone()))
        else {
            return;
        };
        if speed.is_none() && gravity.is_none() {
            return;
        }
        match funrounds_core::PeriodicMovement::start(s2sdk::GetGameTime(), speed, gravity) {
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
            self.public_properties.discard_map_state();
            self.ordinary_chickens.discard_map_state();
            self.bombs.discard_map_state();
            self.projectiles.discard_map_state();
            self.clear_regions_map();
            self.afterburns.discard_map_state();
            self.composed = composed::State::default();
            self.night_vision.discard_map_state();
            self.ghost.discard_map_state();
            self.suicides.discard_map_state();

            self.refills.discard_map_state();
            self.impacts.clear();
            self.pending_takeovers.clear();
            self.players.clear();
            self.controllers.clear();
            self.discard_grapple_map_state();
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
            self.warmup_round_faulted = false;
        }
        self.sync_warmup(game::warmup(), tick, spawn_slot);
        tick
    }

    fn sync_warmup(&mut self, state: Option<bool>, tick: i32, spawn_slot: Option<i32>) {
        if state != Some(true) {
            if state == Some(false) {
                self.warmup_round_faulted = false;
            }
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

        if self.warmup_round_faulted {
            return;
        }

        if self.controller.phase() == Phase::Idle && self.controller.enabled() && self.effects_ready && tick >= 0 {
            let warmup = self
                .controller
                .config()
                .and_then(|config| config.warmup.as_ref())
                .filter(|warmup| warmup.enabled);
            let wall_hang = warmup.is_some_and(|warmup| warmup.mode.wall_hang);
            let grapple = warmup.is_some_and(|warmup| warmup.mode.grapple.is_some());
            let trampoline = warmup.is_some_and(|warmup| warmup.mode.trampoline);
            if trampoline {
                self.on_region_map_ready();
            }
            if wall_hang {
                self.refresh_wall_hang_capability();
            }
            if grapple {
                self.refresh_grapple_capability();
            }
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

    fn bind_pending_spawn(&mut self, task: PlayerTask, player: LivePlayer) -> Option<PlayerIdentity> {
        let identity = self.players.bind_spawn(task.player, player.pawn_handle as u32)?;
        let base_health = self.chickens.base_health(task).ok();
        self.chickens.clear_life(task);
        if let Some((combat_task, _)) = self.combat.lives.get_mut(&player.slot)
            && *combat_task == task
        {
            combat_task.player = identity;
        }
        if let Some(base_health) = base_health {
            self.chickens.observe_life(PlayerTask { player: identity, ..task }, base_health);
        }
        self.death_explosives.bind_candidate(task.player, identity);
        Some(identity)
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
                let Some(identity) = self.bind_pending_spawn(task, player) else {
                    continue;
                };
                task.player = identity;
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
            if !warmup {
                self.on_region_map_ready();
            }
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
        if matches!(name, "smokegrenade_detonate" | "smokegrenade_expired") {
            self.on_region_event(name, event);
        }
        if name != "item_pickup" {
            self.public_properties
                .refresh_inventory(self.controller.enabled(), &self.players, self.controller.generation(), &mut self.rng);
        }
        match name {
            "item_pickup" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.public_properties
                    .item_pickup(slot, event, &self.players, self.controller.generation());
                self.public_properties
                    .refresh_inventory(self.controller.enabled(), &self.players, self.controller.generation(), &mut self.rng);
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
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.stop_grapple_for_bomb_use(slot, name == "bomb_begindefuse");
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
                        self.handle_fault(Fault::mechanism(Mechanism::BombTiming, error));
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
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.finish_grapple_bomb_use(slot);
                if let Err(error) = self.bombs.abort_plant() {
                    self.handle_fault(Fault::mechanism(Mechanism::BombTiming, error));
                }
            }
            "bomb_abortdefuse" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.finish_grapple_bomb_use(slot);
                self.bombs.abort_quick_defuse();
                if let Err(error) = self.bombs.abort_defuse() {
                    self.handle_fault(Fault::mechanism(Mechanism::BombTiming, error));
                }
            }
            "bomb_defused" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.finish_grapple_bomb_use(slot);
            }
            "bomb_planted" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.finish_grapple_bomb_use(slot);
                if active_combat_phase(self.controller.phase())
                    && let Some(mode) = self.controller.current_mode()
                {
                    let kind = mode.bomb.as_ref().map_or(funrounds_core::BombKind::Native, |settings| settings.kind);
                    let round = self.controller.generation();
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
                        self.start_death_explosives();
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
                if kind == Some(equipment::GrenadeKind::Smoke) {
                    self.record_smoke_throw(slot);
                }
                if matches!(
                    kind,
                    Some(
                        equipment::GrenadeKind::He
                            | equipment::GrenadeKind::Molotov
                            | equipment::GrenadeKind::Incendiary
                            | equipment::GrenadeKind::Decoy
                    )
                ) {
                    self.record_regeneration_combat(slot);
                }
                self.observe_proximity_throw(slot, kind);
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
            "bullet_impact" => self.bullet_impact(event),
            "weapon_fire" => {
                let weapon = s2sdk::GetEventString(event, &Str::from_str("weapon")).to_string();
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                // Empty clicks use weapon_fire_on_empty, not this native event.
                let firearm = crate::effects::firearm_event(&weapon);
                let knife = crate::projectiles::knife_fire_event(&weapon);
                if firearm || knife {
                    self.record_regeneration_combat(slot);
                }
                self.capture_shot(slot, &weapon);
                if knife && active_combat_phase(self.controller.phase()) && self.controller.current_mode().is_some_and(|mode| mode.throwing_knives) {
                    self.combat.native_self_damage_shots.remove(&slot);
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
                if !firearm {
                    self.combat.native_self_damage_shots.remove(&slot);
                    return;
                }
                let Some(player) = LivePlayer::resolve(slot) else {
                    self.combat.native_self_damage_shots.remove(&slot);
                    return;
                };
                self.queue_weapon_on_fire(player, &weapon);
                self.capture_native_self_damage_shot(player, &weapon);
                let Some(mode) = self.controller.current_mode() else { return };
                let identity = self.players.identity(slot);
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
                    && let Some(effect) = self.effects.get_mut(&slot).filter(|effect| {
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
                    self.respawning.queue_finish(task);
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
                let punishment = self.is_teamkill_punishment(slot);
                if !punishment {
                    self.count_teamkill(slot, attacker);
                }
                self.forget_teamkill(slot);
                let chicken_weapon_source = self.take_chicken_blast_kill_weapon(slot, attacker);
                self.bombs.abandon_quick_defuse_player(slot);
                if let Some(identity) = self.players.identity(slot) {
                    self.clear_chicken_life(identity);
                }
                self.ordinary_chickens.cancel_purchase(slot, &mut self.engine_actions);
                self.combat.native_self_damage_shots.remove(&slot);
                self.projectiles
                    .retire_player(slot, self.players.identity(slot).map(|player| player.pawn_handle));
                if self.diagnostics() && self.controller.current_mode().is_some_and(|mode| mode.headshots == Headshots::Only) {
                    log(&format!(
                        "death victim={slot} attacker={attacker} weapon={} headshot={}",
                        s2sdk::GetEventString(event, &Str::from_str("weapon")),
                        s2sdk::GetEventBool(event, &Str::from_str("headshot"))
                    ));
                }
                if !punishment && active_combat_phase(self.controller.phase()) {
                    if self.controller.current_mode().is_some_and(|mode| mode.kill_protection)
                        && let Some(player) = LivePlayer::resolve(attacker)
                        && !s2sdk::IsFakeClient(attacker)
                        && let Some(task) = self.combat_task(player)
                        && let Some((expected, life)) = self.combat.lives.get_mut(&attacker)
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
                if !punishment
                    && self.controller.phase() == Phase::Running
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
                self.track_player_death(slot);
                self.pending_takeovers.retain(|pending| pending.slot != slot);
                self.respawning.cancel_round_respawn(slot);
                self.combat.remove_life(slot);
                self.refills.cancel_player(slot);
                self.retire_effect(slot);
                self.pending.retain(|(task, _, _)| task.player.slot != slot);
                if let Some(respawn) = self.controller.current_mode().and_then(|mode| mode.respawn.as_ref())
                    && respawn.team.matches(s2sdk::GetClientTeam(slot) as i32)
                {
                    self.queue_respawn(slot, respawn.delay_seconds);
                }
            }
            "player_blind" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.queue_long_flash(slot, event);
            }
            "player_hurt" => {
                let victim = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                let attacker = s2sdk::GetEventPlayerSlot(event, &Str::from_str("attacker"));
                let weapon = s2sdk::GetEventString(event, &Str::from_str("weapon")).to_string();
                let health_damage = s2sdk::GetEventInt(event, &Str::from_str("dmg_health"));
                let armor_damage = s2sdk::GetEventInt(event, &Str::from_str("dmg_armor"));
                if (health_damage > 0 || armor_damage > 0)
                    && let Some(player) = LivePlayer::resolve(victim)
                    && self.controllers.get(&victim) == Some(&player.controller_handle)
                    && let Err(error) = self.interrupt_movement_for_impulse(player)
                {
                    self.handle_fault(error.context("received damage interrupted temporary movement"));
                }
                if health_damage > 0 || armor_damage > 0 {
                    self.record_regeneration_combat(victim);
                }
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
                self.death_explosives.forget(slot);
                self.forget_teamkill(slot);
                self.bombs.abandon_quick_defuse_player(slot);
                if let Some(identity) = self.players.identity(slot) {
                    self.clear_chicken_life(identity);
                }
                self.ordinary_chickens.cancel_purchase(slot, &mut self.engine_actions);
                self.combat.forget_player(slot);
                self.pending_takeovers.retain(|pending| pending.slot != slot);
                if let Err(error) = self.suicides.forget(slot) {
                    self.handle_fault(Fault::global(error));
                }
                self.players.death(slot);
                self.refills.cancel_player(slot);
                self.pending.retain(|(task, ..)| task.player.slot != slot);
                self.retire_effect(slot);
                self.respawning.cancel_player_respawns(slot);
            }
            _ => {}
        }
    }

    pub(crate) fn handle_fault(&mut self, fault: Fault) {
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
        if fault.scope != Scope::Round {
            self.prestarted_round = None;
        }
        self.controller.set_capabilities(self.faults.capabilities());
        if fault.scope == Scope::Round {
            self.warmup_round_faulted = self.warmup_active;
            self.warmup_activation_pending = false;
            self.controller.clear_active_round();
        } else if let Some(round) = self.round {
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
            && self.healthshot.settled(self.healthshot_active())
    }

    fn cleanup_complete_for_round_transition_without_bombs(&self) -> bool {
        self.cleanup_complete_for_round_transition_except_prestart_parameters_and_bombs()
            && self.parameters.is_restored()
            && self.healthshot.settled(self.healthshot_active())
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
            && self.grapple.is_clean()
            && self.ground_reverse.is_clean()
            && self.air_strafe.is_clean()
            && self.ghost.is_clean()
            && self.night_vision.is_clean()
            && self.public_properties.cleanup_complete(self.controller.enabled())
            && self.equipment_in_flight == 0
            && self.suicides.cleanup_complete(round_transition, s2sdk::GetGameTime())
            && !self.has_guidance_fuse_recovery()
            && self.ordinary_chickens.cleanup_complete(round_transition)
            && self.composed.is_clean()
            && self.afterburns.is_clean()
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
            log("owned cleanup complete; waiting for a new round with available capabilities");
        }
        self.effects_ready = complete && self.faults.recovery == Recovery::Ready;
    }

    fn finish_round_transition_recovery(&mut self) {
        let complete = self.cleanup_complete_for_round_transition_without_bombs()
            && self.bombs.is_clean()
            && !crate::CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING.load(std::sync::atomic::Ordering::Acquire);
        if self.faults.finish_cleanup(complete) {
            log("owned cleanup complete; waiting for a new round with available capabilities");
        }
        self.effects_ready = complete && self.faults.recovery == Recovery::Ready;
    }

    fn recover_incomplete_effects(&mut self) {
        if !self.effects_ready && self.prestarted_round.is_none() {
            let waiting = self.cleanup_after_round_start_wait();
            self.restore_team_names();
            if !self.controller.enabled() {
                self.public_properties.restore_all();
            }
            if waiting {
                self.finish_round_transition_recovery();
            } else {
                self.finish_recovery();
            }
        }
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
            .filter(|source| chickens::chicken_weapon_source_is_current(source, &self.players, &self.controllers))
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
            self.death_explosives.spawned(slot, identity, &self.players);
            if let Some(previous) = previous {
                self.clear_chicken_life(previous);
            }
            self.ordinary_chickens.forget_input(slot);
            self.healthshot.cancel_purchase(slot, &mut self.engine_actions);
            self.ordinary_chickens.cancel_purchase(slot, &mut self.engine_actions);
            self.respawning.cancel_round_respawn(slot);
            self.combat.cancel_spawn_work(slot);
            self.suicides.discard_life(slot);
            self.refills.cancel_player(slot);
            self.retire_effect(slot);
            self.pending.retain(|(task, _, _)| task.player.slot != slot);
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
                if !self.combat.lives.get(&slot).is_some_and(|(expected, _)| *expected == task) {
                    self.combat
                        .lives
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
                .lives
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

    fn takeover_current(&self, takeover: PendingTakeover) -> bool {
        takeover.round == self.controller.generation()
            && self.controllers.get(&takeover.slot) == Some(&takeover.controller_handle)
            && s2sdk::PlayerSlotToEntHandle(takeover.slot) == takeover.controller_handle
            && self.players.connection_identity(takeover.slot) == Some(takeover.expected)
    }

    fn apply_pending_takeovers(&mut self, tick: i32) {
        let pending = std::mem::take(&mut self.pending_takeovers);
        for takeover in pending {
            if !self.takeover_current(takeover) {
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
            self.cancel_grapple_player(takeover.slot);
            let task = PlayerTask {
                player: identity,
                round: takeover.round,
            };
            let mut owners = self.tracked_pawn_owners(player.pawn_handle as u32);
            owners.sort_unstable();
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

            self.combat.takeover(&owners, task, &self.players);
            let armed = self.death_explosives.takeover(&owners, task, &self.players);

            if self.players.takeover(takeover.expected, player.pawn_handle as u32) != Some(identity) {
                failures.push(Fault::global("player identity changed while applying bot_takeover"));
            } else {
                if armed {
                    self.say(takeover.slot, "death_explosion.armed", &[]);
                }
                self.chickens.takeover_pawn(task);
                self.afterburns.takeover_pawn(task, player);
                self.takeover_composed(task, player);
                self.takeover_trampoline(&owners, task.player);
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
        self.takeover_teamkills(&slots, takeover_task, takeover_player);
        self.takeover_shockwave_targets(&slots, takeover_task);
        self.combat.cancel_actions_for(&slots);
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
        self.impacts
            .retain(|impact| !slots.contains(&impact.task.player.slot) && !slots.contains(&impact.victim_task.player.slot));
        self.respawning.cancel_slots(&slots);
        self.ordinary_chickens.cancel_purchases_for_slots(&slots);
        self.healthshot.cancel_purchases_for_slots(&slots);
        self.engine_actions.retain_mut(|action| match action {
            EngineAction::Composed(action) => !slots.contains(&action.task().player.slot),
            EngineAction::Shooting(action) => !action.involves(&slots),
            EngineAction::Proximity(action) => !action.involves(&slots),
            EngineAction::HeGuidance(action) => !action.involves(&slots),
            EngineAction::HeGuidanceFuseRestore(..) => true,
            EngineAction::Grapple(grapple::Action::Acquire { task, .. } | grapple::Action::Pull { task, .. }) => !slots.contains(&task.player.slot),
            EngineAction::Grapple(grapple::Action::RemoveBeam { .. }) => true,
            EngineAction::Shockwave(action) => !action.involves(&slots),
            EngineAction::Teamkill(action) => !action.involves(&slots),
            EngineAction::Rollback(task, _)
            | EngineAction::Loadout(task, _, _)
            | EngineAction::SettleLoadout(task, _, _)
            | EngineAction::Upgrade(task, _, _)
            | EngineAction::ThirdPerson(task, _) => !slots.contains(&task.player.slot),
            EngineAction::Chicken(action) => !action.involves(&slots),
            EngineAction::Healthshot(action) => !action.involves(&slots),
            EngineAction::Afterburn(action) => !action.involves(&slots),
            EngineAction::Respawn(action) => !action.involves(&slots),
            EngineAction::Suicide(action) => action.retain_for_takeover(&slots, suicide_source, takeover_task, takeover_player),
            EngineAction::Combat(action) => !action.involves(&slots),
            EngineAction::ProjectileImpact(impact) => !slots.contains(&impact.task.player.slot) && !slots.contains(&impact.victim_task.player.slot),
            EngineAction::Projectile(action) => match action.kind {
                crate::projectiles::ActionKind::SpawnKnife { task, .. } => !slots.contains(&task.player.slot),
                crate::projectiles::ActionKind::Remove { .. } => true,
            },
            EngineAction::HostageRescue(action) => !slots.contains(&action.task.player.slot),
            EngineAction::Equipment(equipment::Action::Refill(refill)) => !slots.contains(&refill.task.player.slot),
            EngineAction::Equipment(equipment::Action::Remove { .. })
            | EngineAction::Bomb(_)
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

    fn forget_player(&mut self, slot: i32) {
        self.death_explosives.forget(slot);
        self.clear_region_player_life(slot);
        self.forget_teamkill(slot);
        self.ordinary_chickens.clear_pending_chicken_blast_for_slot(slot);
        if let Some(identity) = self.players.identity(slot) {
            self.clear_composed_life(identity);
            self.afterburns.clear_life(PlayerTask {
                round: self.controller.generation(),
                player: identity,
            });
        }
        self.ordinary_chickens.cancel_purchase(slot, &mut self.engine_actions);
        self.healthshot.cancel_purchase(slot, &mut self.engine_actions);
        self.chickens.clear_slot(slot);
        self.ordinary_chickens.forget_input(slot);
        self.combat.forget_player(slot);
        self.pending_takeovers.retain(|pending| pending.slot != slot);
        if let Err(error) = self.suicides.forget(slot) {
            self.record_fault(Fault::global(error));
        }
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
        self.respawning.cancel_player_respawns(slot);
    }

    fn retire_effect(&mut self, slot: i32) {
        self.cancel_grapple_player(slot);
        if let Err(error) = self.night_vision.remove_player(slot) {
            self.record_fault(Fault::global(error));
        }
        if let Err(error) = self.ghost.remove_player(slot) {
            self.handle_fault(error);
        }
        let has_pending_loadout = self.effects.get(&slot).is_some_and(Effect::has_pending_loadout);
        if has_pending_loadout && self.controller.current_mode().is_some_and(|mode| mode.c4_count.is_some()) {
            self.suicides.c4_assigned = false;
            self.engine_actions
                .retain(|action| !matches!(action, EngineAction::Suicide(suicide::Action::GrantC4(task, _)) if task.player.slot == slot));
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
        self.cancel_all_grapples(grapple::StopReason::RoundEnded);
        self.settling_loadouts.clear();
        if let Err(error) = self.retry_third_person_map_detach() {
            self.record_fault(Fault::global(error));
        }
        self.public_properties.clear_round_inventory();
        if let Err(error) = self.night_vision.cleanup_all() {
            self.record_fault(Fault::global(error));
        }
        if round_transition {
            self.clear_chicken_round_work();
            self.advance_chicken_round(self.controller.generation());
            self.suicides.feedback.expire(s2sdk::GetGameTime());
        } else {
            self.cleanup_ordinary_chickens();
        }
        self.healthshot.clear_round();
        self.clear_regions_round();
        self.cleanup_composed();
        self.shooting = shooting::State::default();
        self.stop_he_guidance_for_cleanup();
        self.teamkills = teamkill::State::default();
        self.afterburns.clear_all();

        if let Err(error) = self.air_strafe.apply(false) {
            self.record_fault(error);
        }
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
            self.suicides.feedback.clear();
        }
        self.combat.clear();
        self.center_display = None;
        self.refills.cancel_all();
        self.impacts.clear();
        self.chicken_impacts.clear();
        self.projectiles.clear();
        self.pending.clear();
        self.team_match = None;
        self.periodic_movement = None;
        self.money_income = None;
        self.suicides.c4_assigned = false;
        self.death_explosives = suicide::DeathExplosions::default();
        self.engine_actions.retain(|action| {
            matches!(action, EngineAction::HeGuidanceFuseRestore(_))
                || (round_end && matches!(action, EngineAction::Suicide(suicide::Action::DeathBlast(_))))
        });
        self.respawning.clear();
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
        self.healthshot.stop_hooks();
        self.ground_reverse.retry_cleanup();
        self.air_strafe.retry_cleanup();
        self.public_properties.restore_all();
        self.welcomes.clear();
        self.bombs.acknowledge_shutdown_removals();
        self.cleanup_effects(false);
        self.restore_team_names();
        if let Err(error) = self.healthshot.restore() {
            self.record_fault(error);
        }
        if self.cleanup_complete() && self.public_properties.is_restored() {
            Ok(())
        } else {
            Err("owned properties or ConVars remain unrestored; snapshots retained for shutdown retry".into())
        }
    }

    fn restore_team_names(&mut self) {
        if let Err(error) = self.team_names.restore() {
            self.record_fault(Fault::global(error));
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
                    "config={}\ncapabilities={:?}\nexcluded={}\ncapability_reasons={}\nhostage_rescue_compat_enabled={}\nhostage_rescue_compat_available={}\nhostage_rescue_compat_reason={}\nhostage_rescue_compat_owner={}",
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
                    "funrounds",
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
                    self.air_strafe.retry_cleanup();
                    self.cleanup_effects(false);
                    self.restore_team_names();
                    self.welcomes.clear();
                    self.hostage_rescue.clear_attempts();
                }
                if enabled && !was_enabled {
                    self.public_properties.restore_all();
                }
                self.controller.set_enabled(enabled);
                if !enabled {
                    self.public_properties.restore_all();
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
                    .lives
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
        if cleanup_pending && faults.recovery == Recovery::Ready {
            Recovery::Recovering
        } else {
            faults.recovery
        },
        faults.reasons(),
    )
}

fn read_config(path: &std::path::Path) -> Result<Config, String> {
    let source = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Config::parse(&source).map_err(|error| format!("{}: {error}", path.display()))
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
        let parts = crate::commands::line_reply_parts(&status).collect::<Vec<_>>();
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
