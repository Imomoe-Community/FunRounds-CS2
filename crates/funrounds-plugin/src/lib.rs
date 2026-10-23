// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

mod bombs;
mod capability;
mod commands;
mod damage;
mod disconnect;
mod effects;
mod fault;
#[allow(dead_code)]
mod funrounds_native;
mod game;
mod parameters;
mod preferences;
mod projectiles;
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
use std::sync::{Mutex, TryLockError};

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
    "bomb_beginplant",
    "bomb_abortplant",
    "bomb_planted",
    "bomb_begindefuse",
    "bomb_abortdefuse",
    "bomb_exploded",
    "round_prestart",
];
static ACTIVE: AtomicBool = AtomicBool::new(false);
static PANIC_PENDING: AtomicBool = AtomicBool::new(false);
static PENDING: AtomicBool = AtomicBool::new(false);
static HOOKS: AtomicU32 = AtomicU32::new(0);
static COMMANDS: AtomicU32 = AtomicU32::new(0);
static CHAT_HOOKED: AtomicBool = AtomicBool::new(false);
static CAN_ACQUIRE_HOOKED: AtomicBool = AtomicBool::new(false);
static TOUCH_HOOKED: AtomicBool = AtomicBool::new(false);
static KNIFE_DEATH_PRE_HOOKED: AtomicBool = AtomicBool::new(false);
static RUNTIME: Mutex<Option<runtime::Runtime>> = Mutex::new(None);
static DISCONNECT_BRIDGE: Mutex<Option<disconnect::Bridge>> = Mutex::new(None);
static DAMAGE_BRIDGE: Mutex<Option<damage::Bridge>> = Mutex::new(None);

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
            "FunRounds requires funrounds_native 0.2.0-alpha.test ABI; missing imports: {}",
            missing.join(", ")
        )
    })
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

fn start() -> Result<(), Box<dyn Error>> {
    boundary(|| {
        if HOOKS.load(Ordering::Acquire) != 0
            || COMMANDS.load(Ordering::Acquire) != 0
            || CHAT_HOOKED.load(Ordering::Acquire)
            || TOUCH_HOOKED.load(Ordering::Acquire)
            || KNIFE_DEATH_PRE_HOOKED.load(Ordering::Acquire)
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
        PENDING.store(true, Ordering::Release);
        s2sdk::PrintToServer(&Str::from_str("[FunRounds] waiting for first game simulation before subscribing"));
        Ok(())
    })
}

fn update(_dt: f32) -> Result<(), Box<dyn Error>> {
    boundary(|| {
        if PENDING.swap(false, Ordering::AcqRel) {
            subscribe()?;
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
                drop(guard);
                for action in actions {
                    match action {
                        action @ runtime::EngineAction::Rollback(..) => process_rollback_action(action),
                        action @ (runtime::EngineAction::Loadout(..) | runtime::EngineAction::Upgrade(..)) => process_loadout_action(action),
                        runtime::EngineAction::GrantC4(task, player) => {
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
                        runtime::EngineAction::LowHealthFinish(task, player, numeric) => {
                            execute_valid_action(runtime::EngineAction::LowHealthFinish(task, player, numeric), || {
                                let slot = task.player.slot;
                                let handle = player.pawn_handle;
                                let zero = plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 };
                                let applied = if numeric {
                                    damage::finish_self_damage(handle)?
                                } else {
                                    funrounds_native::TakeEntityDamageWithWeapon(
                                        handle,
                                        handle,
                                        handle,
                                        &zero,
                                        &zero,
                                        1.0,
                                        damage::DMG_HEADSHOT as i32,
                                        -1,
                                        false,
                                    )
                                };
                                s2sdk::PrintToServer(&Str::from_str(&format!(
                                    "[FunRounds] low-health finish slot={slot} pawn={handle} damage={applied}"
                                )));
                                Ok(())
                            })?
                        }
                        runtime::EngineAction::CenterDisplay { generation, text } => execute_valid_action(
                            runtime::EngineAction::CenterDisplay {
                                generation,
                                text: text.clone(),
                            },
                            || {
                                s2sdk::PrintCentreHtmlAll(&Str::from_str(&text), 5);
                                Ok(())
                            },
                        )?,
                        runtime::EngineAction::EndRound(round, reason) => {
                            execute_valid_action(runtime::EngineAction::EndRound(round, reason), || {
                                finish_team_round(reason);
                                Ok(())
                            })?
                        }
                    }
                }
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
    let Some((mut effect, mode)) = prepared else { return };
    let result = catch_unwind(AssertUnwindSafe(|| match &action {
        runtime::EngineAction::Loadout(_, _, equipment) => effect.apply(&mode, equipment, || engine_action_current(&action)),
        runtime::EngineAction::Upgrade(_, player, target, weapon) => {
            let target = effect
                .upgraded_weapon()
                .filter(|(handle, class)| {
                    s2sdk::IsValidEntHandle(*handle)
                        && s2sdk::GetEntityOwner(*handle) == player.pawn_handle
                        && s2sdk::GetClientWeapons(player.slot).iter().any(|owned| owned == handle)
                        && s2sdk::GetEntityClassname(*handle).as_str() == class
                })
                .map_or(*target, |(handle, _)| handle);
            effect
                .replace_upgrade_weapon(target, std::slice::from_ref(weapon), &mode, || engine_action_current(&action))
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
        suicide::Action::Purchase { player, .. } => {
            let existing = {
                let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.as_ref().and_then(|runtime| runtime.existing_suicide_c4(player))
            };
            let result = existing.map(Ok).unwrap_or_else(|| {
                suicide::purchase(
                    player,
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
                let message = if s2sdk::GetClientTeam(owned.player.slot) as i32 == 2 {
                    "[FunRounds] 自爆已启动，五秒后爆炸！"
                } else {
                    "[FunRounds] CT正在销毁C4！"
                };
                s2sdk::PrintToChatAll(&Str::from_str(message));
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
            if let Err(error) = suicide::feedback(action.player()) {
                suicide_fault(error);
            }
            for (task, player, amount) in targets {
                let valid = {
                    let guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.as_ref().is_some_and(|runtime| {
                        runtime.suicide_target_valid(task, player) && runtime.action_valid(&runtime::EngineAction::Suicide(action))
                    })
                };
                if valid && let Err(error) = suicide::hit(action.player(), player, amount) {
                    suicide_fault(error);
                    break;
                }
            }
            let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(runtime) = guard.as_mut() {
                runtime.finish_suicide_blast();
            }
        }
        suicide::Action::Finish { player, amount, .. } => {
            if let Err(error) = suicide::hit(player, player, amount) {
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
            let result = s2sdk::HookEvent(&Str::from_str(event), on_event, HookMode::Post);
            if result != EventHookError::Okay {
                if *event == "round_prestart" {
                    s2sdk::PrintToServer(&Str::from_str(
                        "[FunRounds] round_prestart unavailable; chicken-bomb and team-deathmatch modes excluded",
                    ));
                    continue;
                } else if index >= 11 {
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
        *RUNTIME.lock().map_err(|_| "FunRounds: runtime poisoned")? = Some(runtime::Runtime::new(runtime::EventCapabilities {
            grenade_thrown: hooks & (1 << 6) != 0,
            weapon_fire: hooks & (1 << 7) != 0,
            player_hurt: hooks & (1 << 8) != 0,
            damage: damage_ready,
            combat_events: subscribed_events(hooks, &["player_spawn", "player_death"]).available(),
            round_prestart: subscribed_events(hooks, &["round_prestart"]).available(),
            bombs: subscribed_events(
                hooks,
                &[
                    "bomb_beginplant",
                    "bomb_abortplant",
                    "bomb_planted",
                    "bomb_begindefuse",
                    "bomb_abortdefuse",
                ],
            ),
            moving_blast,
        })?);
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
    let mut failed = Vec::new();
    if CAN_ACQUIRE_HOOKED.load(Ordering::Acquire) {
        if funrounds_native::UnhookPlayerCanAcquire(on_can_acquire) {
            CAN_ACQUIRE_HOOKED.store(false, Ordering::Release);
        } else {
            failed.push("suicide pickup unsubscription failed".into());
        }
    }
    if TOUCH_HOOKED.load(Ordering::Acquire) {
        if funrounds_native::UnhookEntityTouchPre(on_touch) {
            TOUCH_HOOKED.store(false, Ordering::Release);
            touch::clear();
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
        let (actions, equipment, projectiles, rollbacks, error) = {
            let mut guard = RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(runtime) = guard.as_mut() else { break };
            let error = runtime.shutdown().err();
            (
                runtime.take_bomb_actions(),
                runtime.take_equipment_cleanup(),
                runtime.take_projectile_cleanup(),
                runtime.take_rollbacks(),
                error,
            )
        };
        shutdown_error = error;
        if actions.is_empty() && equipment.is_empty() && projectiles.is_empty() && rollbacks.is_empty() {
            shutdown_settled = true;
            break;
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
        let result = s2sdk::UnhookEvent(&Str::from_str(event), on_event, HookMode::Post);
        if result == EventHookError::Okay {
            HOOKS.fetch_and(!bit, Ordering::Release);
        } else {
            failed.push(format!("{event}: {result:?}"));
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
    let hooked = funrounds_native::HookEntityTouchPre(handle, on_touch);
    if hooked {
        TOUCH_HOOKED.store(true, Ordering::Release);
    }
    hooked
}

unsafe extern "C" fn on_touch(entity: i32, other: i32) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        // The native detour covers a shared Touch implementation, including ordinary weapons.
        if !callbacks_active() || !touch::owns(entity) {
            return 0;
        }
        let mut guard = RUNTIME.try_lock().expect("FunRounds runtime busy in touch callback");
        let Some(runtime) = guard.as_mut() else { return 0 };
        assert!(runtime.on_game_thread(), "FunRounds touch callback is not on game thread");
        i32::from(runtime.projectile_touch(entity, other))
    }))
    .unwrap_or_else(|_| {
        contain_failure("touch callback panic contained");
        1
    })
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
        let (diagnostics, bomb_cleanup) = if let Ok(mut guard) = RUNTIME.try_lock() {
            if let Some(runtime) = guard.as_mut() {
                if !runtime.on_game_thread() {
                    panic!("FunRounds event is not on the game thread");
                }
                runtime.event(&name, event);
                let cleanup = if name == "round_prestart" {
                    runtime.take_bomb_actions()
                } else {
                    Vec::new()
                };
                (runtime.diagnostics(), cleanup)
            } else {
                (false, Vec::new())
            }
        } else {
            panic!("FunRounds runtime busy or poisoned in event callback");
        };
        // Submit old helpers before the native round reset flushes entity deletions.
        for action in bomb_cleanup {
            process_bomb_action(action);
        }
        if event_log_enabled(&name, diagnostics) {
            s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] event: {name}")));
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

fn damage_decision(mut context: damage::Damage, multiplier: &mut f32, mark_headshot: &mut bool) -> i32 {
    *multiplier = 1.0;
    *mark_headshot = false;
    catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active() {
            return 0;
        }
        if damage::apply_internal_damage(&context, mark_headshot) {
            return 0;
        }
        let mut guard = RUNTIME.try_lock().expect("FunRounds damage runtime busy or poisoned");
        let Some(runtime) = guard.as_mut() else { return 0 };
        assert!(runtime.on_game_thread(), "FunRounds damage callback is not on game thread");
        context.weapon_class = damage::capture_weapon_class(context.weapon, context.inflictor, context.weapon_kind);
        if runtime.blocks_native_bomb_damage(context.inflictor) {
            return 1;
        }
        let Some((projectile_scale, projectile_head)) = runtime.projectile_damage(&mut context) else {
            return 1;
        };
        let headshots = runtime.headshots();
        if headshots == funrounds_core::Headshots::Normal && !runtime.classic_round() {
            *mark_headshot = projectile_head;
            return 0;
        }
        if !s2sdk::IsValidEntHandle(context.victim) || !context.amount.is_finite() || context.amount < 0.0 {
            return 1;
        }
        let players = game::DamagePlayers::resolve(context.victim, context.attacker);
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
        let mut effective_damage = context.amount * combined_scale;
        let mut damage_scale = combined_scale;
        let mut random_heal = 0;
        if let Some(result) = runtime.random_damage_roll(&context, &players, effective_damage) {
            effective_damage = result.amount;
            random_heal = result.attacker_heal;
            damage_scale = effective_damage / context.amount;
        }
        if !effective_damage.is_finite() || effective_damage == 0.0 {
            return 1;
        }
        if runtime.delay_damage_with_heal(
            &context,
            &players,
            effective_damage,
            delayed_headshot_marker(&context, attributed, headshot, projectile_head),
            random_heal,
        ) {
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
        if !callbacks_active() || slot < 0 || !s2sdk::IsClientInGame(slot) {
            return ResultType::Continue;
        }
        let args = args.iter().map(ToString::to_string).collect::<Vec<_>>();
        let Some(command) = commands::parse_chat(&args, context == ConCommandContext::Chat) else {
            return ResultType::Continue;
        };
        let mut guard = RUNTIME.try_lock().expect("FunRounds chat runtime busy or poisoned");
        let Some(runtime) = guard.as_mut() else { return ResultType::Continue };
        assert!(runtime.on_game_thread(), "FunRounds chat is not on the game thread");
        if !runtime.classic_enabled() {
            return ResultType::Continue;
        }
        let message = command
            .map_err(str::to_owned)
            .and_then(|command| runtime.command(command, slot))
            .unwrap_or_else(|error| format!("[FunRounds] {error}"));
        drop(guard);
        for line in message.lines() {
            s2sdk::PrintToChat(slot, &Str::from_str(line));
        }
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
        let parsed = commands::Command::parse(&args).map_err(String::from);
        let status = matches!(&parsed, Ok(commands::Command::Status));
        let response = parsed.and_then(|command| {
            if slot != -1 && !command.permits_player() {
                return Err("management commands require server console or RCON".into());
            }
            runtime.command(command, slot)
        });
        let message = response.unwrap_or_else(|error| format!("[FunRounds] error: {error}"));
        drop(guard);
        if status {
            for part in commands::status_reply_parts(&message) {
                s2sdk::ReplyToCommand(context, slot, &Str::from_str(part));
            }
        } else {
            s2sdk::ReplyToCommand(context, slot, &Str::from_str(&message));
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
