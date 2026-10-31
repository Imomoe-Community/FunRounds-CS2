// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

mod afterburn;
mod bombs;
mod capability;
mod cash;
mod chicken_adapter;
mod chicken_entities;
mod commands;
mod compat;
mod damage;
mod disconnect;
mod effects;
mod fault;
#[allow(dead_code)]
mod funrounds_native;
mod game;
mod ground_reverse;
mod i18n;
mod legacy_textmsg;
mod night_vision;
mod parameters;
mod preferences;
mod projectiles;
mod random_spawn;
mod runtime;
#[allow(dead_code)]
mod s2sdk;
mod suicide;

mod touch;

use plugify::{Arr, Str, register_plugin};
use s2sdk::{ConCommandContext, ConVarFlag, EventHookError, HookMode, ResultType};
use std::error::Error;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock, TryLockError};
use std::thread::ThreadId;

const EVENTS: &[&str] = &[
    "round_start",
    "round_freeze_end",
    "round_end",
    "player_spawn",
    "player_disconnect",
    "player_death",
    "grenade_thrown",
    "weapon_fire",
    "player_hurt",
    "player_team",
    "player_connect_full",
    "bot_takeover",
    "bomb_beginplant",
    "bomb_abortplant",
    "bomb_planted",
    "bomb_begindefuse",
    "bomb_abortdefuse",
    "bomb_exploded",
    "round_prestart",
    "item_pickup",
];
static ACTIVE: AtomicBool = AtomicBool::new(false);
static PANIC_PENDING: AtomicBool = AtomicBool::new(false);
static PENDING: AtomicBool = AtomicBool::new(false);
static HOOKS: AtomicU32 = AtomicU32::new(0);
static COMMANDS: AtomicU32 = AtomicU32::new(0);
static CHAT_HOOKED: AtomicBool = AtomicBool::new(false);
static CAN_ACQUIRE_HOOKED: AtomicBool = AtomicBool::new(false);
// True while a successful or partial native registration may still need cleanup.
static TOUCH_CLEANUP_PENDING: AtomicBool = AtomicBool::new(false);
static TOUCH_RELEASE_SUPPRESSED: AtomicBool = AtomicBool::new(false);
static KNIFE_DEATH_PRE_HOOKED: AtomicBool = AtomicBool::new(false);
static CHICKEN_DAMAGE_ACTIVE: AtomicBool = AtomicBool::new(false);
static CHICKEN_FOLLOW_BLOCK_ACTIVE: AtomicBool = AtomicBool::new(false);
static CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING: AtomicBool = AtomicBool::new(false);
static CHICKEN_FOLLOW_BLOCK_FAILURE_REPORTED: AtomicBool = AtomicBool::new(false);
static SUICIDE_MANIFEST_CALLBACK_REGISTERED: AtomicBool = AtomicBool::new(false);
static GAME_THREAD_ID: OnceLock<ThreadId> = OnceLock::new();
static HEALTHSHOT_HOOKED: AtomicBool = AtomicBool::new(false);
static USE_TARGET_HOOKED: AtomicBool = AtomicBool::new(false);
static RUNTIME: Mutex<Option<runtime::Runtime>> = Mutex::new(None);
static DISCONNECT_BRIDGE: Mutex<Option<disconnect::Bridge>> = Mutex::new(None);
static DAMAGE_BRIDGE: Mutex<Option<damage::Bridge>> = Mutex::new(None);
static CHICKEN_DAMAGE_BRIDGE: Mutex<Option<damage::EntityBridge>> = Mutex::new(None);

fn callbacks_active() -> bool {
    ACTIVE.load(Ordering::Acquire) && !PANIC_PENDING.load(Ordering::Acquire)
}

fn missing_native_abi() -> Option<String> {
    let mut missing = Vec::new();
    macro_rules! require_import {
        ($name:ident) => {
            if unsafe { std::ptr::addr_of!(funrounds_native::$name).read() }.is_none() {
                missing.push(stringify!($name).trim_start_matches("__funrounds_native_"));
            }
        };
    }
    require_import!(__funrounds_native_HasPlayerTakeDamagePre);
    require_import!(__funrounds_native_HookPlayerTakeDamagePre);
    require_import!(__funrounds_native_UnhookPlayerTakeDamagePre);
    require_import!(__funrounds_native_GetCurrentDamageFrameToken);
    require_import!(__funrounds_native_TakeEntityDamageWithWeapon);
    require_import!(__funrounds_native_IsEntityTouchPreHookAvailable);
    require_import!(__funrounds_native_HookEntityTouchPre);
    require_import!(__funrounds_native_UnhookEntityTouchPre);
    require_import!(__funrounds_native_SetEntityDefaultCollisionGroup);
    if unsafe { std::ptr::addr_of!(effects::__s2sdk_GetWeaponItemDefinition).read() }.is_none() {
        missing.push("GetWeaponItemDefinition");
    }
    if unsafe { std::ptr::addr_of!(disconnect::__s2sdk_OnClientDisconnect_Register).read() }.is_none() {
        missing.push("OnClientDisconnect_Register");
    }
    if unsafe { std::ptr::addr_of!(disconnect::__s2sdk_OnClientDisconnect_Unregister).read() }.is_none() {
        missing.push("OnClientDisconnect_Unregister");
    }
    (!missing.is_empty()).then(|| {
        format!(
            "FunRounds requires funrounds_native 0.3-Alpha Test 2 ABI; missing imports: {}",
            missing.join(", ")
        )
    })
}

fn healthshot_native_available() -> bool {
    let (support, hook, unhook, find_convar, get_convar, set_convar, get_health, set_health) = unsafe {
        (
            std::ptr::addr_of!(funrounds_native::__funrounds_native_HasHealthshotSupport).read(),
            std::ptr::addr_of!(funrounds_native::__funrounds_native_HookHealthshot).read(),
            std::ptr::addr_of!(funrounds_native::__funrounds_native_UnhookHealthshot).read(),
            std::ptr::addr_of!(s2sdk::__s2sdk_FindConVar).read(),
            std::ptr::addr_of!(s2sdk::__s2sdk_GetConVarValue).read(),
            std::ptr::addr_of!(s2sdk::__s2sdk_SetConVarValue).read(),
            std::ptr::addr_of!(s2sdk::__s2sdk_GetClientHealth).read(),
            std::ptr::addr_of!(s2sdk::__s2sdk_SetClientHealth).read(),
        )
    };
    if support.is_none()
        || hook.is_none()
        || unhook.is_none()
        || find_convar.is_none()
        || get_convar.is_none()
        || set_convar.is_none()
        || get_health.is_none()
        || set_health.is_none()
    {
        return false;
    }
    parameters::has_convar("healthshot_health") && parameters::has_convar("healthshot_allow_use_at_full") && funrounds_native::HasHealthshotSupport()
}

fn use_target_native_available() -> bool {
    let (support, hook, unhook) = unsafe {
        (
            std::ptr::addr_of!(funrounds_native::__funrounds_native_HasUseTargetSupport).read(),
            std::ptr::addr_of!(funrounds_native::__funrounds_native_HookUseTarget).read(),
            std::ptr::addr_of!(funrounds_native::__funrounds_native_UnhookUseTarget).read(),
        )
    };
    support.is_some() && hook.is_some() && unhook.is_some() && funrounds_native::HasUseTargetSupport()
}

fn chicken_follow_block_native_available() -> bool {
    let (support, set) = unsafe {
        (
            std::ptr::addr_of!(funrounds_native::__funrounds_native_HasChickenFollowBlockSupport).read(),
            std::ptr::addr_of!(funrounds_native::__funrounds_native_SetChickenFollowBlocking).read(),
        )
    };
    support.is_some() && set.is_some() && funrounds_native::HasChickenFollowBlockSupport()
}

fn set_chicken_follow_blocking(active: bool) -> bool {
    let set = unsafe { std::ptr::addr_of!(funrounds_native::__funrounds_native_SetChickenFollowBlocking).read() };
    set.is_some_and(|set| unsafe { set(active) })
}

fn disable_chicken_follow_block(force: bool) -> bool {
    let active = CHICKEN_FOLLOW_BLOCK_ACTIVE.load(Ordering::Acquire);
    let pending = CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING.load(Ordering::Acquire);
    if !force && !active && !pending {
        CHICKEN_FOLLOW_BLOCK_FAILURE_REPORTED.store(false, Ordering::Release);
        return true;
    }
    if set_chicken_follow_blocking(false) {
        CHICKEN_FOLLOW_BLOCK_ACTIVE.store(false, Ordering::Release);
        CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING.store(false, Ordering::Release);
        CHICKEN_FOLLOW_BLOCK_FAILURE_REPORTED.store(false, Ordering::Release);
        true
    } else {
        CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING.store(true, Ordering::Release);
        false
    }
}

fn sync_chicken_follow_blocking(runtime: &mut runtime::Runtime) {
    let desired = runtime.chicken_follow_block_desired();
    if !desired {
        if !disable_chicken_follow_block(false) && !CHICKEN_FOLLOW_BLOCK_FAILURE_REPORTED.swap(true, Ordering::AcqRel) {
            runtime.handle_fault(fault::Fault::global(
                "native chicken-follow detour cleanup is pending; FunRounds remains stopped until detach is confirmed",
            ));
        }
        return;
    }
    if !runtime.chicken_follow_block_supported() {
        // The configured capability is already excluded during Runtime construction.
        return;
    }
    if CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING.load(Ordering::Acquire) {
        if !disable_chicken_follow_block(true) && !CHICKEN_FOLLOW_BLOCK_FAILURE_REPORTED.swap(true, Ordering::AcqRel) {
            runtime.handle_fault(fault::Fault::global(
                "native chicken-follow detour cleanup is pending; FunRounds remains stopped until detach is confirmed",
            ));
        }
        return;
    }
    if CHICKEN_FOLLOW_BLOCK_ACTIVE.load(Ordering::Acquire) || CHICKEN_FOLLOW_BLOCK_FAILURE_REPORTED.load(Ordering::Acquire) {
        return;
    }
    if set_chicken_follow_blocking(true) {
        CHICKEN_FOLLOW_BLOCK_ACTIVE.store(true, Ordering::Release);
        return;
    }
    if disable_chicken_follow_block(true) {
        if !CHICKEN_FOLLOW_BLOCK_FAILURE_REPORTED.swap(true, Ordering::AcqRel) {
            runtime.handle_fault(fault::Fault::mechanism(
                fault::Mechanism::ChickenFollowBlocking,
                "native chicken-follow detour setup failed; ordinary chicken presets are unavailable",
            ));
        }
    } else if !CHICKEN_FOLLOW_BLOCK_FAILURE_REPORTED.swap(true, Ordering::AcqRel) {
        runtime.handle_fault(fault::Fault::global(
            "native chicken-follow detour setup failed and owned-hook cleanup is not confirmed",
        ));
    }
}

fn contain_failure(context: &str) {
    // A synchronous callback may be unwinding while another frame still holds RUNTIME.
    // Publish the stop without locking; the next game update owns restoration.
    PANIC_PENDING.store(true, Ordering::Release);
    let _ = writeln!(std::io::stderr().lock(), "[FunRounds] {context}; global stop queued");
}

fn apply_pending_fault() -> Result<(), Box<dyn Error>> {
    if !PANIC_PENDING.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut guard = match RUNTIME.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return Err("FunRounds: runtime busy while applying global stop".into()),
    };
    if let Some(runtime) = guard.as_ref() {
        assert!(runtime.on_game_thread(), "FunRounds recovery is not on the game thread");
    }
    PANIC_PENDING.store(false, Ordering::Release);
    RUNTIME.clear_poison();
    if let Some(runtime) = guard.as_mut() {
        runtime.handle_fault(fault::Fault::global("callback failure or panic contained at ABI boundary"));
    }
    Ok(())
}

fn boundary(action: impl FnOnce() -> Result<(), Box<dyn Error>>) -> Result<(), Box<dyn Error>> {
    catch_unwind(AssertUnwindSafe(action)).unwrap_or_else(|_| {
        contain_failure("panic contained at plugin boundary");
        Err("FunRounds: panic contained at plugin boundary".into())
    })
}

fn assert_game_thread() {
    let current = std::thread::current().id();
    if let Some(expected) = GAME_THREAD_ID.get() {
        assert_eq!(expected, &current, "FunRounds engine callback is not on the game thread");
    } else if let Err(current) = GAME_THREAD_ID.set(current) {
        assert_eq!(GAME_THREAD_ID.get(), Some(&current), "FunRounds engine callback thread changed");
    }
}

fn register_suicide_manifest_callback() -> Result<(), Box<dyn Error>> {
    if !suicide::resource_probe().available() {
        s2sdk::PrintToServer(&Str::from_str(
            "[FunRounds] suicide resource imports unavailable; dependent modes are excluded",
        ));
        return Ok(());
    }
    suicide::precache_feedback_resource();
    s2sdk::OnBuildGameSessionManifest_Register(on_build_game_session_manifest);
    SUICIDE_MANIFEST_CALLBACK_REGISTERED.store(true, Ordering::Release);
    s2sdk::PrintToServer(&Str::from_str("[FunRounds] suicide particle precache and manifest callback ready"));
    Ok(())
}

fn unregister_suicide_manifest_callback() -> Result<(), Box<dyn Error>> {
    if !SUICIDE_MANIFEST_CALLBACK_REGISTERED.load(Ordering::Acquire) {
        return Ok(());
    }
    let unregister = unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_OnBuildGameSessionManifest_Unregister).read() }
        .ok_or("FunRounds: S2SDK manifest listener unregistration import is unavailable")?;
    unsafe { unregister(on_build_game_session_manifest) };
    SUICIDE_MANIFEST_CALLBACK_REGISTERED.store(false, Ordering::Release);
    Ok(())
}

unsafe extern "C" fn on_build_game_session_manifest() {
    if catch_unwind(AssertUnwindSafe(|| {
        if !SUICIDE_MANIFEST_CALLBACK_REGISTERED.load(Ordering::Acquire) {
            return;
        }
        assert_game_thread();
        suicide::precache_feedback_resource();
        s2sdk::PrintToServer(&Str::from_str("[FunRounds] suicide particle registered during BuildGameSessionManifest"));
    }))
    .is_err()
    {
        contain_failure("suicide resource manifest callback panic contained");
    }
}

fn start() -> Result<(), Box<dyn Error>> {
    boundary(|| {
        if HOOKS.load(Ordering::Acquire) != 0
            || COMMANDS.load(Ordering::Acquire) != 0
            || CHAT_HOOKED.load(Ordering::Acquire)
            || SUICIDE_MANIFEST_CALLBACK_REGISTERED.load(Ordering::Acquire)
            || legacy_textmsg::hooked()
            || TOUCH_CLEANUP_PENDING.load(Ordering::Acquire)
            || CHICKEN_FOLLOW_BLOCK_ACTIVE.load(Ordering::Acquire)
            || CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING.load(Ordering::Acquire)
            || KNIFE_DEATH_PRE_HOOKED.load(Ordering::Acquire)
            || CHICKEN_DAMAGE_BRIDGE
                .lock()
                .map_err(|_| "FunRounds: chicken damage bridge poisoned")?
                .is_some()
            || DISCONNECT_BRIDGE.lock().map_err(|_| "FunRounds: disconnect bridge poisoned")?.is_some()
            || DAMAGE_BRIDGE.lock().map_err(|_| "FunRounds: damage bridge poisoned")?.is_some()
            || RUNTIME.lock().map_err(|_| "FunRounds: runtime poisoned")?.is_some()
        {
            return Err("FunRounds: previous callbacks or owned-state cleanup remain".into());
        }
        if let Some(error) = missing_native_abi() {
            return Err(error.into());
        }
        PANIC_PENDING.store(false, Ordering::Release);
        s2sdk::PrintToServer(&Str::from_str("[FunRounds] waiting for first game simulation before subscribing"));
        register_suicide_manifest_callback()?;
        PENDING.store(true, Ordering::Release);
        Ok(())
    })
}

fn update(_dt: f32) -> Result<(), Box<dyn Error>> {
    boundary(|| {
        assert_game_thread();
        if PENDING.swap(false, Ordering::AcqRel) {
            subscribe()?;
        }
        if !ACTIVE.load(Ordering::Acquire) && CHICKEN_FOLLOW_BLOCK_CLEANUP_PENDING.load(Ordering::Acquire) {
            let _ = disable_chicken_follow_block(true);
        }
        if ACTIVE.load(Ordering::Acquire) {
            apply_pending_fault()?;
            apply_pending_bomb_explosion();
            let mut guard = RUNTIME.try_lock().map_err(|_| "FunRounds: runtime busy or poisoned")?;
            if let Some(runtime) = guard.as_mut() {
                if !runtime.on_game_thread() {
                    return Err("FunRounds: update is not on the game thread".into());
                }
                let actions = runtime.update();
                sync_chicken_follow_blocking(runtime);
                drop(guard);
                for action in actions {
                    match action {
                        action @ runtime::EngineAction::Rollback(..) => process_rollback_action(action),
                        action @ runtime::EngineAction::SettleLoadout(..) => process_loadout_settlement_action(action),
                        action @ (runtime::EngineAction::Loadout(..)
                        | runtime::EngineAction::Upgrade(..)
                        | runtime::EngineAction::ThirdPerson(..)) => process_loadout_action(action),
                        runtime::EngineAction::GrantC4(task, player) => {
                            let deferred = if callbacks_active() {
                                let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                guard
                                    .as_mut()
                                    .filter(|runtime| runtime.on_game_thread())
                                    .is_some_and(|runtime| runtime.defer_pending_loadout_c4_grant(task, player))
                            } else {
                                false
                            };
                            if deferred {
                                continue;
                            }
                            let action = runtime::EngineAction::GrantC4(task, player);
                            let result = suicide::grant_c4(player, || engine_action_current(&action));
                            if let Err(error) = result
                                && engine_action_current(&action)
                            {
                                equipment_fault(error);
                            }
                        }
                        runtime::EngineAction::RemoveC4(task, player, handles) => {
                            execute_valid_action(runtime::EngineAction::RemoveC4(task, player, handles.clone()), || {
                                runtime::remove_native_c4(player, &handles)
                            })?;
                        }
                        runtime::EngineAction::Suicide(action) => process_suicide_action(action),
                        runtime::EngineAction::Bomb(bomb) => process_bomb_action(bomb),
                        runtime::EngineAction::ChickenSpawn { round, origin } => process_chicken_spawn(round, origin),
                        runtime::EngineAction::ChickenBlast {
                            round,
                            chicken,
                            attacker,
                            origin,
                            config,
                        } => process_chicken_blast(round, chicken, attacker, origin, config),
                        runtime::EngineAction::ChickenRemove(handle) => chicken_entities::remove(handle),
                        runtime::EngineAction::SuicideFeedbackRemove(handle) => suicide::remove_feedback(handle),
                        runtime::EngineAction::AfterburnRemove(handle) => afterburn::remove_flame(handle),
                        runtime::EngineAction::AfterburnIgnite { task, player } => process_afterburn_ignite(task, player),
                        runtime::EngineAction::ChickenEat {
                            task,
                            player,
                            chicken,
                            outcome,
                        } => process_chicken_eat(task, player, chicken, outcome),
                        runtime::EngineAction::ChickenPurchase { task, player, price, origin } => {
                            process_chicken_purchase(task, player, price, origin)
                        }
                        runtime::EngineAction::HealthshotUse {
                            task,
                            player,
                            base_health,
                            config,
                        } => {
                            let action = runtime::EngineAction::HealthshotUse {
                                task,
                                player,
                                base_health,
                                config,
                            };
                            execute_valid_action(action, || {
                                let current = s2sdk::GetClientHealth(task.player.slot);
                                if let funrounds_core::HealthshotOutcome::Applied { health, .. } =
                                    funrounds_core::use_healthshot(config, current, base_health)
                                {
                                    s2sdk::SetClientHealth(task.player.slot, health);
                                    if s2sdk::GetClientHealth(task.player.slot) != health {
                                        return Err(fault::Fault::mechanism(
                                            fault::Mechanism::Healthshot,
                                            format!("healthshot health write failed for slot {}", task.player.slot),
                                        ));
                                    }
                                }
                                Ok(())
                            })?;
                        }
                        runtime::EngineAction::HealthshotPurchase { task, player, price } => process_healthshot_purchase(task, player, price),
                        runtime::EngineAction::Equipment(action) => process_equipment_action(action),
                        runtime::EngineAction::Projectile(action) => process_projectile_action(action),
                        runtime::EngineAction::DelayedDamage(hit) => process_delayed_damage(hit),
                        runtime::EngineAction::Reflection(hit) => process_reflection(hit),
                        runtime::EngineAction::ProjectileImpact(impact) => {
                            execute_valid_action(runtime::EngineAction::ProjectileImpact(impact), || {
                                apply_projectile_impact(impact);
                                Ok(())
                            })?
                        }
                        runtime::EngineAction::Respawn(task, player, team) => {
                            execute_valid_action(runtime::EngineAction::Respawn(task, player, team), || {
                                s2sdk::RespawnClient(task.player.slot);
                                Ok(())
                            })?
                        }
                        runtime::EngineAction::HostageRescue(action) => process_hostage_rescue(action),
                        runtime::EngineAction::RoundRespawn(task, player, team) => {
                            execute_valid_action(runtime::EngineAction::RoundRespawn(task, player, team), || {
                                s2sdk::ForceRespawnClient(task.player.slot);
                                Ok(())
                            })?
                        }
                        runtime::EngineAction::LowHealthFinish(task, player, numeric) => {
                            execute_valid_action(runtime::EngineAction::LowHealthFinish(task, player, numeric), || {
                                let slot = task.player.slot;
                                let handle = player.pawn_handle;
                                let zero = plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 };
                                let applied = if numeric {
                                    damage::finish_self_damage(handle)?
                                } else {
                                    damage::apply_native_damage(
                                        damage::NativeDamageCall {
                                            victim: handle,
                                            inflictor: handle,
                                            attacker: handle,
                                            force: &zero,
                                            position: &zero,
                                            amount: 1.0,
                                            damage_type: damage::DMG_HEADSHOT,
                                            weapon: -1,
                                            skip_plugin_rules: false,
                                        },
                                        fault::Mechanism::NativeSelfDamage,
                                        "native low-health finish rejected after identity validation",
                                    )?
                                };
                                s2sdk::PrintToServer(&Str::from_str(&format!(
                                    "[FunRounds] low-health finish slot={slot} pawn={handle} damage={applied}"
                                )));
                                Ok(())
                            })?
                        }
                        runtime::EngineAction::CenterDisplay { generation } => {
                            execute_valid_action(runtime::EngineAction::CenterDisplay { generation }, || {
                                let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                if let Some(runtime) = guard.as_ref() {
                                    runtime.center();
                                }
                                Ok(())
                            })?
                        }
                        runtime::EngineAction::EndRound(round, reason) => {
                            execute_valid_action(runtime::EngineAction::EndRound(round, reason), || {
                                finish_team_round(reason);
                                Ok(())
                            })?
                        }
                    }
                }
                let rescue_active = RUNTIME
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .as_ref()
                    .is_some_and(runtime::Runtime::hostage_rescue_touch_enabled);
                if !rescue_active {
                    touch::clear_rescue_zones();
                }
                release_touch_hook_if_unused();
            }
        }
        Ok(())
    })
}

fn execute_valid_action(action: runtime::EngineAction, execute: impl FnOnce() -> Result<(), fault::Fault>) -> Result<(), Box<dyn Error>> {
    let guard = RUNTIME.try_lock().map_err(|_| "FunRounds: runtime busy or poisoned")?;
    let valid = callbacks_active() && guard.as_ref().is_some_and(|runtime| runtime.action_valid(&action));
    drop(guard);
    if valid && let Err(error) = execute() {
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(runtime) = guard.as_mut() {
            runtime.handle_fault(error);
        }
    }
    Ok(())
}

fn engine_action_current(action: &runtime::EngineAction) -> bool {
    let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    (callbacks_active() || matches!(action, runtime::EngineAction::Rollback(..)))
        && guard
            .as_ref()
            .is_some_and(|runtime| runtime.on_game_thread() && runtime.action_valid(action))
}

fn process_rollback_action(action: runtime::EngineAction) {
    let effect = {
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(runtime) = guard.as_mut().filter(|runtime| runtime.on_game_thread()) else {
            return;
        };
        runtime.take_rollback_effect(&action)
    };
    let Some(mut effect) = effect else { return };
    let result = catch_unwind(AssertUnwindSafe(|| effect.restore_previous(|| engine_action_current(&action))))
        .unwrap_or_else(|_| Err(fault::Fault::global("equipment rollback panic contained")));
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.finish_equipment_effect(&action, effect, result);
    }
}

fn process_loadout_settlement_action(action: runtime::EngineAction) {
    let runtime::EngineAction::SettleLoadout(_, _, handle) = &action else {
        return;
    };
    let effect = {
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(runtime) = guard.as_mut().filter(|runtime| runtime.on_game_thread() && callbacks_active()) else {
            return;
        };
        runtime.take_settle_loadout_effect(&action)
    };
    let Some(mut effect) = effect else { return };
    let handle = *handle;
    let result = catch_unwind(AssertUnwindSafe(|| {
        effect.settle_pending_loadout(handle, || engine_action_current(&action))
    }))
    .unwrap_or_else(|_| Err(fault::Fault::global("deferred equipment settlement panic contained")));
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.finish_equipment_effect(&action, effect, result);
    }
}

fn equipment_fault(error: String) {
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.handle_fault(fault::Fault::mechanism(fault::Mechanism::C4Assignment, error));
    }
}

fn process_loadout_action(action: runtime::EngineAction) {
    let prepared = {
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(runtime) = guard.as_mut().filter(|runtime| runtime.on_game_thread() && callbacks_active()) else {
            return;
        };
        match runtime.take_equipment_effect(&action) {
            Ok(prepared) => prepared,
            Err(error) => {
                runtime.handle_fault(error);
                return;
            }
        }
    };
    let Some(runtime::PreparedEquipmentEffect { mut effect, mode, upgrade }) = prepared else {
        return;
    };
    let result = catch_unwind(AssertUnwindSafe(|| match &action {
        runtime::EngineAction::Loadout(_, player, equipment) => {
            random_spawn::apply(*player, &mode, || engine_action_current(&action))?;
            effect.apply(&mode, equipment, || engine_action_current(&action))
        }
        runtime::EngineAction::ThirdPerson(..) => effect.update_third_person(&mode, || engine_action_current(&action)),
        runtime::EngineAction::Upgrade(..) => {
            let upgrade = upgrade.as_ref().expect("upgrade action is prepared with its final target and candidate");
            effect
                .replace_upgrade_weapon(upgrade.target, std::slice::from_ref(&upgrade.weapon), &mode, || {
                    engine_action_current(&action)
                })
                .map(|_| ())
        }
        _ => unreachable!("equipment action"),
    }))
    .unwrap_or_else(|_| Err(fault::Fault::global("equipment transaction panic contained")));
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.finish_equipment_effect(&action, effect, result);
    }
}

unsafe extern "C" fn on_can_acquire(pawn: i32, controller: i32) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() {
            return 0;
        }
        let Ok(guard) = RUNTIME.try_lock() else { return 0 };
        guard
            .as_ref()
            .filter(|runtime| runtime.on_game_thread())
            .is_some_and(|runtime| runtime.blocks_suicide_pickup(pawn, controller)) as i32
    }))
    .unwrap_or_else(|_| {
        contain_failure("suicide pickup callback panic contained");
        1
    })
}

fn suicide_action_current(action: suicide::Action) -> bool {
    let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    callbacks_active()
        && guard
            .as_ref()
            .is_some_and(|runtime| runtime.on_game_thread() && runtime.action_valid(&runtime::EngineAction::Suicide(action)))
}

fn process_suicide_action(action: suicide::Action) {
    {
        let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !callbacks_active()
            || !guard
                .as_ref()
                .is_some_and(|runtime| runtime.on_game_thread() && runtime.action_valid(&runtime::EngineAction::Suicide(action)))
        {
            return;
        }
    }
    match action {
        suicide::Action::Purchase { player, price, .. } => {
            let existing = {
                let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.as_ref().and_then(|runtime| runtime.existing_suicide_c4(player))
            };
            let result = existing.map(Ok).unwrap_or_else(|| {
                suicide::purchase(
                    player,
                    price,
                    || suicide_action_current(action),
                    || {
                        let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        guard.as_ref().is_some_and(|runtime| runtime.suicide_buyer_current(action))
                    },
                )
            });
            let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(runtime) = guard.as_mut() {
                runtime.finish_suicide_purchase(action, result);
            }
        }
        suicide::Action::Start { owned, .. } => {
            if let Err(error) = owned.start(|| suicide_action_current(action)) {
                if suicide_action_current(action) {
                    suicide_fault(error);
                }
            } else {
                let is_terrorist = s2sdk::GetClientTeam(owned.player.slot) as i32 == 2;
                let sound_error = is_terrorist.then(|| suicide::play_start_sound(owned.player).err()).flatten();
                let key = if is_terrorist {
                    "suicide.start_broadcast"
                } else {
                    "suicide.ct_broadcast"
                };
                let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(runtime) = guard.as_ref() {
                    runtime.broadcast(key, &[]);
                }
                drop(guard);
                if let Some(error) = sound_error {
                    suicide_fault(error);
                }
            }
        }
        suicide::Action::Blast { .. } => {
            let targets = {
                let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let Some(runtime) = guard.as_mut() else { return };
                match runtime.prepare_suicide_blast(action) {
                    Ok(targets) => targets,
                    Err(error) => {
                        runtime.handle_fault(error);
                        return;
                    }
                }
            };
            let mut feedback_errors = Vec::new();
            let mut blast_sound_source = action.player().pawn_handle;
            match suicide::spawn_feedback(action.player()) {
                Ok(handle) => {
                    let now = s2sdk::GetGameTime();
                    let tracked = RUNTIME
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .as_mut()
                        .is_some_and(|runtime| runtime.track_suicide_feedback(handle, now));
                    if tracked {
                        blast_sound_source = handle;
                    } else {
                        suicide::remove_feedback(handle);
                        feedback_errors.push("自爆粒子反馈无法登记清理".to_owned());
                    }
                }
                Err(error) => feedback_errors.push(error),
            }
            if let Err(error) = suicide::play_explosion_sound(blast_sound_source) {
                feedback_errors.push(error);
            }
            for (task, player, amount) in targets {
                let valid = {
                    let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.as_ref().is_some_and(|runtime| {
                        runtime.suicide_target_valid(task, player) && runtime.action_valid(&runtime::EngineAction::Suicide(action))
                    })
                };
                if valid && let Err(error) = suicide::hit(action.player(), player, amount, false) {
                    suicide_fault(error);
                    break;
                }
            }
            let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(runtime) = guard.as_mut() {
                runtime.finish_suicide_blast();
            }
            drop(guard);
            if !feedback_errors.is_empty() {
                suicide_fault(feedback_errors.join("; "));
            }
        }
        suicide::Action::Finish { player, .. } => {
            let amount = {
                let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if !callbacks_active() {
                    return;
                }
                guard
                    .as_ref()
                    .filter(|runtime| runtime.on_game_thread())
                    .and_then(|runtime| runtime.prepare_suicide_finish(action))
            };
            let Some(amount) = amount else { return };
            if let Err(error) = suicide::hit(player, player, amount, true) {
                suicide_fault(error);
            }
        }
    }
}

fn suicide_fault(error: String) {
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.handle_fault(fault::Fault::mechanism(fault::Mechanism::Suicide, error));
    }
}

fn process_delayed_damage(hit: damage::DelayedDamage) {
    let Some((inflictor, weapon)) = hit.native_handles(s2sdk::IsValidEntHandle) else {
        return;
    };
    let context = damage::Damage {
        victim: hit.target_live.pawn_handle,
        attacker: hit.source_live.pawn_handle,
        inflictor,
        weapon_kind: 0,
        hit_group: 0,
        traced_head: false,
        amount: hit.amount,
        damage_type: hit.damage_type,
        weapon,
        weapon_class: hit.weapon_class.clone(),
        skip_plugin_rules: false,
    };
    let players = game::DamagePlayers {
        victim: Some(hit.target_live),
        attacker: Some(hit.source_live),
    };
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(runtime) = guard.as_mut() else { return };
    if !callbacks_active() || !runtime.action_valid(&runtime::EngineAction::DelayedDamage(hit.clone())) {
        return;
    }
    let blocked = runtime.combat_damage(&context, &players, 1.0, true);
    drop(guard);
    if blocked {
        return;
    }
    if let Err(error) = hit.execute(s2sdk::IsValidEntHandle) {
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(runtime) = guard.as_mut() {
            runtime.handle_fault(error);
        }
    }
}

fn process_reflection(hit: damage::Reflection) {
    let Some((inflictor, weapon)) = hit.native_handles(s2sdk::IsValidEntHandle) else {
        return;
    };
    let context = damage::Damage {
        victim: hit.target_live.pawn_handle,
        attacker: hit.source_live.pawn_handle,
        inflictor,
        weapon_kind: 0,
        hit_group: 0,
        traced_head: false,
        amount: hit.amount,
        damage_type: hit.damage_type,
        weapon,
        weapon_class: hit.weapon_class.clone(),
        skip_plugin_rules: false,
    };
    let players = game::DamagePlayers {
        victim: Some(hit.target_live),
        attacker: Some(hit.source_live),
    };
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(runtime) = guard.as_mut() else { return };
    let action = runtime::EngineAction::Reflection(hit.clone());
    if !callbacks_active() || !runtime.action_valid(&action) {
        return;
    }
    let blocked = runtime.combat_damage(&context, &players, 1.0, false);
    drop(guard);
    if !blocked && let Err(error) = hit.execute(s2sdk::IsValidEntHandle) {
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(runtime) = guard.as_mut() {
            runtime.handle_fault(error);
        }
    }
}

fn process_bomb_action(action: bombs::Action) {
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(runtime) = guard.as_mut() else { return };
    if (!callbacks_active() && !action.is_cleanup()) || !runtime.action_valid(&runtime::EngineAction::Bomb(action)) {
        runtime.cancel_bomb_action(action);
        return;
    }
    drop(guard);

    let result = bombs::execute(action);
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.finish_bomb_action(action, result);
    }
}

fn process_chicken_spawn(round: funrounds_core::RoundGeneration, origin: plugify::Vec3) {
    let action = runtime::EngineAction::ChickenSpawn { round, origin };
    let current = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .is_some_and(|runtime| callbacks_active() && runtime.on_game_thread() && runtime.action_valid(&action));
    if !current {
        return;
    }
    let handle = match chicken_entities::spawn(origin) {
        Ok(handle) => handle,
        Err(error) => {
            s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] ordinary chicken spawn failed: {error}")));
            return;
        }
    };
    let tracked = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
        .is_some_and(|runtime| runtime.on_game_thread() && runtime.track_chicken(round, handle));
    if tracked {
        s2sdk::PrintToServer(&Str::from_str(&format!(
            "[FunRounds] ordinary chicken spawned round={round:?} handle={handle}"
        )));
    } else {
        chicken_entities::remove(handle);
    }
}

fn chicken_purchase_action(task: funrounds_core::PlayerTask, player: game::LivePlayer, price: u32, origin: plugify::Vec3) -> runtime::EngineAction {
    runtime::EngineAction::ChickenPurchase { task, player, price, origin }
}

fn chicken_buyer_identity_current(task: funrounds_core::PlayerTask, player: game::LivePlayer) -> bool {
    let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    callbacks_active() && guard.as_ref().is_some_and(|runtime| runtime.chicken_buyer_identity_current(task, player))
}

fn process_chicken_purchase(task: funrounds_core::PlayerTask, player: game::LivePlayer, price: u32, origin: plugify::Vec3) {
    let action = chicken_purchase_action(task, player, price, origin);
    let result = if !engine_action_current(&action) {
        Err(cash::PurchaseError::Rejected("purchase.cancelled"))
    } else {
        crate::cash::purchase(
            player,
            price,
            cash::PurchaseItem::Chicken,
            || chicken_buyer_identity_current(task, player),
            || {
                if !engine_action_current(&action) {
                    return Err("chicken purchase cancelled".to_owned());
                }
                let handle = chicken_entities::spawn(origin)?;
                let tracked = RUNTIME
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .as_mut()
                    .is_some_and(|runtime| {
                        callbacks_active() && runtime.on_game_thread() && runtime.action_valid(&action) && runtime.track_chicken(task.round, handle)
                    });
                if !tracked {
                    chicken_entities::remove(handle);
                    return Err("chicken purchase invalidated after creation".to_owned());
                }
                Ok(handle)
            },
        )
    };
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.finish_chicken_purchase(task);
    }
    drop(guard);
    if callbacks_active() && game::LivePlayer::resolve_present(player.slot) == Some(player) {
        let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(runtime) = guard.as_ref() {
            let message = match result {
                Ok(_) => i18n::Message::new("chicken.purchased"),
                Err(error) => runtime.purchase_error(player.slot, error),
            };
            i18n::send_chat(player.slot, &runtime.message(player.slot, message));
        }
    }
}

fn healthshot_buyer_current(task: funrounds_core::PlayerTask, player: game::LivePlayer) -> bool {
    let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    callbacks_active() && guard.as_ref().is_some_and(|runtime| runtime.healthshot_buyer_current(task, player))
}

fn process_healthshot_purchase(task: funrounds_core::PlayerTask, player: game::LivePlayer, price: u32) {
    let action = runtime::EngineAction::HealthshotPurchase { task, player, price };
    let result = if !engine_action_current(&action) {
        Err(cash::PurchaseError::Rejected("purchase.cancelled"))
    } else {
        crate::cash::purchase(
            player,
            price,
            cash::PurchaseItem::Healthshot,
            || healthshot_buyer_current(task, player),
            || {
                if !engine_action_current(&action) {
                    return Err("healthshot purchase cancelled".to_owned());
                }
                let before = s2sdk::GetClientWeapons(player.slot).to_vec();
                let handle = s2sdk::GiveNamedItem(player.slot, &Str::from_str("weapon_healthshot"));
                let created = s2sdk::IsValidEntHandle(handle) && !before.contains(&handle);
                let delivered = healthshot_buyer_current(task, player)
                    && created
                    && s2sdk::GetEntityClassname(handle).as_str() == "weapon_healthshot"
                    && s2sdk::GetEntityOwner(handle) == player.pawn_handle
                    && s2sdk::GetClientWeapons(player.slot).iter().any(|owned| *owned == handle);
                if delivered {
                    return Ok(handle);
                }
                if created && s2sdk::IsValidEntHandle(handle) {
                    if s2sdk::GetEntityOwner(handle) == player.pawn_handle
                        && s2sdk::GetClientWeapons(player.slot).iter().any(|owned| *owned == handle)
                    {
                        s2sdk::RemoveWeapon(player.slot, handle);
                    }
                    if s2sdk::IsValidEntHandle(handle) {
                        s2sdk::RemoveEntity(handle);
                    }
                }
                Err("healthshot delivery failed".to_owned())
            },
        )
    };
    if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
        runtime.finish_healthshot_purchase(task);
    }
    if callbacks_active() && game::LivePlayer::resolve_present(player.slot) == Some(player) {
        let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(runtime) = guard.as_ref() {
            let message = match result {
                Ok(_) => i18n::Message::new("healthshot.purchased"),
                Err(error) => runtime.purchase_error(player.slot, error),
            };
            i18n::send_chat(player.slot, &runtime.message(player.slot, message));
        }
    }
}

fn process_afterburn_ignite(task: funrounds_core::PlayerTask, player: game::LivePlayer) {
    if !callbacks_active() {
        return;
    }
    let action = runtime::EngineAction::AfterburnIgnite { task, player };
    let now = f64::from(s2sdk::GetGameTime());
    let Some(remaining) = (|| {
        let mut guard = RUNTIME.try_lock().ok()?;
        let runtime = guard.as_mut()?;
        (runtime.on_game_thread() && runtime.action_valid(&action))
            .then(|| runtime.begin_afterburn_ignite(task, player, now))
            .flatten()
    })() else {
        return;
    };

    if let Some(attached) = afterburn::attached_effect(player) {
        let removals = {
            let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.as_mut().map_or_else(Vec::new, |runtime| {
                runtime.afterburn_attachment_seen(task, player, now, Some(attached));
                runtime.take_afterburn_cleanup()
            })
        };
        for handle in removals {
            afterburn::remove_flame(handle);
        }
        return;
    }

    let flame = funrounds_native::TryIgnitePlayer(player.pawn_handle, remaining);
    if flame == -1 {
        if let Some(attached) = afterburn::attached_effect(player) {
            let removals = {
                let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.as_mut().map_or_else(Vec::new, |runtime| {
                    runtime.afterburn_attachment_seen(task, player, f64::from(s2sdk::GetGameTime()), Some(attached));
                    runtime.take_afterburn_cleanup()
                })
            };
            for handle in removals {
                afterburn::remove_flame(handle);
            }
            return;
        }
        if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
            runtime.afterburn_ignition_failed("native TryIgnitePlayer failed to attach a verified player flame");
        }
        return;
    }

    if !s2sdk::IsValidEntHandle(flame) || s2sdk::GetEntityClassname(flame).as_str() != "entityflame" {
        afterburn::remove_flame(flame);
        if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
            runtime.afterburn_ignition_failed("native TryIgnitePlayer returned an invalid entityflame handle");
        }
        return;
    }
    let recorded = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
        .is_some_and(|runtime| runtime.record_afterburn_flame(task, player, flame, f64::from(s2sdk::GetGameTime())));
    if !recorded {
        // The callback is still outside the runtime lock; remove only the full handle returned by our native call.
        afterburn::remove_flame(flame);
    }
}

fn process_chicken_eat(task: funrounds_core::PlayerTask, player: game::LivePlayer, chicken: i32, outcome: funrounds_core::ChickenEatOutcome) {
    let action = runtime::EngineAction::ChickenEat {
        task,
        player,
        chicken,
        outcome,
    };
    if engine_action_current(&action) {
        match outcome {
            funrounds_core::ChickenEatOutcome::Heal { healed, .. } => {
                let action = runtime::EngineAction::ChickenEat {
                    task,
                    player,
                    chicken,
                    outcome,
                };
                let _ = execute_valid_action(action, || {
                    let health = s2sdk::GetClientHealth(player.slot).saturating_add(healed);
                    s2sdk::SetClientHealth(player.slot, health);
                    if s2sdk::GetClientHealth(player.slot) != health {
                        return Err(fault::Fault::global(format!("chicken heal write failed for slot {}", player.slot)));
                    }
                    Ok(())
                });
            }
            funrounds_core::ChickenEatOutcome::Damage { amount } => {
                apply_chicken_eating_damage(task, player, chicken, outcome, amount.max(1));
            }
            funrounds_core::ChickenEatOutcome::Death => {
                let amount = s2sdk::GetClientHealth(player.slot)
                    .saturating_add(s2sdk::GetClientArmor(player.slot))
                    .max(1);
                apply_chicken_eating_damage(task, player, chicken, outcome, amount);
            }
            funrounds_core::ChickenEatOutcome::RiskBlast(config) => {
                let headshot = RUNTIME
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .as_ref()
                    .is_some_and(|runtime| runtime.headshots() == funrounds_core::Headshots::Only);
                if let Some(origin) = chicken_entities::position(chicken) {
                    process_chicken_blast(task.round, chicken, Some(player.pawn_handle), origin, config);
                }
                finish_chicken_risk_blast(task, player, headshot);
            }
        }
    }
    chicken_entities::remove(chicken);
    if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
        runtime.finish_chicken_use(task.round, chicken);
    }
}

// Synchronous continuation: the blast may end the round, but not cancel this
// accepted eater's forced death. Never carry it over to another pawn or life.
fn finish_chicken_risk_blast(task: funrounds_core::PlayerTask, player: game::LivePlayer, headshot: bool) {
    let current = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .is_some_and(|runtime| callbacks_active() && runtime.on_game_thread() && runtime.chicken_risk_finish_current(task, player));
    if !current {
        return;
    }
    let amount = (s2sdk::GetClientHealth(player.slot).max(0) as f32 + s2sdk::GetClientArmor(player.slot).max(0) as f32) * 10.0;
    let zero = plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    let result = damage::with_internal_damage_context(
        damage::InternalDamageContext {
            victim: player.pawn_handle,
            attacker: player.pawn_handle,
            inflictor: player.pawn_handle,
            weapon: -1,
            victim_slot: player.slot,
            attacker_slot: player.slot,
            headshot,
            check_protection: false,
            suppress_lifesteal: true,
            random_heal: None,
            death_weapon: Some("chicken".into()),
        },
        || {
            damage::apply_native_damage(
                damage::NativeDamageCall {
                    victim: player.pawn_handle,
                    inflictor: player.pawn_handle,
                    attacker: player.pawn_handle,
                    force: &zero,
                    position: &zero,
                    amount: amount.max(1.0),
                    damage_type: 0,
                    weapon: -1,
                    skip_plugin_rules: true,
                },
                fault::Mechanism::ChickenEating,
                "chicken risk forced death rejected",
            )
        },
    );
    if let Err(error) = result {
        s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] chicken risk forced death failed: {error}")));
    }
}

fn apply_chicken_eating_damage(
    task: funrounds_core::PlayerTask,
    player: game::LivePlayer,
    chicken: i32,
    outcome: funrounds_core::ChickenEatOutcome,
    amount: i32,
) {
    let action = runtime::EngineAction::ChickenEat {
        task,
        player,
        chicken,
        outcome,
    };
    if !engine_action_current(&action) {
        return;
    }
    let headshot = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .is_some_and(|runtime| runtime.headshots() == funrounds_core::Headshots::Only)
        && amount >= s2sdk::GetClientHealth(player.slot).saturating_add(s2sdk::GetClientArmor(player.slot));
    let zero = plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    // The native skip flag identifies the call; only this exact scoped replay
    // bypasses ordinary self/headshot/combination rules in the damage callback.
    let result = damage::with_internal_damage_context(
        damage::InternalDamageContext {
            victim: player.pawn_handle,
            attacker: player.pawn_handle,
            inflictor: player.pawn_handle,
            weapon: -1,
            victim_slot: player.slot,
            attacker_slot: player.slot,
            headshot,
            check_protection: matches!(outcome, funrounds_core::ChickenEatOutcome::Damage { .. }),
            suppress_lifesteal: true,
            random_heal: None,
            death_weapon: Some("chicken".into()),
        },
        || {
            damage::apply_native_damage(
                damage::NativeDamageCall {
                    victim: player.pawn_handle,
                    inflictor: player.pawn_handle,
                    attacker: player.pawn_handle,
                    force: &zero,
                    position: &zero,
                    amount: amount.max(1) as f32,
                    damage_type: 0,
                    weapon: -1,
                    skip_plugin_rules: true,
                },
                fault::Mechanism::ChickenEating,
                "chicken eating damage rejected after identity validation",
            )
        },
    );
    if let Err(error) = result {
        s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] chicken eating damage failed: {error}")));
    }
}

fn process_chicken_blast(
    round: funrounds_core::RoundGeneration,
    chicken: i32,
    attacker: Option<i32>,
    origin: plugify::Vec3,
    config: funrounds_core::ChickenBlastConfig,
) {
    let action = runtime::EngineAction::ChickenBlast {
        round,
        chicken,
        attacker,
        origin,
        config,
    };
    if !engine_action_current(&action) {
        return;
    }
    let (effect, credited_attacker) = match chicken_entities::prepare_blast(chicken, attacker, origin, config) {
        Ok(prepared) => prepared,
        Err(error) => {
            s2sdk::PrintToServer(&Str::from_str(&format!(
                "[FunRounds] ordinary chicken blast setup failed chicken={chicken}: {error}"
            )));
            return;
        }
    };
    let tracked = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
        .is_some_and(|runtime| runtime.on_game_thread() && runtime.track_chicken_explosion(round, effect, chicken));
    if !tracked {
        chicken_entities::remove(effect);
        return;
    }
    if engine_action_current(&action) {
        if catch_unwind(AssertUnwindSafe(|| chicken_entities::emit_blast_sound(effect, origin, config.radius))).is_err() {
            contain_failure("chicken blast sound callback panic contained");
        }
        if catch_unwind(AssertUnwindSafe(|| chicken_entities::detonate(effect, credited_attacker))).is_err() {
            contain_failure("chicken blast callback panic contained");
        }
    }
    chicken_entities::remove(effect);
    if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
        runtime.finish_chicken_explosion(round, effect);
    }
    s2sdk::PrintToServer(&Str::from_str(&format!(
        "[FunRounds] ordinary chicken blast round={round:?} chicken={chicken} damage={} radius={}",
        config.damage, config.radius
    )));
}

fn process_equipment_action(action: effects::equipment::Action) {
    let result = effects::equipment::execute(action, || {
        let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.as_ref().is_some_and(|runtime| {
            assert!(runtime.on_game_thread(), "FunRounds equipment action is not on the game thread");
            (callbacks_active() || matches!(action, effects::equipment::Action::Remove { .. }))
                && runtime.action_valid(&runtime::EngineAction::Equipment(action))
        })
    });
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.finish_equipment_action(action, result);
    }
}

fn apply_pending_bomb_explosion() {
    if !callbacks_active() {
        return;
    }
    let explosion = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
        .and_then(|runtime| {
            assert!(runtime.on_game_thread(), "FunRounds bomb explosion is not on the game thread");
            runtime.take_bomb_explosion()
        });
    if let Some(explosion) = explosion {
        let (temporary, result) = bombs::explode(explosion);
        if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
            runtime.finish_bomb_explosion(explosion, temporary, result);
        }
    }
}

fn finish_team_round(reason: s2sdk::CSRoundEndReason) {
    // TerminateRound announces the result but does not award a native round win.
    if let Err(error) = game::award_team_round(reason) {
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(runtime) = guard.as_mut() {
            runtime.handle_fault(fault::Fault::mechanism(fault::Mechanism::TeamDeathmatch, error));
        }
        return;
    }
    s2sdk::TerminateRound(3.0, reason);
}

fn subscribe() -> Result<(), Box<dyn Error>> {
    let result = boundary(|| {
        if HOOKS.load(Ordering::Acquire) != 0 {
            return Err("FunRounds: previous event subscriptions remain".into());
        }
        for (index, event) in EVENTS.iter().enumerate() {
            let result = s2sdk::HookEvent(&Str::from_str(event), on_event, event_hook_mode(event));
            if result != EventHookError::Okay {
                if *event == "round_prestart" {
                    s2sdk::PrintToServer(&Str::from_str(
                        "[FunRounds] round_prestart unavailable; purchase, chicken-bomb, and team-deathmatch modes excluded",
                    ));
                    continue;
                } else if *event == "bot_takeover" {
                    return Err("FunRounds requires the bot_takeover event for player lifecycle tracking".into());
                } else if *event == "item_pickup" {
                    s2sdk::PrintToServer(&Str::from_str(&format!(
                        "[FunRounds] {event} unavailable; weapon scale pickup recovery uses inventory reconciliation"
                    )));
                    continue;
                } else if index >= 12 {
                    s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] {event} unavailable; special bomb modes excluded")));
                    continue;
                } else if *event == "grenade_thrown" {
                    s2sdk::PrintToServer(&Str::from_str("[FunRounds] grenade_thrown unavailable; HE refill modes excluded"));
                    continue;
                } else if *event == "weapon_fire" {
                    s2sdk::PrintToServer(&Str::from_str("[FunRounds] weapon_fire unavailable; shooting-effect modes excluded"));
                    continue;
                } else if *event == "player_hurt" {
                    s2sdk::PrintToServer(&Str::from_str("[FunRounds] player_hurt unavailable; lifesteal modes excluded"));
                    continue;
                } else if *event == "player_connect_full" {
                    s2sdk::PrintToServer(&Str::from_str(
                        "[FunRounds] player_connect_full unavailable; classic public behavior capability unavailable",
                    ));
                    continue;
                }
                return Err(format!("FunRounds: cannot subscribe to {event}: {result:?}").into());
            }
            HOOKS.fetch_or(1 << index, Ordering::Release);
        }
        if unsafe { s2sdk::__s2sdk_SetEventString }.is_some() {
            let event = Str::from_str("player_death");
            let result = s2sdk::HookEvent(&event, on_player_death_pre, HookMode::Pre);
            if result == EventHookError::Okay {
                KNIFE_DEATH_PRE_HOOKED.store(true, Ordering::Release);
            } else {
                s2sdk::PrintToServer(&Str::from_str(&format!(
                    "[FunRounds] throwing-knife kill icon hook unavailable: {result:?}"
                )));
            }
        } else {
            s2sdk::PrintToServer(&Str::from_str(
                "[FunRounds] SetEventString unavailable; throwing-knife kill icon remains unchanged",
            ));
        }
        *DISCONNECT_BRIDGE.lock().map_err(|_| "FunRounds: disconnect bridge poisoned")? = Some(disconnect::Bridge::subscribe(on_client_disconnect));
        let hooks = HOOKS.load(Ordering::Acquire);
        let bridge = damage::Bridge::subscribe(on_damage);
        let damage_ready = bridge.is_some();
        *DAMAGE_BRIDGE.lock().map_err(|_| "FunRounds: damage bridge poisoned")? = bridge;
        s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] damage hook ready={damage_ready}")));
        let (chicken_bridge, chicken_damage_ready) = damage::EntityBridge::subscribe(on_chicken_entity_damage);
        *CHICKEN_DAMAGE_BRIDGE.lock().map_err(|_| "FunRounds: chicken damage bridge poisoned")? = chicken_bridge;
        if !chicken_damage_ready {
            s2sdk::PrintToServer(&Str::from_str(
                "[FunRounds] ordinary chicken damage filtering unavailable; configured chickens are disabled",
            ));
        }
        if unsafe { s2sdk::__s2sdk_AddCommandListener }.is_some() {
            let hooked = s2sdk::AddCommandListener(&Str::from_str(""), on_chat, HookMode::Pre);
            CHAT_HOOKED.store(hooked, Ordering::Release);
        }
        let (support, inspect, hook, unhook) = unsafe {
            (
                funrounds_native::__funrounds_native_HasSuicideSupport,
                funrounds_native::__funrounds_native_GetLookAtWeaponButtonState,
                funrounds_native::__funrounds_native_HookPlayerCanAcquire,
                funrounds_native::__funrounds_native_UnhookPlayerCanAcquire,
            )
        };
        let suicide_ready = support.is_some()
            && inspect.is_some()
            && hook.is_some()
            && unhook.is_some()
            && funrounds_native::HasSuicideSupport()
            && funrounds_native::HookPlayerCanAcquire(on_can_acquire);
        CAN_ACQUIRE_HOOKED.store(suicide_ready, Ordering::Release);
        let mut moving_blast = subscribed_events(hooks, &["bomb_exploded"]);
        moving_blast.require(damage_ready, "hook:entity_damage");
        let mut bot_takeover = subscribed_events(hooks, &["bot_takeover"]);
        bot_takeover.include(game::bot_takeover_probe());
        let mut combat_events = subscribed_events(hooks, &["player_spawn", "player_death"]);
        combat_events.include(bot_takeover.clone());
        let mut bomb_events = subscribed_events(
            hooks,
            &[
                "bomb_beginplant",
                "bomb_abortplant",
                "bomb_planted",
                "bomb_begindefuse",
                "bomb_abortdefuse",
            ],
        );
        bomb_events.include(bot_takeover.clone());
        let bomb_plant_tracking = subscribed_events(hooks, &["bomb_beginplant", "bomb_planted"]);
        let healthshot_support = healthshot_native_available();
        let use_target_support = use_target_native_available();
        let chicken_follow_block_support = chicken_follow_block_native_available();
        let mut runtime = runtime::Runtime::new(runtime::EventCapabilities {
            grenade_thrown: hooks & (1 << 6) != 0,
            weapon_fire: hooks & (1 << 7) != 0,
            player_hurt: hooks & (1 << 8) != 0,
            damage: damage_ready,
            chicken_damage: chicken_damage_ready,
            chicken_follow_block_support,
            combat_events: combat_events.available(),
            bot_takeover: bot_takeover.clone(),
            round_prestart: subscribed_events(hooks, &["round_prestart"]).available(),
            bombs: bomb_events,
            bomb_plant_tracking,
            moving_blast,
            healthshot: healthshot_support,
        })?;
        if runtime.healthshot_configured() {
            if !healthshot_support {
                runtime.healthshot_hook_failure("native healthshot hooks or required ConVars unavailable");
            } else {
                HEALTHSHOT_HOOKED.store(true, Ordering::Release);
                if funrounds_native::HookHealthshot(on_healthshot_can_use, on_healthshot_effect) {
                    runtime.healthshot_hook_ready();
                } else {
                    runtime.healthshot_hook_failure("native healthshot hook registration failed");
                }
            }
        }
        if runtime.chicken_use_configured() {
            if use_target_support {
                USE_TARGET_HOOKED.store(true, Ordering::Release);
                if funrounds_native::HookUseTarget(on_use_target) {
                    runtime.chicken_use_hook_ready();
                } else {
                    runtime.chicken_use_hook_failure("native committed use-target hook registration failed");
                }
            } else {
                runtime.chicken_use_hook_failure("native committed use-target selector unavailable");
            }
        }
        *RUNTIME.lock().map_err(|_| "FunRounds: runtime poisoned")? = Some(runtime);
        {
            let mut guard = RUNTIME.lock().map_err(|_| "FunRounds: runtime poisoned")?;
            sync_chicken_follow_blocking(guard.as_mut().expect("runtime was just installed"));
        }
        let legacy_filter_enabled = RUNTIME
            .lock()
            .map_err(|_| "FunRounds: runtime poisoned")?
            .as_ref()
            .is_some_and(runtime::Runtime::legacy_textmsg_enabled);
        legacy_textmsg::set_enabled(legacy_filter_enabled);
        match legacy_textmsg::install() {
            Ok(()) => s2sdk::PrintToServer(&Str::from_str("[FunRounds] legacy cash/score TextMsg filter ready")),
            Err(reason) => s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] {reason}"))),
        }
        register_commands()?;
        ACTIVE.store(true, Ordering::Release);
        s2sdk::PrintToServer(&Str::from_str(&format!(
            "[FunRounds] {} loaded; subscribed to {} events; gameplay presets enabled when configured",
            funrounds_core::VERSION,
            hooks.count_ones()
        )));
        Ok(())
    });
    if result.is_err() {
        boundary(cleanup)?;
    }
    result
}

fn subscribed_events(hooks: u32, events: &[&str]) -> capability::Probe {
    let mut probe = capability::Probe::default();
    for event in events {
        probe.require(
            EVENTS
                .iter()
                .position(|name| name == event)
                .is_some_and(|index| hooks & (1 << index) != 0),
            format!("event:{event}"),
        );
    }
    probe
}

fn cleanup() -> Result<(), Box<dyn Error>> {
    ACTIVE.store(false, Ordering::Release);
    legacy_textmsg::set_enabled(false);
    CHICKEN_DAMAGE_ACTIVE.store(false, Ordering::Release);
    let mut failed = Vec::new();
    if let Err(error) = unregister_suicide_manifest_callback() {
        failed.push(error.to_string());
    }
    if !disable_chicken_follow_block(false) {
        failed.push("native chicken-follow detour cleanup is not confirmed".into());
    }
    if legacy_textmsg::hooked() && !legacy_textmsg::uninstall() {
        failed.push("legacy TextMsg hook unsubscription failed".into());
    }
    if CAN_ACQUIRE_HOOKED.load(Ordering::Acquire) {
        if funrounds_native::UnhookPlayerCanAcquire(on_can_acquire) {
            CAN_ACQUIRE_HOOKED.store(false, Ordering::Release);
        } else {
            failed.push("suicide pickup unsubscription failed".into());
        }
    }
    if HEALTHSHOT_HOOKED.load(Ordering::Acquire) {
        if funrounds_native::UnhookHealthshot(on_healthshot_can_use, on_healthshot_effect) {
            HEALTHSHOT_HOOKED.store(false, Ordering::Release);
        } else {
            failed.push("healthshot callback unsubscription failed".into());
        }
    }
    if USE_TARGET_HOOKED.load(Ordering::Acquire) {
        if funrounds_native::UnhookUseTarget(on_use_target) {
            USE_TARGET_HOOKED.store(false, Ordering::Release);
        } else {
            failed.push("player use-target callback unsubscription failed".into());
        }
    }
    if TOUCH_CLEANUP_PENDING.load(Ordering::Acquire) {
        if funrounds_native::UnhookEntityTouchPre(on_touch) {
            TOUCH_CLEANUP_PENDING.store(false, Ordering::Release);
            TOUCH_RELEASE_SUPPRESSED.store(false, Ordering::Release);
            touch::clear();
            touch::clear_rescue_zones();
        } else {
            failed.push("projectile touch unsubscription failed".into());
        }
    }
    if CHAT_HOOKED.load(Ordering::Acquire) {
        if s2sdk::RemoveCommandListener(&Str::from_str(""), on_chat, HookMode::Pre) {
            CHAT_HOOKED.store(false, Ordering::Release);
        } else {
            failed.push("chat listener unsubscription failed".into());
        }
    }
    if KNIFE_DEATH_PRE_HOOKED.load(Ordering::Acquire) {
        let result = s2sdk::UnhookEvent(&Str::from_str("player_death"), on_player_death_pre, HookMode::Pre);
        if result == EventHookError::Okay {
            KNIFE_DEATH_PRE_HOOKED.store(false, Ordering::Release);
        } else {
            failed.push(format!("player_death pre hook: {result:?}"));
        }
    }
    let mut chicken_damage_bridge = CHICKEN_DAMAGE_BRIDGE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(registered) = chicken_damage_bridge.as_ref() {
        if registered.unsubscribe() {
            *chicken_damage_bridge = None;
        } else {
            failed.push("ordinary chicken damage unsubscription failed".into());
        }
    }
    drop(chicken_damage_bridge);
    let mut bridge = DAMAGE_BRIDGE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(registered) = bridge.as_ref() {
        if !registered.unsubscribe() {
            failed.push("damage callback unsubscription failed".into());
        } else {
            *bridge = None;
        }
    }
    drop(bridge);
    if let Some(bridge) = DISCONNECT_BRIDGE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take() {
        bridge.unsubscribe();
    }
    let mut shutdown_error = None;
    let mut shutdown_settled = false;
    for _ in 0..4 {
        let (actions, equipment, projectiles, rollbacks, chickens, afterburns, suicide_feedback, error) = {
            let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(runtime) = guard.as_mut() else { break };
            let error = runtime.shutdown().err();
            (
                runtime.take_bomb_actions(),
                runtime.take_equipment_cleanup(),
                runtime.take_projectile_cleanup(),
                runtime.take_rollbacks(),
                runtime.take_chicken_cleanup(),
                runtime.take_afterburn_cleanup(),
                runtime.take_suicide_feedback_cleanup(),
                error,
            )
        };
        shutdown_error = error;
        if actions.is_empty()
            && equipment.is_empty()
            && projectiles.is_empty()
            && rollbacks.is_empty()
            && chickens.is_empty()
            && afterburns.is_empty()
            && suicide_feedback.is_empty()
        {
            shutdown_settled = true;
            break;
        }
        for handle in chickens {
            chicken_entities::remove(handle);
        }
        for handle in afterburns {
            afterburn::remove_flame(handle);
        }
        for handle in suicide_feedback {
            suicide::remove_feedback(handle);
        }
        for action in actions {
            process_bomb_action(action);
        }
        for action in equipment {
            process_equipment_action(action);
        }
        for action in projectiles {
            process_projectile_action(action);
        }
        for action in rollbacks {
            process_rollback_action(action);
        }
    }
    if !shutdown_settled {
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(runtime) = guard.as_mut() {
            shutdown_error = runtime.shutdown().err();
        }
    }
    if let Some(error) = shutdown_error {
        failed.push(error);
    }
    for (index, name) in commands::NAMES.iter().enumerate() {
        let bit = 1 << index;
        if COMMANDS.load(Ordering::Acquire) & bit == 0 {
            continue;
        }
        s2sdk::RemoveCommand(&Str::from_str(name), on_command);
        // Unregistration preserves the engine name; check callbacks rather than name presence.
        if command_exists(name) {
            failed.push(format!("command remains: {name}"));
        } else {
            COMMANDS.fetch_and(!bit, Ordering::Release);
        }
    }
    for (index, event) in EVENTS.iter().enumerate() {
        let bit = 1 << index;
        if HOOKS.load(Ordering::Acquire) & bit == 0 {
            continue;
        }
        let result = s2sdk::UnhookEvent(&Str::from_str(event), on_event, event_hook_mode(event));
        if result == EventHookError::Okay {
            HOOKS.fetch_and(!bit, Ordering::Release);
        } else {
            failed.push(format!("{event}: {result:?}"));
        }
    }
    let preferences = {
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.as_mut().and_then(runtime::Runtime::take_preferences)
    };
    if let Some(mut preferences) = preferences
        && let Err(error) = preferences.shutdown()
    {
        failed.push(format!("player preferences: {error}"));
        let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(runtime) = guard.as_mut() {
            runtime.restore_preferences(preferences);
        } else {
            failed.push("runtime disappeared while retaining unsaved player preferences".into());
        }
    }
    if !failed.is_empty() {
        return Err(format!("FunRounds: event cleanup failed: {}", failed.join(", ")).into());
    }
    *RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    RUNTIME.clear_poison();
    DAMAGE_BRIDGE.clear_poison();
    DISCONNECT_BRIDGE.clear_poison();
    PANIC_PENDING.store(false, Ordering::Release);
    Ok(())
}

fn end() -> Result<(), Box<dyn Error>> {
    boundary(|| {
        PENDING.store(false, Ordering::Release);
        cleanup()?;
        s2sdk::PrintToServer(&Str::from_str("[FunRounds] unloaded; all event subscriptions removed"));
        Ok(())
    })
}

fn hook_knife_touch(handle: i32) -> bool {
    TOUCH_RELEASE_SUPPRESSED.store(false, Ordering::Release);
    let hooked = touch::register_with_retry(|| funrounds_native::HookEntityTouchPre(handle, on_touch));
    // A failed Hook can leave a native-owned partial detour if its rollback also
    // failed. Retain the cleanup gate until Unhook confirms full detachment.
    TOUCH_CLEANUP_PENDING.store(true, Ordering::Release);
    hooked
}

fn release_touch_hook_if_unused() {
    if TOUCH_CLEANUP_PENDING.load(Ordering::Acquire) && !touch::has_owners() && !TOUCH_RELEASE_SUPPRESSED.load(Ordering::Acquire) {
        if funrounds_native::UnhookEntityTouchPre(on_touch) {
            TOUCH_CLEANUP_PENDING.store(false, Ordering::Release);
            TOUCH_RELEASE_SUPPRESSED.store(false, Ordering::Release);
        } else {
            // Avoid retrying an unchanged detach failure every game frame; plugin
            // cleanup makes one final explicit attempt before reporting failure.
            TOUCH_RELEASE_SUPPRESSED.store(true, Ordering::Release);
        }
    }
}

fn sync_hostage_rescue_touch() {
    let enabled = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .is_some_and(runtime::Runtime::hostage_rescue_touch_enabled);
    if !enabled {
        touch::clear_rescue_zones();
        if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
            runtime.hostage_rescue_hook_ready();
        }
        release_touch_hook_if_unused();
        return;
    }

    let target_zones = match compat::hostage_rescue::enabled_rescue_zones() {
        Ok(zones) => zones,
        Err(reason) => {
            touch::clear_rescue_zones();
            if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
                runtime.hostage_rescue_hook_failure(reason);
            }
            release_touch_hook_if_unused();
            return;
        }
    };
    let targets = target_zones.iter().copied().collect::<std::collections::HashSet<_>>();
    touch::retain_rescue_zones(&targets);
    let mut failure = None;
    for handle in target_zones {
        if touch::owns_rescue_zone(handle) {
            continue;
        }
        if hook_knife_touch(handle) {
            touch::track_rescue_zone(handle);
        } else {
            failure = Some(format!("native Touch hook failed for rescue trigger {handle}"));
        }
    }
    if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
        if let Some(reason) = failure {
            runtime.hostage_rescue_hook_failure(reason);
        } else {
            runtime.hostage_rescue_hook_ready();
        }
    }
    release_touch_hook_if_unused();
}

unsafe extern "C" fn on_chicken_entity_damage(
    victim: i32,
    attacker: i32,
    inflictor: i32,
    ability: i32,
    damage: f32,
    damage_type: u32,
    _skip_plugin_rules: bool,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() || !CHICKEN_DAMAGE_ACTIVE.load(Ordering::Acquire) {
            return 0;
        }
        let mut guard = RUNTIME.try_lock().expect("FunRounds runtime busy in chicken damage callback");
        let Some(runtime) = guard.as_mut() else { return 0 };
        assert!(runtime.on_game_thread(), "FunRounds chicken damage callback is not on the game thread");
        i32::from(runtime.chicken_damage_blocked(chicken_entities::EntityDamageSource {
            victim,
            attacker,
            inflictor,
            ability,
            damage_type,
            damage,
        }))
    }))
    .unwrap_or_else(|_| {
        contain_failure("chicken damage callback panic contained");
        0
    })
}

unsafe extern "C" fn on_healthshot_can_use(pawn_handle: i32, weapon_handle: i32) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() {
            return 0;
        }
        let guard = RUNTIME.try_lock().expect("FunRounds runtime busy in healthshot eligibility callback");
        let Some(runtime) = guard.as_ref() else { return 0 };
        assert!(
            runtime.on_game_thread(),
            "FunRounds healthshot eligibility callback is not on game thread"
        );
        i32::from(runtime.reject_healthshot_use(pawn_handle, weapon_handle))
    }))
    .unwrap_or_else(|_| {
        contain_failure("healthshot eligibility callback panic contained");
        0
    })
}

unsafe extern "C" fn on_healthshot_effect(pawn_handle: i32, weapon_handle: i32) {
    catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() {
            return;
        }
        let mut guard = RUNTIME.try_lock().expect("FunRounds runtime busy in healthshot effect callback");
        let Some(runtime) = guard.as_mut() else { return };
        assert!(runtime.on_game_thread(), "FunRounds healthshot effect callback is not on game thread");
        runtime.queue_healthshot_use(pawn_handle, weapon_handle);
    }))
    .unwrap_or_else(|_| contain_failure("healthshot effect callback panic contained"));
}

unsafe extern "C" fn on_use_target(pawn_handle: i32, target_handle: i32, in_use: bool) {
    catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() {
            return;
        }
        let mut guard = RUNTIME.try_lock().expect("FunRounds runtime busy in player use-target callback");
        let Some(runtime) = guard.as_mut() else { return };
        assert!(runtime.on_game_thread(), "FunRounds player use-target callback is not on game thread");
        runtime.observe_chicken_use_target(pawn_handle, target_handle, in_use);
    }))
    .unwrap_or_else(|_| contain_failure("player use-target callback panic contained"));
}

unsafe extern "C" fn on_touch(entity: i32, other: i32) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        // The native detour covers a shared Touch implementation, including ordinary weapons.
        if !callbacks_active() || !touch::owns_touch_pair(entity, other) {
            return 0;
        }
        let mut guard = RUNTIME.try_lock().expect("FunRounds runtime busy in touch callback");
        let Some(runtime) = guard.as_mut() else { return 0 };
        assert!(runtime.on_game_thread(), "FunRounds touch callback is not on game thread");
        let damageable_chicken = runtime.is_damageable_chicken(other);
        let block = runtime.projectile_touch(entity, other, damageable_chicken);
        let chicken_impacts = runtime.take_chicken_impacts();
        let rescue = runtime.hostage_rescue_touch(entity, other);
        drop(guard);
        for impact in chicken_impacts {
            process_projectile_chicken_impact(impact);
        }
        if let Some(action) = rescue {
            process_hostage_rescue(action);
        }
        i32::from(block)
    }))
    .unwrap_or_else(|_| {
        contain_failure("touch callback panic contained");
        1
    })
}

fn process_projectile_chicken_impact(impact: projectiles::ChickenImpact) {
    let current = RUNTIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .filter(|runtime| callbacks_active() && runtime.on_game_thread())
        .is_some_and(|runtime| runtime.chicken_impact_current(impact));
    let result = if current { projectiles::apply_chicken_impact(impact) } else { Ok(0) };
    if let Some(runtime) = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
        runtime.finish_chicken_impact(impact, result);
    }
}

fn process_hostage_rescue(action: compat::hostage_rescue::ForwardAction) {
    let input = Str::from_str("OnRescueZoneTouch");
    let value = plugify::Var::new(&plugify::Any::Invalid);
    let _ = execute_valid_action(runtime::EngineAction::HostageRescue(action), || {
        s2sdk::AcceptEntityInput(action.hostage, &input, -1, -1, &value, s2sdk::FieldType::Auto, 0);
        Ok(())
    });
}

fn process_projectile_action(action: projectiles::Action) {
    let result = projectiles::execute(action, || {
        let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.as_ref().is_some_and(|runtime| {
            assert!(runtime.on_game_thread(), "FunRounds projectile action is not on the game thread");
            (callbacks_active() || matches!(action.kind, projectiles::ActionKind::Remove { .. }))
                && runtime.action_valid(&runtime::EngineAction::Projectile(action))
        })
    });
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.finish_projectile_action(action, result);
    }
}

fn apply_projectile_impact(impact: projectiles::Impact) {
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let diagnostics = guard.as_ref().is_some_and(runtime::Runtime::diagnostics);
    let prepared = guard.as_mut().and_then(|runtime| runtime.prepare_impact(impact));
    drop(guard);
    let result = prepared.map(projectiles::apply_impact);
    if diagnostics && let Some(Ok(applied)) = result.as_ref() {
        let prepared = prepared.expect("prepared impact produced a result");
        s2sdk::PrintToServer(&Str::from_str(&format!(
            "[FunRounds] projectile {:?} entity={} attacker={} victim={} base={} head={} applied={applied}",
            prepared.kind, prepared.projectile, prepared.task.player.slot, prepared.victim.slot, prepared.amount, prepared.headshot
        )));
    }
    let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(runtime) = guard.as_mut() {
        runtime.finish_impact(prepared.unwrap_or(impact));
        if let Some(Err(error)) = result {
            runtime.handle_fault(error);
        }
    }
}

fn probe_player(slot: i32) {
    if let Some(player) = game::LivePlayer::resolve(slot) {
        s2sdk::PrintToServer(&Str::from_str(&format!(
            "[FunRounds] probe slot={slot} health={} armor={} gravity={} active_weapon_handle={} controller_handle={} pawn_handle={} warmup={:?} helmet={:?} team={:?}",
            s2sdk::GetClientHealth(slot),
            s2sdk::GetClientArmor(slot),
            s2sdk::GetClientGravity(slot),
            s2sdk::GetClientActiveWeapon(slot),
            player.controller_handle,
            player.pawn_handle,
            game::warmup(),
            player.helmet(),
            s2sdk::GetClientTeam(slot),
        )));
    }
}

fn event_log_enabled(name: &str, diagnostics: bool) -> bool {
    diagnostics || matches!(name, "round_start" | "round_freeze_end" | "round_end")
}

fn event_hook_mode(name: &str) -> HookMode {
    if name == "round_prestart" { HookMode::Pre } else { HookMode::Post }
}

fn probe_event_players(name: &str, event: usize, diagnostics: bool) {
    if !diagnostics {
        return;
    }
    if name == "player_spawn" {
        let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
        probe_player(slot);
    } else if name == "round_freeze_end" {
        for slot in 0..s2sdk::GetMaxClients() {
            probe_player(slot);
        }
    }
}

// The native pre-disconnect listener runs before CS2 deletes the carrier's children.
unsafe extern "C" fn on_client_disconnect(slot: i32, _reason: i32) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() {
            return;
        }
        let action = {
            let mut guard = RUNTIME.try_lock().expect("FunRounds runtime busy in pre-disconnect callback");
            let Some(runtime) = guard.as_mut() else { return };
            assert!(runtime.on_game_thread(), "FunRounds pre-disconnect is not on the game thread");
            runtime.prepare_carrier_disconnect(slot)
        };
        if let Some(action) = action {
            process_bomb_action(action);
        }
    }));
    if result.is_err() {
        contain_failure("pre-disconnect callback panic contained");
    }
}

// The event and string are borrowed for this callback only. No pointers or player slots are retained.
unsafe extern "C" fn on_event(name: &Str, event: usize, _dont_broadcast: bool) -> ResultType {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() {
            return;
        }
        let name = name.to_string();
        if name == "round_end" {
            // CS2 has committed the winner, but mode health/armor have not been restored yet.
            apply_pending_bomb_explosion();
        }
        let (diagnostics, bomb_cleanup, chicken_cleanup, suicide_feedback_cleanup, prestart_context) = if let Ok(mut guard) = RUNTIME.try_lock() {
            if let Some(runtime) = guard.as_mut() {
                if !runtime.on_game_thread() {
                    panic!("FunRounds event is not on the game thread");
                }
                runtime.event(&name, event);
                sync_chicken_follow_blocking(runtime);
                let prestart_context = if name == "round_prestart" {
                    runtime.take_prestart_context()
                } else {
                    None
                };
                let cleanup = if name == "round_prestart" {
                    runtime.take_bomb_actions()
                } else {
                    Vec::new()
                };
                let chicken_cleanup = if name == "round_prestart" {
                    runtime.take_chicken_cleanup()
                } else {
                    Vec::new()
                };
                let suicide_feedback_cleanup = if name == "round_prestart" {
                    runtime.take_suicide_feedback_cleanup()
                } else {
                    Vec::new()
                };
                (
                    runtime.diagnostics(),
                    cleanup,
                    chicken_cleanup,
                    suicide_feedback_cleanup,
                    prestart_context,
                )
            } else {
                (false, Vec::new(), Vec::new(), Vec::new(), None)
            }
        } else {
            panic!("FunRounds runtime busy or poisoned in event callback");
        };
        // Submit old helpers before the native round reset flushes entity deletions.
        for handle in chicken_cleanup {
            chicken_entities::remove(handle);
        }
        for handle in suicide_feedback_cleanup {
            suicide::remove_feedback(handle);
        }
        for action in bomb_cleanup {
            process_bomb_action(action);
        }
        if let Some(context) = prestart_context {
            let chicken_cleanup = {
                let mut guard = RUNTIME
                    .try_lock()
                    .expect("FunRounds runtime busy or poisoned after round_prestart cleanup");
                if let Some(runtime) = guard.as_mut() {
                    if !runtime.on_game_thread() {
                        panic!("FunRounds round_prestart completion is not on the game thread");
                    }
                    runtime.finish_round_prestart(context);
                    runtime.take_chicken_cleanup()
                } else {
                    Vec::new()
                }
            };
            // Round selection can disable ordinary chickens after the first cleanup drain.
            // Submit that newly queued work before round_start can confirm the engine reset.
            for handle in chicken_cleanup {
                chicken_entities::remove(handle);
            }
        }
        if event_log_enabled(&name, diagnostics) {
            s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] event: {name}")));
        }
        if name == "round_start" {
            sync_hostage_rescue_touch();
        }
        probe_event_players(&name, event, diagnostics);
    }));
    if result.is_err() {
        contain_failure("event callback panic contained");
    }
    ResultType::Continue
}

unsafe extern "C" fn on_player_death_pre(_name: &Str, event: usize, _dont_broadcast: bool) -> ResultType {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() {
            return;
        }
        let victim = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
        let attacker = s2sdk::GetEventPlayerSlot(event, &Str::from_str("attacker"));
        let replay_weapon = damage::internal_damage_death_weapon(victim, attacker);
        let matches_knife_impact = {
            let guard = RUNTIME.try_lock().expect("FunRounds runtime busy or poisoned in death pre hook");
            let Some(runtime) = guard.as_ref() else { return };
            assert!(runtime.on_game_thread(), "FunRounds death pre hook is not on the game thread");
            runtime.active_knife_death(victim, attacker)
        };
        if let Some(weapon) = replay_weapon {
            s2sdk::SetEventString(event, &Str::from_str("weapon"), &Str::from_str(&weapon));
        } else if matches_knife_impact {
            s2sdk::SetEventString(event, &Str::from_str("weapon"), &Str::from_str("knife"));
        }
    }));
    if result.is_err() {
        contain_failure("player_death pre callback panic contained");
    }
    ResultType::Continue
}

register_plugin!(start: start, update: update, end: end);

unsafe extern "C" fn on_damage(
    victim: i32,
    attacker: i32,
    inflictor: i32,
    weapon_kind: i32,
    hit_group: i32,
    traced_head: bool,
    damage: f32,
    damage_type: u32,
    multiplier: &mut f32,
    mark_headshot: &mut bool,
    weapon: i32,
    skip_plugin_rules: bool,
) -> i32 {
    damage_decision(
        damage::Damage {
            victim,
            attacker,
            inflictor,
            weapon_kind,
            hit_group,
            traced_head,
            amount: damage,
            damage_type,
            weapon,
            weapon_class: None,
            skip_plugin_rules,
        },
        multiplier,
        mark_headshot,
    )
}

fn observer_team(team: i32) -> bool {
    team == 1
}

fn observer_slot(slot: i32) -> bool {
    let imports_available = unsafe {
        std::ptr::addr_of!(s2sdk::__s2sdk_GetMaxClients).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_IsClientInGame).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_GetClientTeam).read().is_some()
    };
    imports_available && slot >= 0 && slot < s2sdk::GetMaxClients() && s2sdk::IsClientInGame(slot) && observer_team(s2sdk::GetClientTeam(slot) as i32)
}

fn observer_attacker_matches(attacker: i32, team: i32, controller: i32, pawn: Option<i32>) -> bool {
    observer_team(team) && (attacker == controller || pawn == Some(attacker))
}

fn observer_attacker_handle(attacker: i32) -> bool {
    if attacker == -1 || !s2sdk::IsValidEntHandle(attacker) {
        return false;
    }
    for slot in 0..s2sdk::GetMaxClients() {
        if !s2sdk::IsClientInGame(slot) {
            continue;
        }
        let team = s2sdk::GetClientTeam(slot) as i32;
        if !observer_team(team) {
            continue;
        }
        let controller = s2sdk::PlayerSlotToEntHandle(slot);
        if !s2sdk::IsValidEntHandle(controller) {
            continue;
        }
        if observer_attacker_matches(attacker, team, controller, None) {
            return true;
        }
        let pawn = game::LivePlayer::resolve_present(slot)
            .filter(|player| player.controller_handle == controller && player.pawn_controller() == Some(controller))
            .map(|player| player.pawn_handle);
        if observer_attacker_matches(attacker, team, controller, pawn) {
            return true;
        }
    }
    false
}

fn observer_damage_should_block(internal_attacker_is_observer: Option<bool>, native_observer_attacker: impl FnOnce() -> bool) -> bool {
    internal_attacker_is_observer.unwrap_or_else(native_observer_attacker)
}

fn chicken_blast_action(owned_chicken_blast: bool, protected: bool, headshots: funrounds_core::Headshots) -> Option<(i32, bool)> {
    owned_chicken_blast.then(|| {
        // Chicken explosions have no hit trace or weapon hitgroup. In a
        // headshots-only round classify their accepted blast damage as a
        // headshot regardless of whether that individual blast is lethal.
        // Protection remains an independent block above this classification.
        let mark_headshot = !protected && headshots == funrounds_core::Headshots::Only;
        (i32::from(protected), mark_headshot)
    })
}

fn damage_decision(mut context: damage::Damage, multiplier: &mut f32, mark_headshot: &mut bool) -> i32 {
    *multiplier = 1.0;
    *mark_headshot = false;
    catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() {
            return 0;
        }
        let internal_observer = damage::internal_damage_attacker_slot(&context).map(observer_slot);
        if observer_damage_should_block(internal_observer, || observer_attacker_handle(context.attacker)) {
            return 1;
        }
        if let Some(action) = damage::apply_internal_damage(&context, mark_headshot, || {
            let guard = RUNTIME.try_lock().expect("FunRounds damage runtime busy or poisoned");
            guard.as_ref().is_some_and(|runtime| {
                assert!(runtime.on_game_thread(), "FunRounds damage callback is not on game thread");
                runtime.combat_protected(&game::DamagePlayers::resolve(context.victim, context.attacker))
            })
        }) {
            return action;
        }
        let mut guard = RUNTIME.try_lock().expect("FunRounds damage runtime busy or poisoned");
        let Some(runtime) = guard.as_mut() else { return 0 };
        assert!(runtime.on_game_thread(), "FunRounds damage callback is not on game thread");
        context.weapon_class = damage::capture_weapon_class(context.weapon, context.inflictor, context.weapon_kind);
        if runtime.blocks_native_bomb_damage(context.inflictor) {
            return 1;
        }
        if !s2sdk::IsValidEntHandle(context.victim) || !context.amount.is_finite() || context.amount < 0.0 {
            return 1;
        }
        let players = game::DamagePlayers::resolve(context.victim, context.attacker);
        runtime.observe_chicken_blast_player_damage(&context, &players);
        let Some((projectile_scale, projectile_head)) = runtime.projectile_damage(&mut context) else {
            return 1;
        };
        // A physical player hit is not a miss even when a later configured rule
        // blocks it, defers it, or changes its amount to zero.
        runtime.record_native_player_hit(&context, &players);
        if runtime.chicken_blast_damage(&context)
            && let Some((action, is_headshot)) = chicken_blast_action(true, runtime.combat_protected(&players), runtime.headshots())
        {
            *mark_headshot = is_headshot;
            return action;
        }
        let headshots = runtime.headshots();
        if headshots == funrounds_core::Headshots::Normal && !runtime.classic_round() {
            *mark_headshot = projectile_head;
            return 0;
        }
        if runtime.suppress_hit_native_self_damage(&context, &players) {
            return 1;
        }
        if runtime.combat_protected(&players) {
            return 1;
        }
        let attributed = players.attacker.is_some();
        if headshots == funrounds_core::Headshots::Disabled && context.is_head_hit(attributed) {
            return 1;
        }
        let (action, scale, headshot) = if headshots == funrounds_core::Headshots::Only {
            context.headshot_rule(attributed, runtime.classic_round())
        } else {
            (0, 1.0, false)
        };
        if action != 0 {
            return action;
        }
        let Some(common) = runtime.classic_damage_scale(&context, &players) else {
            return 1;
        };
        let combined_scale = scale * common * projectile_scale;
        let afterburn_damage = runtime.afterburn_flame_damage(&context, &players);
        let mut effective_damage = afterburn_damage.unwrap_or(context.amount) * combined_scale;
        let mut damage_scale = if afterburn_damage.is_some() {
            effective_damage / context.amount
        } else {
            combined_scale
        };
        let mut random_heal = 0;
        if let Some(result) = runtime.random_damage_roll(&context, &players, effective_damage) {
            effective_damage = result.amount;
            random_heal = result.attacker_heal;
            damage_scale = effective_damage / context.amount;
        }
        if !effective_damage.is_finite() || effective_damage == 0.0 {
            return 1;
        }
        if let Some(delayed_hit) = runtime.prepare_delayed_damage_with_heal(
            &context,
            &players,
            effective_damage,
            delayed_headshot_marker(&context, attributed, headshot, projectile_head),
            random_heal,
        ) {
            // Record the genuine source tick before the delayed hit is deferred. The
            // internal skip-rules replay cannot recreate this ground-burn contact.
            runtime.queue_afterburn_ground_tick(&context, &players);
            runtime.defer_damage(delayed_hit);
            return 1;
        }
        if runtime.combat_damage(&context, &players, damage_scale, true) {
            return 1;
        }
        if random_heal > 0
            && let Err(error) = runtime.queue_random_heal(&players, random_heal)
        {
            runtime.handle_fault(error);
            return 1;
        }
        runtime.queue_afterburn_ground_tick(&context, &players);
        *multiplier = damage_scale;
        *mark_headshot = headshot || projectile_head;
        action
    }))
    .unwrap_or_else(|_| {
        contain_failure("damage callback panic contained");
        1
    })
}

fn delayed_headshot_marker(context: &damage::Damage, attributed: bool, rule_headshot: bool, projectile_head: bool) -> bool {
    context.is_head_hit(attributed) || rule_headshot || projectile_head
}

unsafe extern "C" fn on_chat(slot: i32, context: ConCommandContext, args: &Arr<Str>) -> ResultType {
    catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() || slot < 0 {
            return ResultType::Continue;
        }
        if !s2sdk::IsClientInGame(slot) {
            // ClientCommand Pre still carries the real slot before SDK alias conversion.
            // Console context also describes client binds; only slot -1 is the server.
            let chat = context == ConCommandContext::Chat
                || args
                    .iter()
                    .next()
                    .is_some_and(|arg| arg.as_str().eq_ignore_ascii_case("say") || arg.as_str().eq_ignore_ascii_case("say_team"));
            if chat {
                s2sdk::PrintToServer(&Str::from_str(&format!(
                    "[FunRounds] blocked pre-ingame chat slot={slot} command={}",
                    if context == ConCommandContext::Chat {
                        "<chat-alias>"
                    } else {
                        args.iter().next().map_or("<empty>", Str::as_str)
                    }
                )));
                return ResultType::Stop;
            }
            return ResultType::Continue;
        }
        let args = args.iter().map(ToString::to_string).collect::<Vec<_>>();
        let Some(command) = commands::parse_chat(&args, context == ConCommandContext::Chat) else {
            return ResultType::Continue;
        };
        let mut guard = RUNTIME.try_lock().expect("FunRounds chat runtime busy or poisoned");
        let Some(runtime) = guard.as_mut() else { return ResultType::Continue };
        assert!(runtime.on_game_thread(), "FunRounds chat is not on the game thread");
        if !runtime.chat_commands_enabled() {
            return ResultType::Continue;
        }
        let message = command
            .map_err(|key| runtime.text(slot, key, &[]))
            .and_then(|command| runtime.command(command, slot))
            .unwrap_or_else(|error| runtime.text(slot, "feedback.error", &[("error", &error)]));
        drop(guard);
        i18n::send_chat(slot, &message);
        ResultType::Handled
    }))
    .unwrap_or_else(|_| {
        contain_failure("chat callback panic contained");
        ResultType::Handled
    })
}

fn command_exists(name: &str) -> bool {
    s2sdk::HasConsoleCommandCallbacks(&Str::from_str(name))
}

fn register_commands() -> Result<(), Box<dyn Error>> {
    for (index, name) in commands::NAMES.iter().enumerate() {
        if command_exists(name) {
            return Err(format!("FunRounds: command name already owned: {name}").into());
        }
        if !s2sdk::AddConsoleCommand(
            &Str::from_str(name),
            &Str::from_str("FunRounds"),
            if commands::client_executable(name) {
                ConVarFlag::ClientCanExecute
            } else {
                ConVarFlag::None
            },
            on_command,
            HookMode::Pre,
        ) {
            // Failed AddConsoleCommand can still attach our listener to an existing command.
            s2sdk::RemoveCommandListener(&Str::from_str(name), on_command, HookMode::Pre);
            return Err(format!("FunRounds: cannot register {name}").into());
        }
        COMMANDS.fetch_or(1 << index, Ordering::Release);
    }
    Ok(())
}

unsafe extern "C" fn on_command(slot: i32, context: ConCommandContext, args: &Arr<Str>) -> ResultType {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), Box<dyn Error>> {
        if !callbacks_active() {
            return Ok(());
        }
        let mut guard = RUNTIME.try_lock().map_err(|_| "FunRounds: runtime busy or poisoned")?;
        let runtime = guard.as_mut().ok_or("FunRounds: runtime unavailable")?;
        if !runtime.on_game_thread() {
            return Err("FunRounds: command is not on the game thread".into());
        }
        let args = args.iter().map(ToString::to_string).collect::<Vec<_>>();
        let parsed = commands::Command::parse(&args).map_err(|key| runtime.text(slot, key, &[]));
        // A client bind has Console context; wire decisions and errors must still be visible in game.
        let wire_reply = slot >= 0 && args.first().is_some_and(|name| name == "fr_wire");
        let status = matches!(&parsed, Ok(commands::Command::Status));
        let sync_touch = matches!(&parsed, Ok(commands::Command::Enable(_)));
        let response = parsed.and_then(|command| {
            if slot != -1 && !command.permits_player() {
                return Err("management commands require server console or RCON".into());
            }
            runtime.command(command, slot)
        });
        let message = response.unwrap_or_else(|error| runtime.text(slot, "feedback.error", &[("error", &error)]));
        if sync_touch {
            sync_chicken_follow_blocking(runtime);
            legacy_textmsg::set_enabled(runtime.legacy_textmsg_enabled());
        }
        drop(guard);
        if sync_touch {
            sync_hostage_rescue_touch();
        }
        if wire_reply {
            i18n::send_chat(slot, &message);
        } else if status {
            for part in commands::status_reply_parts(&message) {
                s2sdk::ReplyToCommand(context, slot, &Str::from_str(part));
            }
        } else {
            for part in i18n::console_parts(&message) {
                s2sdk::ReplyToCommand(context, slot, &Str::from_str(part));
            }
        }
        Ok(())
    }));
    if !matches!(result, Ok(Ok(()))) {
        contain_failure("command callback failed");
    }
    ResultType::Handled
}

#[cfg(test)]
mod tests {
    use super::*;

    static CALLBACK_TEST: Mutex<()> = Mutex::new(());

    #[test]
    fn quiet_event_logs_keep_round_transitions_without_per_player_samples() {
        for name in ["round_start", "round_freeze_end", "round_end"] {
            assert!(event_log_enabled(name, false));
        }
        for name in ["weapon_fire", "player_hurt", "player_death", "player_spawn", "grenade_thrown"] {
            assert!(!event_log_enabled(name, false));
            assert!(event_log_enabled(name, true));
        }
    }

    #[test]
    fn disabled_event_probes_do_not_access_the_sdk_or_event_pointer() {
        probe_event_players("player_spawn", usize::MAX, false);
        probe_event_players("round_freeze_end", usize::MAX, false);
    }

    #[test]
    fn damage_panic_queues_a_global_stop_without_relocking_the_held_runtime() {
        let _serial = CALLBACK_TEST.lock().unwrap();
        let runtime = RUNTIME.lock().unwrap();
        assert!(runtime.is_none());
        let was_active = ACTIVE.swap(true, Ordering::AcqRel);
        let was_pending = PANIC_PENDING.swap(false, Ordering::AcqRel);
        let mut multiplier = 9.0;
        let mut headshot = true;
        assert_eq!(
            damage_decision(
                damage::Damage {
                    victim: 1,
                    attacker: 2,
                    inflictor: 2,
                    weapon_kind: 0,
                    hit_group: 0,
                    traced_head: false,
                    amount: 1.0,
                    damage_type: 0,
                    weapon: -1,
                    weapon_class: None,
                    skip_plugin_rules: false,
                },
                &mut multiplier,
                &mut headshot
            ),
            1
        );
        assert_eq!(multiplier, 1.0);
        assert!(!headshot);
        assert!(PANIC_PENDING.load(Ordering::Acquire));
        assert!(ACTIVE.load(Ordering::Acquire));
        assert!(!callbacks_active());
        drop(runtime);
        apply_pending_fault().unwrap();
        assert!(!PANIC_PENDING.load(Ordering::Acquire));
        ACTIVE.store(was_active, Ordering::Release);
        PANIC_PENDING.store(was_pending, Ordering::Release);
    }

    #[test]
    fn boundary_panic_recovers_the_poisoned_mutex_before_the_next_cleanup() {
        let _serial = CALLBACK_TEST.lock().unwrap();
        let was_pending = PANIC_PENDING.swap(false, Ordering::AcqRel);
        assert!(
            boundary(|| {
                let runtime = RUNTIME.lock().unwrap();
                assert!(runtime.is_none());
                panic!("fixture failure with runtime held");
            })
            .is_err()
        );
        assert!(RUNTIME.is_poisoned());
        assert!(PANIC_PENDING.load(Ordering::Acquire));
        apply_pending_fault().unwrap();
        assert!(!RUNTIME.is_poisoned());
        assert!(!PANIC_PENDING.load(Ordering::Acquire));
        PANIC_PENDING.store(was_pending, Ordering::Release);
    }

    #[test]
    fn missing_event_diagnostics_identify_the_actual_subscription() {
        let hook = EVENTS.iter().position(|name| *name == "bomb_beginplant").unwrap();
        let probe = subscribed_events(1 << hook, &["bomb_beginplant", "bomb_planted"]);
        let mut reasons = Vec::new();
        assert!(!probe.record("bomb_timing", &mut reasons));
        assert_eq!(reasons, ["bomb_timing:[event:bomb_planted]"]);
        let hook = EVENTS.iter().position(|name| *name == "round_prestart").unwrap();
        assert!(subscribed_events(1 << hook, &["round_prestart"]).available());
        assert!(!subscribed_events(!(1 << hook), &["round_prestart"]).record("team_deathmatch", &mut reasons));
        assert_eq!(reasons[1], "team_deathmatch:[event:round_prestart]");
    }

    #[test]
    fn shared_touch_ignores_unowned_entities_before_acquiring_runtime() {
        let _serial = CALLBACK_TEST.lock().unwrap();
        let _runtime = RUNTIME.lock().unwrap();
        let was_active = ACTIVE.swap(true, Ordering::AcqRel);
        assert_eq!(unsafe { on_touch(0x12345678, -1) }, 0);
        assert!(ACTIVE.load(Ordering::Acquire));
        ACTIVE.store(was_active, Ordering::Release);
    }
}
