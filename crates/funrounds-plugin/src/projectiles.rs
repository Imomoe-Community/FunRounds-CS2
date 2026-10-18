// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::{Probe, imports},
    damage::Damage,
    fault::{Fault, Mechanism},
    game::{LivePlayer, entity_handles},
    s2sdk,
};
use funrounds_core::{PlayerTask, PlayerTracker, RoundGeneration};
use plugify::{Str, Vec3};
use std::collections::{HashMap, HashSet};

const KNIFE_ENTITY: &str = "smokegrenade_projectile";
const CONTENTS_PLAYER: u64 = 1 << 18;
const DECOY_PROJECTILE: &str = "decoy_projectile";
const DECOY_THROWER_CLASS: &str = "CBaseGrenade";
const ELASTICITY_CLASS: &str = "CBaseEntity";
const ELASTICITY_FIELD: &str = "m_flElasticity";
const DECOY_THROW_WINDOW: f32 = 0.5;

macro_rules! imports_available {
    ($($name:ident),+ $(,)?) => {{
        true $(&& unsafe { std::ptr::addr_of!(s2sdk::$name).read().is_some() })+
    }};
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dodgeball,
    Knife,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub task: PlayerTask,
    pub projectile: i32,
    pub victim: LivePlayer,
    pub victim_task: PlayerTask,
    pub kind: Kind,
    pub amount: f32,
    pub headshot: bool,
    pub force: Vec3,
    pub hit_position: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Claim {
    Ignore,
    Block { projectile: i32, remove: bool },
    Apply(Impact),
}

#[derive(Clone, Copy)]
struct Tracked {
    task: PlayerTask,
    kind: Kind,
    expires_at: Option<f32>,
    claimed: bool,
}

#[derive(Clone, Copy)]
struct PendingDecoy {
    task: PlayerTask,
    expires_at: f32,
}

#[derive(Clone, Copy)]
struct RecentDecoy {
    handle: i32,
    task: PlayerTask,
    seen_at: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnedKnife {
    pub task: PlayerTask,
    pub handle: i32,
    pub expires_at: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    SpawnKnife { task: PlayerTask, player: LivePlayer },
    Remove { handle: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    id: u64,
    map_epoch: u64,
    pub kind: ActionKind,
}

pub enum ActionResult {
    Spawned {
        created: Option<i32>,
        result: Result<SpawnedKnife, Fault>,
    },
    Removed(bool),
    Cancelled,
}

#[derive(Default)]
pub struct Projectiles {
    tracked: HashMap<i32, Tracked>,
    retired: HashSet<i32>,
    baseline_decoys: HashSet<i32>,
    pending_knives: Vec<(PlayerTask, i32)>,
    seen_decoys: HashSet<i32>,
    pending_decoys: Vec<PendingDecoy>,
    recent_decoys: Vec<RecentDecoy>,
    active_impact: Option<Impact>,
    next_action: u64,
    map_epoch: u64,
    pending_actions: HashMap<u64, Action>,
    removing: HashSet<i32>,
}

impl Projectiles {
    pub fn discard_map_state(&mut self) {
        crate::touch::clear();
        *self = Self {
            next_action: self.next_action,
            map_epoch: self.map_epoch.checked_add(1).expect("projectile map epoch exhausted"),
            ..Self::default()
        };
    }

    pub fn capabilities(damage_hook: bool) -> (Probe, Probe) {
        let mut decoys = Probe::default();
        decoys.require(damage_hook, "hook:entity_damage");
        imports!(decoys;
            __s2sdk_GetFirstActiveEntity, __s2sdk_GetNextActiveEntity, __s2sdk_GetEntityClassname,
            __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer, __s2sdk_GetEntSchemaEnt2,
            __s2sdk_GetEntityAbsOrigin, __s2sdk_GetEntityAbsVelocity, __s2sdk_GetClientHealth,
            __s2sdk_TakeEntityDamage, __s2sdk_RemoveEntity,
        );
        decoys.schema(DECOY_THROWER_CLASS, "m_hThrower");
        let mut knives = decoys.clone();
        imports!(knives;
            __s2sdk_CreateEntityByName, __s2sdk_DispatchSpawn2, __s2sdk_SetEntityModel,
            __s2sdk_SetEntityOwner, __s2sdk_SetEntityGravity, __s2sdk_SetEntityMoveType,
            __s2sdk_GetEntityMoveType, __s2sdk_SetEntityDefaultCollisionGroup, __s2sdk_SetEntityAngVelocity,
            __s2sdk_TeleportEntity, __s2sdk_GetEntSchemaEnt2, __s2sdk_SetEntSchemaEnt2,
            __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2, __s2sdk_SetEntSchemaFloat2, __s2sdk_GetEntSchemaFloat2,
            __s2sdk_GetClientEyePosition, __s2sdk_GetClientEyeAngles, __s2sdk_GetClientAbsVelocity, __s2sdk_GetClientTeam,
        );
        for (class, field) in [
            (ELASTICITY_CLASS, ELASTICITY_FIELD),
            ("CBaseEntity", "m_iTeamNum"),
            ("CBaseEntity", "m_nNextThinkTick"),
            ("CBaseEntity", "m_pCollision"),
            ("CCollisionProperty", "m_collisionAttribute"),
            ("VPhysicsCollisionAttribute_t", "m_nInteractsWith"),
        ] {
            knives.schema(class, field);
        }
        knives.include(crate::touch::probe());
        (decoys, knives)
    }

    pub fn baseline_decoys(&mut self) {
        self.recent_decoys.clear();
        self.pending_decoys.clear();
        let existing = entity_handles(DECOY_PROJECTILE);
        self.baseline_decoys.extend(existing.iter().copied());
        self.seen_decoys.extend(existing);
    }

    pub fn observe_decoy_throw(&mut self, task: PlayerTask, now: f32) {
        if !now.is_finite() || self.pending_decoys.iter().any(|pending| pending.task == task) {
            return;
        }
        let mut matched = false;
        for candidate in &self.recent_decoys {
            if candidate.task == task && now - candidate.seen_at <= DECOY_THROW_WINDOW {
                self.tracked.insert(
                    candidate.handle,
                    Tracked {
                        task,
                        kind: Kind::Dodgeball,
                        expires_at: None,
                        claimed: false,
                    },
                );
                matched = true;
            }
        }
        self.recent_decoys.retain(|candidate| !(matched && candidate.task == task));
        if !matched {
            self.pending_decoys.push(PendingDecoy {
                task,
                expires_at: now + DECOY_THROW_WINDOW,
            });
        }
    }

    // Poll only entity identity and ownership. The actual collision remains the native damage hook.
    pub fn discover_decoys(&mut self, now: f32, round: RoundGeneration, players: &PlayerTracker) {
        if !now.is_finite() {
            return;
        }
        self.seen_decoys.retain(|handle| s2sdk::IsValidEntHandle(*handle));
        self.baseline_decoys.retain(|handle| s2sdk::IsValidEntHandle(*handle));
        self.pending_decoys
            .retain(|pending| pending.expires_at >= now && valid_task(pending.task, round, players).is_some());
        self.recent_decoys
            .retain(|candidate| now - candidate.seen_at <= DECOY_THROW_WINDOW && valid_task(candidate.task, round, players).is_some());

        for handle in entity_handles(DECOY_PROJECTILE) {
            if !self.seen_decoys.insert(handle) || self.tracked.contains_key(&handle) {
                continue;
            }
            let Some(thrower) = decoy_thrower(handle) else { continue };
            if let Some(index) = self
                .pending_decoys
                .iter()
                .position(|pending| pending.task.player.pawn_handle as i32 == thrower && valid_task(pending.task, round, players).is_some())
            {
                let pending = self.pending_decoys.remove(index);
                self.tracked.insert(
                    handle,
                    Tracked {
                        task: pending.task,
                        kind: Kind::Dodgeball,
                        expires_at: None,
                        claimed: false,
                    },
                );
            } else if let Some(task) = current_thrower_task(handle, round, players) {
                self.recent_decoys.push(RecentDecoy { handle, task, seen_at: now });
            }
        }
    }

    pub fn schedule_knife(&mut self, task: PlayerTask, fire_tick: i32) {
        if !self.pending_knives.iter().any(|(pending, _)| *pending == task) {
            self.pending_knives.push((task, fire_tick));
        }
    }

    pub fn take_due_knives(&mut self, tick: i32, round: RoundGeneration, players: &PlayerTracker) -> Vec<Action> {
        let mut due = Vec::new();
        let mut pending = Vec::new();
        for (task, fire_tick) in std::mem::take(&mut self.pending_knives) {
            if tick <= fire_tick {
                pending.push((task, fire_tick));
                continue;
            }
            if let Some(player) = valid_task(task, round, players) {
                due.push(self.action(ActionKind::SpawnKnife { task, player }));
            }
        }
        self.pending_knives = pending;
        due
    }

    fn register_knife(&mut self, spawned: SpawnedKnife) -> bool {
        if !s2sdk::IsValidEntHandle(spawned.handle) || self.tracked.contains_key(&spawned.handle) {
            return false;
        }
        self.tracked.insert(
            spawned.handle,
            Tracked {
                task: spawned.task,
                kind: Kind::Knife,
                expires_at: Some(spawned.expires_at),
                claimed: false,
            },
        );
        true
    }

    fn action(&mut self, kind: ActionKind) -> Action {
        self.next_action = self.next_action.checked_add(1).expect("projectile action sequence exhausted");
        let action = Action {
            id: self.next_action,
            map_epoch: self.map_epoch,
            kind,
        };
        self.pending_actions.insert(action.id, action);
        action
    }

    pub fn action_current(&self, action: Action) -> bool {
        action.map_epoch == self.map_epoch && self.pending_actions.get(&action.id) == Some(&action)
    }

    pub fn action_finished(&mut self, action: Action, result: ActionResult, accept_spawn: bool) -> Result<(), Fault> {
        if !self.action_current(action) {
            return Ok(());
        }
        self.pending_actions.remove(&action.id);
        match (action.kind, result) {
            (ActionKind::SpawnKnife { task, .. }, ActionResult::Spawned { created, result }) => {
                if let Ok(knife) = result.as_ref()
                    && accept_spawn
                    && (knife.task != task || created != Some(knife.handle) || self.tracked.contains_key(&knife.handle))
                {
                    self.retired.extend(created);
                    return Err(Fault::global("knife creation returned an unowned or already tracked handle"));
                }
                if let Ok(knife) = result.as_ref()
                    && accept_spawn
                    && self.register_knife(*knife)
                {
                    crate::touch::track(knife.handle);
                    return Ok(());
                }
                self.retired.extend(created);
                if accept_spawn || result.as_ref().is_err_and(|error| error.scope == crate::fault::Scope::Global) {
                    result.map(|_| ())
                } else {
                    Ok(())
                }
            }
            (ActionKind::SpawnKnife { .. }, ActionResult::Cancelled) => Ok(()),
            (ActionKind::Remove { handle }, ActionResult::Removed(gone)) => {
                self.removing.remove(&handle);
                if gone {
                    self.retired.remove(&handle);
                }
                Ok(())
            }
            _ => Err("projectile engine action returned an unexpected result".into()),
        }
    }

    pub fn cleanup_actions(&mut self) -> Vec<Action> {
        self.cleanup_actions_with(s2sdk::IsValidEntHandle)
    }

    fn cleanup_actions_with(&mut self, mut valid: impl FnMut(i32) -> bool) -> Vec<Action> {
        self.retired.retain(|handle| {
            if valid(*handle) {
                true
            } else {
                crate::touch::forget(*handle);
                false
            }
        });
        let handles = self
            .retired
            .iter()
            .copied()
            .filter(|handle| !self.removing.contains(handle))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                self.removing.insert(handle);
                self.action(ActionKind::Remove { handle })
            })
            .collect()
    }

    pub fn is_clean(&self) -> bool {
        self.tracked.is_empty()
            && self.retired.is_empty()
            && self.removing.is_empty()
            && self.pending_actions.is_empty()
            && self.pending_knives.is_empty()
            && self.active_impact.is_none()
    }

    pub fn claim_damage(
        &mut self,
        damage: &Damage,
        round: RoundGeneration,
        players: &PlayerTracker,
        dodgeball_mode: bool,
        throwing_knives_mode: bool,
    ) -> Claim {
        if self.synthetic_impact(damage).is_some() {
            return Claim::Ignore;
        }
        if self.retired.contains(&damage.inflictor) {
            return Claim::Block {
                projectile: damage.inflictor,
                remove: s2sdk::IsValidEntHandle(damage.inflictor),
            };
        }
        if !self.tracked.contains_key(&damage.inflictor) && dodgeball_mode && is_decoy(damage.inflictor) {
            if self.baseline_decoys.contains(&damage.inflictor) {
                self.retired.insert(damage.inflictor);
                return Claim::Block {
                    projectile: damage.inflictor,
                    remove: true,
                };
            }
            let task = decoy_thrower(damage.inflictor).and_then(|thrower| self.pending_decoy_task(thrower, round, players));
            if let Some(task) = task {
                self.pending_decoys.retain(|pending| pending.task != task);
                self.tracked.insert(
                    damage.inflictor,
                    Tracked {
                        task,
                        kind: Kind::Dodgeball,
                        expires_at: None,
                        claimed: false,
                    },
                );
                self.seen_decoys.insert(damage.inflictor);
            } else {
                self.retired.insert(damage.inflictor);
                return Claim::Block {
                    projectile: damage.inflictor,
                    remove: true,
                };
            }
        }
        let Some(projectile) = self.tracked.get_mut(&damage.inflictor) else {
            return Claim::Ignore;
        };
        if !claim_once(&mut projectile.claimed) {
            return Claim::Block {
                projectile: damage.inflictor,
                remove: false,
            };
        }
        let tracked = *projectile;
        let mode_enabled = match tracked.kind {
            Kind::Dodgeball => dodgeball_mode,
            Kind::Knife => throwing_knives_mode,
        };
        if !mode_enabled || !s2sdk::IsValidEntHandle(damage.inflictor) {
            self.retired.insert(damage.inflictor);
            return Claim::Block {
                projectile: damage.inflictor,
                remove: true,
            };
        }
        if valid_task(tracked.task, round, players).is_none() {
            self.retired.insert(damage.inflictor);
            return Claim::Block {
                projectile: damage.inflictor,
                remove: true,
            };
        }
        let Some((victim, victim_task)) = live_player_task_for_handle(damage.victim, round, players) else {
            self.retired.insert(damage.inflictor);
            return Claim::Block {
                projectile: damage.inflictor,
                remove: true,
            };
        };
        let position = s2sdk::GetEntityAbsOrigin(damage.inflictor);
        let velocity = s2sdk::GetEntityAbsVelocity(damage.inflictor);
        if !finite_vec(position) || !finite_vec(velocity) || length_squared(velocity) <= f32::EPSILON {
            self.retired.insert(damage.inflictor);
            return Claim::Block {
                projectile: damage.inflictor,
                remove: true,
            };
        }
        let (amount, headshot) = match tracked.kind {
            Kind::Dodgeball => (funrounds_core::dodgeball_damage(s2sdk::GetClientHealth(victim.slot)), false),
            Kind::Knife => {
                let head =
                    distance_squared(position, s2sdk::GetClientEyePosition(victim.slot)) <= funrounds_core::THROWING_KNIFE_HEADSHOT_DISTANCE.powi(2);
                (
                    if head {
                        funrounds_core::THROWING_KNIFE_HEAD_DAMAGE
                    } else {
                        funrounds_core::THROWING_KNIFE_BODY_DAMAGE
                    },
                    head,
                )
            }
        };
        if !amount.is_finite() || amount <= 0.0 {
            self.retired.insert(damage.inflictor);
            return Claim::Block {
                projectile: damage.inflictor,
                remove: true,
            };
        }
        Claim::Apply(Impact {
            task: tracked.task,
            projectile: damage.inflictor,
            victim,
            victim_task,
            kind: tracked.kind,
            amount,
            headshot,
            force: velocity,
            hit_position: position,
        })
    }

    pub fn claim_touch(
        &mut self,
        entity_handle: i32,
        other_handle: i32,
        round: RoundGeneration,
        players: &PlayerTracker,
        throwing_knives_mode: bool,
    ) -> Claim {
        if self.retired.contains(&entity_handle) {
            return Claim::Block {
                projectile: entity_handle,
                remove: s2sdk::IsValidEntHandle(entity_handle),
            };
        }
        let Some(tracked) = self.tracked.get(&entity_handle).copied() else {
            return Claim::Ignore;
        };
        if tracked.kind != Kind::Knife {
            return Claim::Ignore;
        }
        if tracked.claimed {
            return Claim::Block {
                projectile: entity_handle,
                remove: false,
            };
        }
        if !throwing_knives_mode
            || !s2sdk::IsValidEntHandle(entity_handle)
            || tracked.expires_at.is_some_and(|expires_at| s2sdk::GetGameTime() >= expires_at)
            || valid_task(tracked.task, round, players).is_none()
        {
            self.retired.insert(entity_handle);
            return Claim::Block {
                projectile: entity_handle,
                remove: true,
            };
        }

        // World, prop, and non-player contacts keep their native physics response.
        // Only a live player touch can claim the one damage action.
        let Some((victim, victim_task)) = live_player_task_for_handle(other_handle, round, players) else {
            return Claim::Ignore;
        };
        let position = s2sdk::GetEntityAbsOrigin(entity_handle);
        let velocity = s2sdk::GetEntityAbsVelocity(entity_handle);
        let eye = s2sdk::GetClientEyePosition(victim.slot);
        let Some(impact) = knife_impact(tracked.task, entity_handle, victim, victim_task, position, velocity, eye) else {
            return Claim::Ignore;
        };
        let Some(projectile) = self.tracked.get_mut(&entity_handle) else {
            return Claim::Ignore;
        };
        if !claim_once(&mut projectile.claimed) {
            return Claim::Block {
                projectile: entity_handle,
                remove: false,
            };
        }
        Claim::Apply(impact)
    }

    // The native damage action can synchronously re-enter the damage hook. Match only that exact pair.
    pub fn begin_native_impact(&mut self, impact: Impact) -> bool {
        if self.active_impact.is_some()
            || !self
                .tracked
                .get(&impact.projectile)
                .is_some_and(|tracked| tracked.claimed && tracked.task == impact.task)
        {
            return false;
        }
        self.active_impact = Some(impact);
        true
    }

    pub fn synthetic_impact(&self, damage: &Damage) -> Option<Impact> {
        self.active_impact.filter(|impact| impact_matches_damage(*impact, damage))
    }

    pub fn active_knife_death(&self, victim: i32, attacker: i32) -> bool {
        self.active_impact.is_some_and(|impact| {
            impact.kind == Kind::Knife
                && impact.victim.slot == victim
                && impact.victim_task.player.slot == victim
                && impact.task.player.slot == attacker
        })
    }

    pub fn finish_native_impact(&mut self, impact: Impact) {
        if self.active_impact == Some(impact) {
            self.active_impact = None;
        }
        let handle = self
            .tracked
            .get(&impact.projectile)
            .is_some_and(|tracked| tracked.task == impact.task && tracked.claimed)
            .then(|| self.tracked.remove(&impact.projectile))
            .flatten()
            .map(|_| impact.projectile);
        if handle.is_some() && s2sdk::IsValidEntHandle(impact.projectile) {
            self.retired.insert(impact.projectile);
        }
    }

    pub fn sweep(&mut self, now: f32, round: RoundGeneration, players: &PlayerTracker) {
        let active_handle = self.active_impact.map(|impact| impact.projectile);
        let stale = self
            .tracked
            .iter()
            .filter_map(|(handle, tracked)| {
                let expired = tracked.expires_at.is_some_and(|deadline| now >= deadline);
                let invalid_task = valid_task(tracked.task, round, players).is_none();
                (!tracked.claimed && (expired || invalid_task || !s2sdk::IsValidEntHandle(*handle))).then_some(*handle)
            })
            .collect::<Vec<_>>();
        for handle in stale {
            if self.tracked.remove(&handle).is_some() {
                if s2sdk::IsValidEntHandle(handle) && Some(handle) != active_handle {
                    self.retired.insert(handle);
                } else {
                    crate::touch::forget(handle);
                    self.retired.remove(&handle);
                }
            }
        }
        self.pending_knives.retain(|(task, _)| valid_task(*task, round, players).is_some());
    }

    fn pending_decoy_task(&self, thrower: i32, round: RoundGeneration, players: &PlayerTracker) -> Option<PlayerTask> {
        self.pending_decoys
            .iter()
            .find(|pending| pending.task.player.pawn_handle as i32 == thrower && task_matches_tracker(pending.task, round, players))
            .map(|pending| pending.task)
    }

    fn retire_decoys(&mut self, slot: i32, handles: impl IntoIterator<Item = i32>) {
        self.pending_decoys.retain(|pending| pending.task.player.slot != slot);
        let recent = self
            .recent_decoys
            .iter()
            .filter(|candidate| candidate.task.player.slot == slot)
            .map(|candidate| candidate.handle);
        self.retired.extend(recent);
        self.recent_decoys.retain(|candidate| candidate.task.player.slot != slot);
        self.retired.extend(handles);
    }

    pub fn retire_player(&mut self, slot: i32, pawn: Option<u32>) {
        let undiscovered = if pawn.is_some()
            && imports_available!(
                __s2sdk_GetFirstActiveEntity,
                __s2sdk_GetNextActiveEntity,
                __s2sdk_GetEntityClassname,
                __s2sdk_GetEntSchemaEnt2,
            ) {
            entity_handles(DECOY_PROJECTILE)
                .into_iter()
                .filter(|handle| decoy_thrower(*handle).is_some_and(|thrower| Some(thrower as u32) == pawn))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        self.retire_decoys(slot, undiscovered);
        self.pending_knives.retain(|(task, _)| task.player.slot != slot);
        let handles = self
            .tracked
            .iter()
            .filter_map(|(handle, tracked)| (tracked.task.player.slot == slot).then_some(*handle))
            .collect::<Vec<_>>();
        self.retire_handles(handles)
    }

    pub fn clear(&mut self) {
        self.pending_knives.clear();
        self.pending_decoys.clear();
        self.recent_decoys.clear();
        self.seen_decoys.clear();
        self.baseline_decoys.clear();
        let handles = self.tracked.keys().copied().collect::<Vec<_>>();
        self.retire_handles(handles)
    }

    pub fn retire(&mut self, handle: i32) -> bool {
        self.tracked.remove(&handle);
        self.seen_decoys.remove(&handle);
        self.baseline_decoys.remove(&handle);
        self.recent_decoys.retain(|candidate| candidate.handle != handle);
        if s2sdk::IsValidEntHandle(handle) {
            self.retired.insert(handle);
            false
        } else {
            crate::touch::forget(handle);
            self.retired.remove(&handle);
            true
        }
    }

    fn retire_handles(&mut self, handles: Vec<i32>) {
        for handle in handles {
            self.tracked.remove(&handle);
            self.seen_decoys.remove(&handle);
            self.baseline_decoys.remove(&handle);
            self.recent_decoys.retain(|candidate| candidate.handle != handle);
            if s2sdk::IsValidEntHandle(handle) {
                self.retired.insert(handle);
            } else {
                crate::touch::forget(handle);
                self.retired.remove(&handle);
            }
        }
    }
}

pub fn execute(action: Action, mut current: impl FnMut() -> bool) -> ActionResult {
    match action.kind {
        ActionKind::SpawnKnife { task, player } => {
            if !current() {
                return ActionResult::Cancelled;
            }
            let mut created = None;
            let result = spawn_knife(task, player, &mut created, current);
            ActionResult::Spawned { created, result }
        }
        ActionKind::Remove { handle } => {
            if current() {
                remove_entity(handle);
            }
            ActionResult::Removed(!s2sdk::IsValidEntHandle(handle))
        }
    }
}

fn spawn_knife(task: PlayerTask, player: LivePlayer, created: &mut Option<i32>, mut current: impl FnMut() -> bool) -> Result<SpawnedKnife, Fault> {
    if task.player.slot != player.slot || task.player.pawn_handle != player.pawn_handle as u32 || LivePlayer::resolve(player.slot) != Some(player) {
        return Err(Fault::cancelled("player identity changed before knife spawn"));
    }
    if unsafe { s2sdk::__s2sdk_TakeEntityDamage }.is_none() {
        return Err(Fault::mechanism(Mechanism::ProjectileDamage, "TakeEntityDamage unavailable"));
    }
    if !Projectiles::capabilities(true).1.available() {
        return Err(Fault::mechanism(
            Mechanism::ThrowingKnives,
            "throwing-knife entity APIs or physics schemas unavailable",
        ));
    }

    let angles = s2sdk::GetClientEyeAngles(player.slot);
    let direction = forward(angles);
    let eye = s2sdk::GetClientEyePosition(player.slot);
    let origin = add(eye, scale(direction, 50.0));
    let velocity = add(
        scale(direction, funrounds_core::THROWING_KNIFE_SPEED),
        s2sdk::GetClientAbsVelocity(player.slot),
    );
    let model = match s2sdk::GetClientTeam(player.slot) {
        s2sdk::CSTeam::T => "weapons/models/knife/knife_default_t/weapon_knife_default_t.vmdl",
        _ => "weapons/models/knife/knife_default_ct/weapon_knife_default_ct.vmdl",
    };

    let handle = s2sdk::CreateEntityByName(&Str::from_str(KNIFE_ENTITY));
    if !s2sdk::IsValidEntHandle(handle) {
        return Err(Fault::mechanism(
            Mechanism::ThrowingKnives,
            "could not create throwing-knife physics entity",
        ));
    }
    *created = Some(handle);
    if !current() {
        return Err(Fault::cancelled("throwing-knife task cancelled during creation"));
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 {
        return Err(Fault::mechanism(
            Mechanism::ThrowingKnives,
            "throwing-knife grenade pointer unavailable before spawn",
        ));
    }
    s2sdk::SetEntityOwner(handle, player.pawn_handle);
    s2sdk::SetEntSchemaEnt2(
        pointer,
        &Str::from_str(DECOY_THROWER_CLASS),
        &Str::from_str("m_hThrower"),
        player.pawn_handle,
        true,
        0,
    );
    s2sdk::SetEntSchema2(
        pointer,
        &Str::from_str("CBaseEntity"),
        &Str::from_str("m_iTeamNum"),
        s2sdk::GetClientTeam(player.slot) as i32 as i64,
        true,
        0,
    );
    let keys = plugify::Arr::from(vec!["classname".to_owned()]);
    let values = plugify::Arr::from(vec![plugify::Any::from("weapon_knife_projectile")]);
    s2sdk::DispatchSpawn2(handle, &keys, &values);
    if !current() {
        return Err(Fault::cancelled("throwing-knife task cancelled during spawn"));
    }
    if !s2sdk::IsValidEntHandle(handle) {
        return Err(Fault::mechanism(
            Mechanism::ThrowingKnives,
            "throwing-knife entity disappeared during spawn",
        ));
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 || s2sdk::GetEntSchemaEnt2(pointer, &Str::from_str(DECOY_THROWER_CLASS), &Str::from_str("m_hThrower"), 0) != player.pawn_handle {
        return Err(Fault::mechanism(
            Mechanism::ThrowingKnives,
            "throwing-knife grenade lost its thrower during spawn",
        ));
    }
    s2sdk::SetEntityModel(handle, &Str::from_str(model));
    s2sdk::SetEntityMoveType(handle, s2sdk::MoveType::Flygravity);
    if s2sdk::GetEntityMoveType(handle) != s2sdk::MoveType::Flygravity {
        return Err(Fault::mechanism(
            Mechanism::ThrowingKnives,
            "throwing-knife MoveType did not change to Flygravity",
        ));
    }
    s2sdk::SetEntityGravity(handle, funrounds_core::THROWING_KNIFE_GRAVITY);
    include_player_interaction(handle).map_err(|error| Fault::mechanism(Mechanism::ThrowingKnives, error))?;
    s2sdk::SetEntityDefaultCollisionGroup(handle);
    set_elasticity(handle, funrounds_core::THROWING_KNIFE_ELASTICITY).map_err(|error| Fault::mechanism(Mechanism::ThrowingKnives, error))?;
    s2sdk::SetEntSchema2(pointer, &Str::from_str("CBaseEntity"), &Str::from_str("m_nNextThinkTick"), -1, false, 0);
    if s2sdk::GetEntSchema2(pointer, &Str::from_str("CBaseEntity"), &Str::from_str("m_nNextThinkTick"), 0) != -1 {
        return Err(Fault::mechanism(
            Mechanism::ThrowingKnives,
            "throwing-knife grenade think was not disabled",
        ));
    }
    s2sdk::SetEntityAngVelocity(handle, &Vec3 { x: 4000.0, y: 0.0, z: 0.0 });
    if !current() {
        return Err(Fault::cancelled("throwing-knife task cancelled before Touch subscription"));
    }
    if !crate::hook_knife_touch(handle) {
        return Err(Fault::mechanism(
            Mechanism::ThrowingKnives,
            "could not hook the knife's concrete native Touch implementation",
        ));
    }
    s2sdk::TeleportEntity(handle, &origin, &angles, &velocity);
    if !current() {
        return Err(Fault::cancelled("throwing-knife task cancelled during teleport"));
    }
    if !s2sdk::IsValidEntHandle(handle) {
        return Err(Fault::mechanism(
            Mechanism::ThrowingKnives,
            "throwing-knife entity disappeared during teleport",
        ));
    }
    Ok(SpawnedKnife {
        task,
        handle,
        expires_at: s2sdk::GetGameTime() + funrounds_core::THROWING_KNIFE_LIFETIME,
    })
}

pub fn apply_impact(impact: Impact) -> Result<i32, Fault> {
    if unsafe { s2sdk::__s2sdk_TakeEntityDamage }.is_none() {
        return Err(Fault::mechanism(Mechanism::ProjectileDamage, "TakeEntityDamage unavailable"));
    }
    let damage_type = match impact.kind {
        Kind::Dodgeball => s2sdk::DamageTypes::DMG_BLAST,
        Kind::Knife => s2sdk::DamageTypes::DMG_SLASH,
    };
    Ok(s2sdk::TakeEntityDamage(
        impact.victim.pawn_handle,
        impact.projectile,
        impact.task.player.pawn_handle as i32,
        &impact.force,
        &impact.hit_position,
        impact.amount,
        damage_type,
    ))
}

pub fn remove_entity(handle: i32) {
    crate::touch::forget(handle);
    if s2sdk::IsValidEntHandle(handle) {
        s2sdk::RemoveEntity(handle);
    }
}

fn valid_task(task: PlayerTask, round: RoundGeneration, players: &PlayerTracker) -> Option<LivePlayer> {
    let player = LivePlayer::resolve(task.player.slot)?;
    (player.pawn_handle as u32 == task.player.pawn_handle && task_matches_tracker(task, round, players)).then_some(player)
}

fn task_matches_tracker(task: PlayerTask, round: RoundGeneration, players: &PlayerTracker) -> bool {
    players.validates(task, round, task.player.pawn_handle)
}

fn claim_once(claimed: &mut bool) -> bool {
    if *claimed {
        false
    } else {
        *claimed = true;
        true
    }
}

fn knife_impact(
    task: PlayerTask,
    projectile: i32,
    victim: LivePlayer,
    victim_task: PlayerTask,
    position: Vec3,
    velocity: Vec3,
    eye: Vec3,
) -> Option<Impact> {
    if victim.slot != victim_task.player.slot
        || victim.pawn_handle as u32 != victim_task.player.pawn_handle
        || task.round != victim_task.round
        || !finite_vec(position)
        || !finite_vec(velocity)
        || !finite_vec(eye)
        || length_squared(velocity) <= f32::EPSILON
    {
        return None;
    }
    let headshot = distance_squared(position, eye) <= funrounds_core::THROWING_KNIFE_HEADSHOT_DISTANCE.powi(2);
    Some(Impact {
        task,
        projectile,
        victim,
        victim_task,
        kind: Kind::Knife,
        amount: if headshot {
            funrounds_core::THROWING_KNIFE_HEAD_DAMAGE
        } else {
            funrounds_core::THROWING_KNIFE_BODY_DAMAGE
        },
        headshot,
        force: velocity,
        hit_position: position,
    })
}

fn impact_matches_damage(impact: Impact, damage: &Damage) -> bool {
    impact.projectile == damage.inflictor
        && impact.victim.pawn_handle == damage.victim
        && impact.task.player.pawn_handle as i32 == damage.attacker
        && (impact.amount - damage.amount).abs() <= 0.01
}

fn live_player_for_handle(handle: i32) -> Option<LivePlayer> {
    (0..s2sdk::GetMaxClients())
        .filter_map(LivePlayer::resolve)
        .find(|player| player.pawn_handle == handle)
}

fn live_player_task_for_handle(handle: i32, round: RoundGeneration, players: &PlayerTracker) -> Option<(LivePlayer, PlayerTask)> {
    let victim = live_player_for_handle(handle)?;
    let task = PlayerTask {
        round,
        player: players.identity(victim.slot)?,
    };
    task_matches_tracker(task, round, players).then_some((victim, task))
}

fn decoy_thrower(handle: i32) -> Option<i32> {
    if !s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    let thrower = s2sdk::GetEntSchemaEnt2(
        s2sdk::EntHandleToEntPointer(handle),
        &Str::from_str(DECOY_THROWER_CLASS),
        &Str::from_str("m_hThrower"),
        0,
    );
    s2sdk::IsValidEntHandle(thrower).then_some(thrower)
}

fn current_thrower_task(handle: i32, round: RoundGeneration, players: &PlayerTracker) -> Option<PlayerTask> {
    let thrower = decoy_thrower(handle)?;
    (0..s2sdk::GetMaxClients()).find_map(|slot| {
        let player = LivePlayer::resolve(slot)?;
        let identity = players.identity(slot)?;
        (thrower == player.pawn_handle).then_some(PlayerTask { round, player: identity })
    })
}

fn is_decoy(handle: i32) -> bool {
    if !s2sdk::IsValidEntHandle(handle) {
        return false;
    }
    matches!(s2sdk::GetEntityClassname(handle).to_string().as_str(), DECOY_PROJECTILE | "weapon_decoy")
}

fn player_collision_mask(mask: u64) -> u64 {
    mask | CONTENTS_PLAYER
}

fn include_player_interaction(handle: i32) -> Result<(), String> {
    let entity = s2sdk::EntHandleToEntPointer(handle);
    if entity == 0 {
        return Err("throwing-knife projectile pointer is invalid before collision setup".into());
    }
    let collision = s2sdk::GetEntSchema2(entity, &Str::from_str("CBaseEntity"), &Str::from_str("m_pCollision"), 0) as usize;
    if collision == 0 {
        return Err("throwing-knife projectile has no collision property".into());
    }
    let attributes = s2sdk::GetEntSchema2(collision, &Str::from_str("CCollisionProperty"), &Str::from_str("m_collisionAttribute"), 0) as usize;
    if attributes == 0 {
        return Err("throwing-knife projectile has no collision attributes".into());
    }
    let class = Str::from_str("VPhysicsCollisionAttribute_t");
    let field = Str::from_str("m_nInteractsWith");
    let original = s2sdk::GetEntSchema2(attributes, &class, &field, 0) as u64;
    let updated = player_collision_mask(original);
    s2sdk::SetEntSchema2(attributes, &class, &field, updated as i64, false, 0);
    let actual = s2sdk::GetEntSchema2(attributes, &class, &field, 0) as u64;
    if actual != updated {
        return Err(format!(
            "throwing-knife player collision mask failed: expected={updated:#x}, actual={actual:#x}"
        ));
    }
    Ok(())
}

fn set_elasticity(handle: i32, elasticity: f32) -> Result<(), String> {
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 {
        return Err("throwing-knife entity pointer unavailable".into());
    }
    let class = Str::from_str(ELASTICITY_CLASS);
    let field = Str::from_str(ELASTICITY_FIELD);
    s2sdk::SetEntSchemaFloat2(pointer, &class, &field, f64::from(elasticity), true, 0);
    let value = s2sdk::GetEntSchemaFloat2(pointer, &class, &field, 0) as f32;
    if !value.is_finite() || (value - elasticity).abs() > 0.001 {
        return Err("throwing-knife elasticity write failed".into());
    }
    Ok(())
}

fn forward(angles: Vec3) -> Vec3 {
    let pitch = angles.x.to_radians();
    let yaw = angles.y.to_radians();
    Vec3 {
        x: pitch.cos() * yaw.cos(),
        y: pitch.cos() * yaw.sin(),
        z: -pitch.sin(),
    }
}

fn add(left: Vec3, right: Vec3) -> Vec3 {
    Vec3 {
        x: left.x + right.x,
        y: left.y + right.y,
        z: left.z + right.z,
    }
}

fn scale(vector: Vec3, scalar: f32) -> Vec3 {
    Vec3 {
        x: vector.x * scalar,
        y: vector.y * scalar,
        z: vector.z * scalar,
    }
}

fn length_squared(vector: Vec3) -> f32 {
    vector.x * vector.x + vector.y * vector.y + vector.z * vector.z
}

fn distance_squared(left: Vec3, right: Vec3) -> f32 {
    length_squared(Vec3 {
        x: left.x - right.x,
        y: left.y - right.y,
        z: left.z - right.z,
    })
}

fn finite_vec(vector: Vec3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use funrounds_core::{Capabilities, RoundController};

    fn sample_task() -> (PlayerTracker, RoundController, funrounds_core::RoundKey, PlayerTask) {
        use rand::{SeedableRng, rngs::StdRng};

        let mut rounds = RoundController::new(None, Capabilities::default()).unwrap();
        let key = rounds.round_key(1);
        let mut rng = StdRng::seed_from_u64(1);
        assert!(rounds.start_round(key, true, &mut rng));
        let mut players = PlayerTracker::default();
        assert!(players.connect(1));
        let player = players.spawn(1, 1, 0x1100).unwrap();
        let task = PlayerTask {
            round: rounds.generation(),
            player,
        };
        (players, rounds, key, task)
    }

    fn sample_victim_task(players: &mut PlayerTracker, round: RoundGeneration, victim: LivePlayer) -> PlayerTask {
        assert!(players.connect(victim.slot));
        PlayerTask {
            round,
            player: players.spawn(victim.slot, 1, victim.pawn_handle as u32).unwrap(),
        }
    }

    fn queued_spawn(projectiles: &mut Projectiles) -> Action {
        let (_, _, _, task) = sample_task();
        projectiles.action(ActionKind::SpawnKnife {
            task,
            player: LivePlayer {
                slot: task.player.slot,
                controller_handle: 0x1200,
                pawn_handle: task.player.pawn_handle as i32,
            },
        })
    }

    #[test]
    fn failed_knife_creation_keeps_the_full_handle_until_a_later_cleanup_confirms_deletion() {
        let mut projectiles = Projectiles::default();
        let spawn = queued_spawn(&mut projectiles);
        let handle = 0x81001020u32 as i32;
        let failure = Fault::mechanism(Mechanism::ThrowingKnives, "Touch subscription failed");
        assert_eq!(
            projectiles.action_finished(
                spawn,
                ActionResult::Spawned {
                    created: Some(handle),
                    result: Err(failure.clone())
                },
                true
            ),
            Err(failure)
        );
        assert!(!projectiles.is_clean());
        let first = projectiles.cleanup_actions_with(|candidate| candidate == handle);
        assert_eq!(first.len(), 1);
        assert!(projectiles.cleanup_actions_with(|_| true).is_empty());
        projectiles.action_finished(first[0], ActionResult::Removed(false), true).unwrap();
        assert!(!projectiles.is_clean());
        let retry = projectiles.cleanup_actions_with(|candidate| candidate == handle);
        assert_eq!(retry.len(), 1);
        assert_ne!(first[0], retry[0]);
        projectiles.action_finished(first[0], ActionResult::Removed(true), true).unwrap();
        assert!(projectiles.action_current(retry[0]));
        assert!(!projectiles.is_clean());
        projectiles.action_finished(retry[0], ActionResult::Removed(true), true).unwrap();
        assert!(projectiles.is_clean());
    }

    #[test]
    fn cancelled_knife_creation_is_accounted_once_and_never_becomes_a_mechanism_failure() {
        let mut projectiles = Projectiles::default();
        let spawn = queued_spawn(&mut projectiles);
        projectiles.clear();
        assert!(!projectiles.is_clean());
        projectiles
            .action_finished(
                spawn,
                ActionResult::Spawned {
                    created: Some(1024),
                    result: Err(Fault::mechanism(Mechanism::ThrowingKnives, "old native result")),
                },
                false,
            )
            .unwrap();
        assert_eq!(projectiles.retired, HashSet::from([1024]));
        projectiles
            .action_finished(
                spawn,
                ActionResult::Spawned {
                    created: Some(2048),
                    result: Err(Fault::global("duplicate result")),
                },
                true,
            )
            .unwrap();
        assert_eq!(projectiles.retired, HashSet::from([1024]));
        assert!(projectiles.cleanup_actions_with(|_| false).is_empty());
        assert!(projectiles.is_clean());
    }

    #[test]
    fn cancelled_current_knife_creation_preserves_global_failure() {
        let mut projectiles = Projectiles::default();
        let action = queued_spawn(&mut projectiles);
        projectiles.clear();
        let fault = projectiles
            .action_finished(
                action,
                ActionResult::Spawned {
                    created: Some(1024),
                    result: Err(Fault::global("unknown ownership during cancelled native call")),
                },
                false,
            )
            .err()
            .unwrap();
        assert_eq!(fault.scope, crate::fault::Scope::Global);
        assert_eq!(projectiles.retired, HashSet::from([1024]));
        let mut faults = crate::fault::Faults::new(funrounds_core::Capabilities::default());
        faults.record(&fault);
        projectiles.cleanup_actions_with(|_| false);
        faults.finish_cleanup(projectiles.is_clean());
        assert_eq!(faults.recovery, crate::fault::Recovery::Stopped);
    }

    #[test]
    fn late_knife_and_remove_results_cannot_delete_a_reused_new_map_handle() {
        let mut projectiles = Projectiles::default();
        let old_spawn = queued_spawn(&mut projectiles);
        projectiles.retired.insert(1024);
        let old_remove = projectiles.cleanup_actions_with(|_| true)[0];
        projectiles.discard_map_state();
        let current = queued_spawn(&mut projectiles);
        projectiles.retired.insert(1024);
        let current_remove = projectiles.cleanup_actions_with(|_| true)[0];
        projectiles
            .action_finished(
                old_spawn,
                ActionResult::Spawned {
                    created: Some(1024),
                    result: Err(Fault::global("old map")),
                },
                true,
            )
            .unwrap();
        projectiles.action_finished(old_remove, ActionResult::Removed(true), true).unwrap();
        assert!(projectiles.action_current(current));
        assert!(projectiles.action_current(current_remove));
        assert_eq!(projectiles.retired, HashSet::from([1024]));
    }

    #[test]
    fn knife_angle_and_velocity_match_sourcepawn_launch_vector() {
        let forward = forward(Vec3 { x: 0.0, y: 90.0, z: 0.0 });
        assert!((forward.x).abs() < 0.0001);
        assert!((forward.y - 1.0).abs() < 0.0001);
        assert!(forward.z.abs() < 0.0001);
        let velocity = add(scale(forward, funrounds_core::THROWING_KNIFE_SPEED), Vec3 { x: 2.0, y: 3.0, z: 4.0 });
        assert!((velocity.x - 2.0).abs() < 0.001);
        assert!((velocity.y - 3503.0).abs() < 0.001);
        assert!((velocity.z - 4.0).abs() < 0.001);
    }

    #[test]
    fn knife_collision_mask_adds_player_layer_without_losing_existing_interactions() {
        let original = 0x2000_03001;
        let updated = player_collision_mask(original);
        assert_eq!(updated, 0x2000_43001);
        assert_eq!(updated & !CONTENTS_PLAYER, original);
        assert_eq!(player_collision_mask(updated), updated);
    }

    #[test]
    fn eye_distance_uses_inclusive_legacy_headshot_boundary() {
        let eye = Vec3 {
            x: 100.0,
            y: 200.0,
            z: 300.0,
        };
        let edge = Vec3 {
            x: 118.0,
            y: 200.0,
            z: 300.0,
        };
        let outside = Vec3 {
            x: 118.001,
            y: 200.0,
            z: 300.0,
        };
        let limit = funrounds_core::THROWING_KNIFE_HEADSHOT_DISTANCE.powi(2);
        assert!(distance_squared(eye, edge) <= limit);
        assert!(distance_squared(eye, outside) > limit);
    }

    #[test]
    fn knife_touch_damage_uses_exact_legacy_eye_distance_and_damage_values() {
        let (mut players, _, _, task) = sample_task();
        let victim = LivePlayer {
            slot: 2,
            controller_handle: 0x2200,
            pawn_handle: 0x2201,
        };
        let victim_task = sample_victim_task(&mut players, task.round, victim);
        let eye = Vec3 {
            x: 100.0,
            y: 200.0,
            z: 300.0,
        };
        let velocity = Vec3 { x: 3500.0, y: 2.0, z: 0.0 };
        let at_boundary = knife_impact(
            task,
            0x3300,
            victim,
            victim_task,
            Vec3 {
                x: 118.0,
                y: 200.0,
                z: 300.0,
            },
            velocity,
            eye,
        )
        .unwrap();
        assert!(at_boundary.headshot);
        assert_eq!(at_boundary.amount, 250.0);
        assert_eq!(
            at_boundary.hit_position,
            Vec3 {
                x: 118.0,
                y: 200.0,
                z: 300.0
            }
        );

        let outside = knife_impact(
            task,
            0x3301,
            victim,
            victim_task,
            Vec3 {
                x: 118.001,
                y: 200.0,
                z: 300.0,
            },
            velocity,
            eye,
        )
        .unwrap();
        assert!(!outside.headshot);
        assert_eq!(outside.amount, 80.0);
    }

    #[test]
    fn projectile_damage_can_be_claimed_once() {
        let mut claimed = false;
        assert!(claim_once(&mut claimed));
        assert!(!claim_once(&mut claimed));
    }

    #[test]
    fn synthetic_damage_matches_only_the_active_impact_identity() {
        let (mut players, _rounds, _key, task) = sample_task();
        let victim = LivePlayer {
            slot: 2,
            controller_handle: 0x2200,
            pawn_handle: 0x2201,
        };
        let victim_task = sample_victim_task(&mut players, task.round, victim);
        let impact = Impact {
            task,
            projectile: 0x3300,
            victim,
            victim_task,
            kind: Kind::Knife,
            amount: 80.0,
            headshot: false,
            force: Vec3 { x: 1.0, y: 0.0, z: 0.0 },
            hit_position: Vec3 { x: 10.0, y: 20.0, z: 30.0 },
        };
        let mut projectiles = Projectiles::default();
        projectiles.tracked.insert(
            impact.projectile,
            Tracked {
                task,
                kind: Kind::Knife,
                expires_at: None,
                claimed: true,
            },
        );
        assert!(projectiles.begin_native_impact(impact));
        assert!(projectiles.active_knife_death(victim.slot, task.player.slot));
        assert!(!projectiles.active_knife_death(victim.slot + 1, task.player.slot));
        assert!(!projectiles.active_knife_death(victim.slot, task.player.slot + 1));
        let mut damage = Damage {
            victim: impact.victim.pawn_handle,
            attacker: task.player.pawn_handle as i32,
            inflictor: impact.projectile,
            weapon_kind: 0,
            hit_group: 0,
            traced_head: false,
            amount: impact.amount,
            damage_type: None,
        };
        assert_eq!(projectiles.synthetic_impact(&damage), Some(impact));

        damage.victim += 1;
        assert_eq!(projectiles.synthetic_impact(&damage), None);
        damage.victim = impact.victim.pawn_handle;
        damage.attacker += 1;
        assert_eq!(projectiles.synthetic_impact(&damage), None);
        damage.attacker = task.player.pawn_handle as i32;
        damage.inflictor += 1;
        assert_eq!(projectiles.synthetic_impact(&damage), None);
        damage.inflictor = impact.projectile;
        damage.amount += 0.02;
        assert_eq!(projectiles.synthetic_impact(&damage), None);
        projectiles.active_impact = Some(Impact {
            kind: Kind::Dodgeball,
            ..impact
        });
        assert!(!projectiles.active_knife_death(victim.slot, task.player.slot));
        projectiles.active_impact = None;
        assert!(!projectiles.active_knife_death(victim.slot, task.player.slot));
        assert!(players.validates(task, task.round, task.player.pawn_handle));
        assert!(players.validates(victim_task, task.round, victim.pawn_handle as u32));
    }

    #[test]
    fn projectile_task_rejects_old_round_and_life_generations() {
        let (players, mut rounds, _, task) = sample_task();
        assert!(task_matches_tracker(task, rounds.generation(), &players));
        rounds.reset();
        assert!(!task_matches_tracker(task, rounds.generation(), &players));

        let (mut players, rounds, _, task) = sample_task();
        assert!(players.death(task.player.slot));
        assert!(players.spawn(task.player.slot, 2, 0x1101).is_some());
        assert!(!task_matches_tracker(task, rounds.generation(), &players));
    }

    #[test]
    fn queued_impact_rejects_victim_death_and_same_pawn_respawn() {
        let (mut players, rounds, _, attacker_task) = sample_task();
        let victim = LivePlayer {
            slot: 2,
            controller_handle: 0x2200,
            pawn_handle: 0x2201,
        };
        let victim_task = sample_victim_task(&mut players, rounds.generation(), victim);
        let impact = knife_impact(
            attacker_task,
            0x3300,
            victim,
            victim_task,
            Vec3 { x: 118.0, y: 0.0, z: 0.0 },
            Vec3 { x: 3500.0, y: 0.0, z: 0.0 },
            Vec3 { x: 100.0, y: 0.0, z: 0.0 },
        )
        .unwrap();
        assert!(players.validates(impact.victim_task, rounds.generation(), victim.pawn_handle as u32));

        assert!(players.death(victim.slot));
        assert!(players.spawn(victim.slot, 2, victim.pawn_handle as u32).is_some());
        assert!(!players.validates(impact.victim_task, rounds.generation(), victim.pawn_handle as u32));

        let current = players.identity(victim.slot).unwrap();
        assert_ne!(current, impact.victim_task.player);
        let current_task = PlayerTask {
            round: rounds.generation(),
            player: current,
        };
        assert!(players.validates(current_task, rounds.generation(), victim.pawn_handle as u32));
    }

    #[test]
    fn death_and_same_pawn_respawn_cannot_reauthorize_an_old_decoy() {
        let (mut players, rounds, _, old_task) = sample_task();
        let mut projectiles = Projectiles::default();
        projectiles.observe_decoy_throw(old_task, 0.0);
        assert_eq!(
            projectiles.pending_decoy_task(old_task.player.pawn_handle as i32, rounds.generation(), &players),
            Some(old_task)
        );
        assert!(
            projectiles
                .pending_decoy_task(old_task.player.slot, rounds.generation(), &players)
                .is_none()
        );
        projectiles.recent_decoys.push(RecentDecoy {
            handle: 0x3300,
            task: old_task,
            seen_at: 0.0,
        });

        // The native retirement scan also supplies a projectile not yet discovered by the frame loop.
        projectiles.retire_decoys(old_task.player.slot, [0x4400]);
        assert!(players.death(old_task.player.slot));
        let new_player = players.spawn(old_task.player.slot, 2, old_task.player.pawn_handle).unwrap();
        assert_ne!(new_player, old_task.player);
        assert!(
            projectiles
                .pending_decoy_task(new_player.pawn_handle as i32, rounds.generation(), &players)
                .is_none()
        );
        projectiles.observe_decoy_throw(
            PlayerTask {
                player: new_player,
                round: rounds.generation(),
            },
            0.1,
        );
        assert!(projectiles.retired.contains(&0x3300));
        assert!(projectiles.retired.contains(&0x4400));
        assert!(projectiles.recent_decoys.is_empty());
        assert!(!projectiles.tracked.contains_key(&0x3300));
        assert!(!projectiles.tracked.contains_key(&0x4400));
    }

    #[test]
    fn map_discard_drops_all_old_handle_state_without_engine_calls() {
        let (mut players, _, _, task) = sample_task();
        let victim = LivePlayer {
            slot: 2,
            controller_handle: 0x2200,
            pawn_handle: 0x2201,
        };
        let victim_task = sample_victim_task(&mut players, task.round, victim);
        let impact = Impact {
            task,
            projectile: 0x3300,
            victim,
            victim_task,
            kind: Kind::Knife,
            amount: 80.0,
            headshot: false,
            force: Vec3 { x: 1.0, y: 0.0, z: 0.0 },
            hit_position: Vec3 { x: 10.0, y: 20.0, z: 30.0 },
        };
        let mut projectiles = Projectiles::default();
        projectiles.tracked.insert(
            impact.projectile,
            Tracked {
                task,
                kind: Kind::Knife,
                expires_at: Some(1.0),
                claimed: true,
            },
        );
        projectiles.retired.insert(0x4400);
        projectiles.baseline_decoys.insert(0x5500);
        projectiles.pending_knives.push((task, 1));
        projectiles.seen_decoys.insert(0x6600);
        projectiles.pending_decoys.push(PendingDecoy { task, expires_at: 1.0 });
        projectiles.recent_decoys.push(RecentDecoy {
            handle: 0x7700,
            task,
            seen_at: 0.0,
        });
        projectiles.active_impact = Some(impact);

        crate::touch::track(impact.projectile);
        assert!(crate::touch::owns(impact.projectile));
        assert!(!crate::touch::owns(impact.projectile + (1 << 15)));
        crate::touch::forget(impact.projectile);
        assert!(!crate::touch::owns(impact.projectile));
        crate::touch::track(impact.projectile);
        projectiles.discard_map_state();

        assert!(!crate::touch::owns(impact.projectile));
        assert!(projectiles.tracked.is_empty());
        assert!(projectiles.retired.is_empty());
        assert!(projectiles.baseline_decoys.is_empty());
        assert!(projectiles.pending_knives.is_empty());
        assert!(projectiles.seen_decoys.is_empty());
        assert!(projectiles.pending_decoys.is_empty());
        assert!(projectiles.recent_decoys.is_empty());
        assert!(projectiles.active_impact.is_none());
    }
}
