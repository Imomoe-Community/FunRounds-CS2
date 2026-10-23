// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::{Probe, imports},
    fault::{Fault, Mechanism},
    game::{LivePlayer, entity_handles},
    s2sdk,
};
use funrounds_core::{BombKind, BombSettings, Capabilities, PlayerTask, PlayerTracker, RoundGeneration};
use plugify::{Str, Vec3};
use rand::{Rng, RngExt};
use std::collections::{HashMap, HashSet};

const PLANTED: &str = "CPlantedC4";
const C4: &str = "CC4";
const MARKED_FOR_DELETE: i64 = 1 << 9;
const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
const CHICKEN_BOMB_OFFSET_Z: f32 = 10.0;
const BLAST: &str = "CEnvExplosion";

#[derive(Clone, Copy, PartialEq)]
pub struct Explosion {
    bomb: i32,
    map_epoch: u64,
    round: RoundGeneration,
    position: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarryPose {
    World,
    Hand,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarryPlacement {
    pub player: Option<LivePlayer>,
    pub pose: CarryPose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionKind {
    SpawnChicken {
        round: RoundGeneration,
        bomb: i32,
    },
    SpawnProxy {
        round: RoundGeneration,
        bomb: i32,
        carrier: PlayerTask,
    },
    PlaceCarry {
        round: RoundGeneration,
        bomb: i32,
        proxy: i32,
        target: CarryPlacement,
        previous_owner: Option<LivePlayer>,
    },
    Cleanup {
        round: RoundGeneration,
        bomb: i32,
        chicken: Option<i32>,
        proxy: Option<i32>,
    },
    RemoveEntity {
        handle: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    id: u64,
    map_epoch: u64,
    kind: ActionKind,
}

impl Action {
    pub fn is_cleanup(self) -> bool {
        matches!(self.kind, ActionKind::Cleanup { .. } | ActionKind::RemoveEntity { .. })
    }
}

pub enum ActionResult {
    Chicken(i32),
    Proxy(i32),
    Placed(CarryPlacement),
    Cleaned(Vec<i32>),
    Removed(i32),
    CreationFailed { created: Option<i32>, error: Fault },
}

pub fn capabilities(events: Probe, explosion_event: Probe, round_prestart: bool, reasons: &mut Vec<String>) -> Capabilities {
    let mut entity_iteration = Probe::default();
    imports!(entity_iteration;
        __s2sdk_GetFirstActiveEntity, __s2sdk_GetNextActiveEntity,
        __s2sdk_GetEntityClassname, __s2sdk_IsValidEntHandle,
    );
    let mut timing = entity_iteration.clone();
    timing.include(events);
    for (class, field) in [
        (C4, "m_fArmedTime"),
        (C4, "m_bStartedArming"),
        (PLANTED, "m_flC4Blow"),
        (PLANTED, "m_flTimerLength"),
        (PLANTED, "m_flDefuseLength"),
        (PLANTED, "m_flDefuseCountDown"),
        (PLANTED, "m_hBombDefuser"),
        (PLANTED, "m_bBeingDefused"),
        (PLANTED, "m_bBombTicking"),
        (PLANTED, "m_bBombDefused"),
        ("CCSPlayerPawn", "m_iProgressBarDuration"),
        ("CCSPlayerPawn", "m_flProgressBarStartTime"),
    ] {
        timing.schema(class, field);
    }
    imports!(timing;
        __s2sdk_GetEntSchema2, __s2sdk_GetEntSchemaEnt2, __s2sdk_SetEntSchema2,
        __s2sdk_GetEntSchemaFloat2, __s2sdk_SetEntSchemaFloat2,
    );
    let mut owned_removal = Probe::default();
    owned_removal.schema("CEntityInstance", "m_pEntity");
    owned_removal.schema("CEntityIdentity", "m_flags");
    imports!(owned_removal; __s2sdk_EntHandleToEntPointer);
    let mut moving_explosion = explosion_event;
    for (class, field) in [
        (BLAST, "m_iMagnitude"),
        (BLAST, "m_flPlayerDamage"),
        (BLAST, "m_iRadiusOverride"),
        (BLAST, "m_hInflictor"),
        ("CBaseEntity", "m_spawnflags"),
    ] {
        moving_explosion.schema(class, field);
    }
    imports!(moving_explosion; __s2sdk_SetEntSchemaEnt2, __s2sdk_SetEntityOwner, __s2sdk_GetEntityOwner);
    let mut chicken = timing.clone();
    chicken.require(round_prestart, "event:round_prestart");
    chicken.include(moving_explosion.clone());
    chicken.include(owned_removal.clone());
    chicken.schema("CBaseEntity", "m_bTakesDamage");
    chicken.schema("CBaseEntity", "m_pfnUse");
    chicken.schema("CBaseEntity", "m_fEffects");
    imports!(chicken;
        __s2sdk_CreateEntityByName, __s2sdk_DispatchSpawn, __s2sdk_SetEntityParent,
        __s2sdk_AcceptEntityInput, __s2sdk_GetEntityParent, __s2sdk_SetEntityLocalOrigin,
        __s2sdk_GetEntityAngRotation, __s2sdk_GetEntityAbsOrigin, __s2sdk_SetEntityAbsOrigin,
        __s2sdk_TeleportEntity, __s2sdk_RemoveEntity,
    );
    let mut carry = timing.clone();
    carry.include(moving_explosion);
    carry.include(owned_removal);
    carry.schema(C4, "m_bBombPlacedAnimation");
    carry.schema("CBaseEntity", "m_hOwnerEntity");
    carry.schema("CBaseEntity", "m_fEffects");
    carry.require(crate::parameters::has_convar("mp_anyone_can_pickup_c4"), "convar:mp_anyone_can_pickup_c4");
    imports!(carry;
        __s2sdk_GiveNamedItem, __s2sdk_SetEntSchemaEnt2, __s2sdk_DropWeapon,
        __s2sdk_SetEntityOwner, __s2sdk_GetEntityOwner, __s2sdk_SetEntityParent,
        __s2sdk_AcceptEntityInput, __s2sdk_GetEntityParent, __s2sdk_GetEntityAbsOrigin,
        __s2sdk_SetEntityAbsOrigin, __s2sdk_TeleportEntity, __s2sdk_ScriptLookupAttachment,
        __s2sdk_GetEntityAttachmentOrigin, __s2sdk_GetEntityAttachmentAngles,
        __s2sdk_RemoveEntity, __s2sdk_IsClientInGame,
    );
    let mut bounce = entity_iteration;
    imports!(bounce; __s2sdk_GetEntityOwner, __s2sdk_SetEntityAbsVelocity);
    Capabilities {
        bomb_timing: timing.record("bomb_timing", reasons),
        chicken_bomb: chicken.record("chicken_bomb", reasons),
        carry_bomb: carry.record("carry_bomb", reasons),
        bomb_bounce: bounce.record("bomb_bounce", reasons),
        ..Capabilities::default()
    }
}

fn schema(class: &str, field: &str) -> bool {
    unsafe { s2sdk::__s2sdk_GetSchemaOffset }.is_some() && s2sdk::GetSchemaOffset(&Str::from_str(class), &Str::from_str(field)) >= 0
}

fn integer(handle: i32, class: &str, field: &str) -> i64 {
    s2sdk::GetEntSchema2(s2sdk::EntHandleToEntPointer(handle), &Str::from_str(class), &Str::from_str(field), 0)
}

fn float(handle: i32, class: &str, field: &str) -> f32 {
    s2sdk::GetEntSchemaFloat2(s2sdk::EntHandleToEntPointer(handle), &Str::from_str(class), &Str::from_str(field), 0) as f32
}

fn set_float(handle: i32, class: &str, field: &str, value: f32) -> Result<(), String> {
    if !s2sdk::IsValidEntHandle(handle) {
        return Err(format!("bomb entity disappeared: {handle}"));
    }
    s2sdk::SetEntSchemaFloat2(
        s2sdk::EntHandleToEntPointer(handle),
        &Str::from_str(class),
        &Str::from_str(field),
        f64::from(value),
        class != BLAST,
        0,
    );
    if (float(handle, class, field) - value).abs() > 0.001 {
        return Err(format!("{class}.{field} write failed"));
    }
    Ok(())
}

fn set_integer(handle: i32, class: &str, field: &str, value: i64) -> Result<(), String> {
    if !s2sdk::IsValidEntHandle(handle) {
        return Err(format!("bomb entity disappeared: {handle}"));
    }
    s2sdk::SetEntSchema2(
        s2sdk::EntHandleToEntPointer(handle),
        &Str::from_str(class),
        &Str::from_str(field),
        value,
        class != BLAST,
        0,
    );
    if integer(handle, class, field) != value {
        return Err(format!("{class}.{field} write failed"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum PropertyValue {
    Float(f32),
    Integer(i64),
}

struct OwnedProperty {
    handle: i32,
    class: &'static str,
    field: &'static str,
    previous: PropertyValue,
}

impl OwnedProperty {
    fn restore(&self) -> Result<(), String> {
        if !s2sdk::IsValidEntHandle(self.handle) {
            return Ok(());
        }
        match self.previous {
            PropertyValue::Float(value) => set_float(self.handle, self.class, self.field, value),
            PropertyValue::Integer(value) => set_integer(self.handle, self.class, self.field, value),
        }
    }
}

fn is_defuse_property(property: &OwnedProperty, bomb: i32, pawn: i32) -> bool {
    (property.handle == bomb && property.class == PLANTED && matches!(property.field, "m_flDefuseLength" | "m_flDefuseCountDown"))
        || (property.handle == pawn
            && property.class == "CCSPlayerPawn"
            && matches!(property.field, "m_iProgressBarDuration" | "m_flProgressBarStartTime"))
}

fn is_finished_bomb_property(property: &OwnedProperty, bomb: i32) -> bool {
    property.handle == bomb
        || (property.class == C4 && property.field == "m_fArmedTime")
        || (property.class == "CCSPlayerPawn" && matches!(property.field, "m_iProgressBarDuration" | "m_flProgressBarStartTime"))
}

fn is_consumed_defuse_property(property: &OwnedProperty, bomb: i32, pawn: Option<i32>) -> bool {
    property.handle == bomb || pawn.is_some_and(|pawn| is_defuse_property(property, bomb, pawn))
}

#[derive(Clone, Copy)]
struct TimedAction {
    task: PlayerTask,
    handle: i32,
    tick: i32,
    kit: bool,
}

struct DefuseAction {
    action: TimedAction,
    apply_pending: bool,
}

impl DefuseAction {
    fn take_due(&mut self, tick: i32) -> Option<TimedAction> {
        if !self.apply_pending || tick <= self.action.tick {
            return None;
        }
        self.apply_pending = false;
        Some(self.action)
    }
}

#[derive(Clone, Copy)]
struct Planted {
    handle: i32,
    round: RoundGeneration,
    kind: BombKind,
    planter: Option<PlayerTask>,
    chicken: Option<i32>,
    proxy: Option<i32>,
    placement: Option<CarryPlacement>,
    spawn_pending: bool,
    placement_pending: bool,
    cleanup_pending: bool,
}

#[derive(Default)]
pub struct Bombs {
    arming: Option<TimedAction>,
    defusing: Option<DefuseAction>,
    before_plant: Option<HashSet<i32>>,
    plant_task: Option<PlayerTask>,
    plant_weapon: Option<i32>,
    pending_plant: Option<(RoundGeneration, i32)>,
    planted: Option<Planted>,
    completed_chicken: Option<Planted>,
    cleanup_requested: bool,
    bouncing: HashMap<i32, f32>,
    retiring: HashSet<i32>,
    retiring_pending: HashSet<i32>,
    properties: Vec<OwnedProperty>,
    next_action: u64,
    map_epoch: u64,
    queued: Vec<Action>,
    pending_actions: HashMap<u64, Action>,
    pending_explosion: Option<Explosion>,
    exploded: HashSet<i32>,
    active_explosion: Option<i32>,
}

impl Bombs {
    pub fn excludes_suicide_c4(&self, player: LivePlayer, weapon: i32) -> bool {
        self.planted
            .is_some_and(|bomb| bomb.proxy == Some(weapon) || bomb.placement.is_some_and(|placement| placement.player == Some(player)))
    }

    pub fn capture_explosion(&mut self, round: RoundGeneration) {
        let Some(bomb) = self.planted else { return };
        if bomb.round != round
            || self.cleanup_requested
            || !matches!(bomb.kind, BombKind::Chicken | BombKind::Carry)
            || !s2sdk::IsValidEntHandle(bomb.handle)
        {
            return;
        }
        self.remember_explosion(Explosion {
            bomb: bomb.handle,
            map_epoch: self.map_epoch,
            round,
            position: s2sdk::GetEntityAbsOrigin(bomb.handle),
        });
    }

    fn remember_explosion(&mut self, explosion: Explosion) {
        if self.exploded.insert(explosion.bomb) {
            self.pending_explosion = Some(explosion);
        }
    }

    pub fn take_explosion(&mut self, round: RoundGeneration) -> Option<Explosion> {
        let explosion = self.pending_explosion.take()?;
        if explosion.round != round || !s2sdk::IsValidEntHandle(explosion.bomb) {
            return None;
        }
        self.active_explosion = Some(explosion.bomb);
        Some(explosion)
    }

    pub fn finish_explosion(&mut self, explosion: Explosion, temporary: i32) -> bool {
        if explosion.map_epoch != self.map_epoch || self.active_explosion != Some(explosion.bomb) {
            return false;
        }
        self.active_explosion = None;
        if s2sdk::IsValidEntHandle(temporary) {
            self.retiring.insert(temporary);
        }
        true
    }

    pub fn blocks_native_explosion(&self, inflictor: i32) -> bool {
        self.exploded.contains(&inflictor) && self.active_explosion != Some(inflictor)
    }

    pub fn prepare_carrier_disconnect(&mut self, round: RoundGeneration, player: LivePlayer) -> Option<Action> {
        let proxy = self.planted?.proxy?;
        self.queue_carrier_release(round, player, s2sdk::GetEntityOwner(proxy))
    }

    fn queue_carrier_release(&mut self, round: RoundGeneration, player: LivePlayer, owner: i32) -> Option<Action> {
        let bomb = self.planted?;
        let proxy = bomb.proxy?;
        if bomb.kind != BombKind::Carry
            || bomb.round != round
            || self.cleanup_requested
            || bomb.cleanup_pending
            || bomb.placement_pending
            || owner != player.pawn_handle
        {
            return None;
        }
        self.queue(ActionKind::PlaceCarry {
            round,
            bomb: bomb.handle,
            proxy,
            target: CarryPlacement {
                player: None,
                pose: CarryPose::World,
            },
            previous_owner: Some(player),
        });
        self.planted.as_mut().expect("planted bomb exists").placement_pending = true;
        // Execute this release before the native disconnect resumes, not on the next frame.
        self.queued.pop()
    }

    pub fn begin_plant(&mut self, task: PlayerTask) {
        let handle = s2sdk::GetClientActiveWeapon(task.player.slot);
        if !s2sdk::IsValidEntHandle(handle) || s2sdk::GetEntityClassname(handle).to_string() != "weapon_c4" {
            return;
        }
        self.before_plant = Some(entity_handles("planted_c4").into_iter().collect());
        self.plant_task = Some(task);
        self.plant_weapon = Some(handle);
        self.arming = Some(TimedAction {
            task,
            handle,
            tick: s2sdk::GetGameTickCount(),
            kit: false,
        });
    }

    pub fn abort_plant(&mut self) -> Result<(), String> {
        self.arming = None;
        self.before_plant = None;
        self.plant_task = None;
        if let Some(handle) = self.plant_weapon.take() {
            return self.restore_properties(|property| property.handle == handle && property.class == C4);
        }
        Ok(())
    }

    pub fn planted(&mut self, round: RoundGeneration, task: Option<PlayerTask>) {
        self.arming = None;
        self.plant_weapon = None;
        self.plant_task = task.or(self.plant_task.take());
        self.pending_plant = Some((round, s2sdk::GetGameTickCount()));
    }

    pub fn begin_defuse(&mut self, task: PlayerTask, kit: bool) -> Result<(), String> {
        self.abort_defuse()?;
        if let Some(planted) = self.planted {
            self.defusing = Some(DefuseAction {
                action: TimedAction {
                    task,
                    handle: planted.handle,
                    tick: s2sdk::GetGameTickCount(),
                    kit,
                },
                apply_pending: true,
            });
        }
        Ok(())
    }

    pub fn abort_defuse(&mut self) -> Result<(), String> {
        if let Some(defuse) = self.defusing.as_ref() {
            let action = defuse.action;
            self.restore_properties(|property| is_defuse_property(property, action.handle, action.task.player.pawn_handle as i32))?;
            self.defusing = None;
        }
        Ok(())
    }

    pub fn update(
        &mut self,
        settings: Option<&BombSettings>,
        round: RoundGeneration,
        players: &PlayerTracker,
        rng: &mut impl Rng,
    ) -> Result<(), Fault> {
        self.exploded.retain(|handle| s2sdk::IsValidEntHandle(*handle));
        self.retire_finished_entities();
        if self.cleanup_requested {
            self.queue_cleanup();
            return Ok(());
        }
        if settings.is_none() {
            self.arming = None;
            self.before_plant = None;
            self.plant_task = None;
            self.plant_weapon = None;
            self.pending_plant = None;
        }

        let tick = s2sdk::GetGameTickCount();
        let now = s2sdk::GetGameTime();
        if let (Some(action), Some(settings)) = (self.arming.as_ref(), settings)
            && tick > action.tick
        {
            let action = self.arming.take().expect("arming action exists");
            if valid_player(action.task, round, players)
                && s2sdk::IsValidEntHandle(action.handle)
                && s2sdk::GetClientActiveWeapon(action.task.player.slot) == action.handle
                && integer(action.handle, C4, "m_bStartedArming") != 0
            {
                self.write_property(action.handle, C4, "m_fArmedTime", PropertyValue::Float(now + settings.plant_time))?;
            }
        }

        if let (Some((generation, planted_tick)), Some(settings)) = (self.pending_plant, settings)
            && tick > planted_tick
        {
            self.pending_plant = None;
            if generation == round {
                let before = self.before_plant.take().ok_or("bomb_planted without a tracked installation")?;
                let candidates = entity_handles("planted_c4")
                    .into_iter()
                    .filter(|handle| !before.contains(handle))
                    .collect::<Vec<_>>();
                let handle = unique_new_bomb(&candidates)?;
                if self.planted.is_some() || self.completed_chicken.is_some() {
                    return Err("second authoritative planted bomb refused".into());
                }
                self.planted = Some(Planted {
                    handle,
                    round,
                    kind: settings.kind,
                    planter: self.plant_task.take(),
                    chicken: None,
                    proxy: None,
                    placement: None,
                    spawn_pending: false,
                    placement_pending: false,
                    cleanup_pending: false,
                });
                self.plant_weapon = None;
                self.write_property(handle, PLANTED, "m_flTimerLength", PropertyValue::Float(settings.explode_time))?;
                self.write_property(handle, PLANTED, "m_flC4Blow", PropertyValue::Float(now + settings.explode_time))?;
                if settings.kind == BombKind::Carry {
                    // The native weapon supplies view/world models; keep the ticking C4 for effects and rules only.
                    self.write_property(
                        handle,
                        "CBaseEntity",
                        "m_fEffects",
                        PropertyValue::Integer(integer(handle, "CBaseEntity", "m_fEffects") | 32),
                    )?;
                }
                s2sdk::PrintToServer(&Str::from_str(&format!(
                    "[FunRounds] planted bomb={handle} kind={:?} deadline={}",
                    settings.kind,
                    now + settings.explode_time
                )));
            }
        }

        if let Some(action) = self.defusing.as_mut().and_then(|defuse| defuse.take_due(tick))
            && valid_player(action.task, round, players)
            && s2sdk::IsValidEntHandle(action.handle)
            && integer(action.handle, PLANTED, "m_bBeingDefused") != 0
            && s2sdk::GetEntSchemaEnt2(
                s2sdk::EntHandleToEntPointer(action.handle),
                &Str::from_str(PLANTED),
                &Str::from_str("m_hBombDefuser"),
                0,
            ) as u32
                == action.task.player.pawn_handle
        {
            let duration = settings.map_or(if action.kit { 5.0 } else { 10.0 }, |settings| {
                if action.kit { settings.defuse_kit_time } else { settings.defuse_time }
            });
            self.write_property(action.handle, PLANTED, "m_flDefuseLength", PropertyValue::Float(duration))?;
            self.write_property(action.handle, PLANTED, "m_flDefuseCountDown", PropertyValue::Float(now + duration))?;
            if let Some(player) = LivePlayer::resolve(action.task.player.slot)
                && player.pawn_handle as u32 == action.task.player.pawn_handle
            {
                let seconds = duration.ceil().clamp(0.0, i32::MAX as f32) as i64;
                self.write_property(
                    player.pawn_handle,
                    "CCSPlayerPawn",
                    "m_iProgressBarDuration",
                    PropertyValue::Integer(seconds),
                )?;
                self.write_property(player.pawn_handle, "CCSPlayerPawn", "m_flProgressBarStartTime", PropertyValue::Float(now))?;
            }
        }

        if let Some(bomb) = self.planted
            && (bomb.round != round
                || !s2sdk::IsValidEntHandle(bomb.handle)
                || integer(bomb.handle, PLANTED, "m_bBombTicking") == 0
                || integer(bomb.handle, PLANTED, "m_bBombDefused") != 0)
        {
            self.release_completed_defuse();
            self.keep_defused_chicken().map_err(Fault::global)?;
            self.defusing = None;
            let restore = self.restore_properties(|property| is_finished_bomb_property(property, bomb.handle));
            self.cleanup_requested = true;
            self.queue_cleanup();
            return restore.map_err(Fault::global);
        }

        if let (Some(settings), Some(bomb)) = (settings, self.planted)
            && bomb.round == round
        {
            match bomb.kind {
                BombKind::Chicken if !bomb.spawn_pending && bomb.chicken.is_none() => {
                    self.queue(ActionKind::SpawnChicken { round, bomb: bomb.handle });
                    self.planted.as_mut().expect("planted bomb exists").spawn_pending = true;
                }
                BombKind::Carry => {
                    if !bomb.spawn_pending && bomb.proxy.is_none() {
                        let carrier = bomb.planter.filter(|task| valid_player(*task, round, players)).or_else(|| {
                            (0..s2sdk::GetMaxClients()).find_map(|slot| {
                                let player = LivePlayer::resolve(slot)?;
                                let identity = players.identity(slot)?;
                                let task = PlayerTask { player: identity, round };
                                players.validates(task, round, player.pawn_handle as u32).then_some(task)
                            })
                        });
                        if let Some(carrier) = carrier {
                            self.queue(ActionKind::SpawnProxy {
                                round,
                                bomb: bomb.handle,
                                carrier,
                            });
                            self.planted.as_mut().expect("planted bomb exists").spawn_pending = true;
                        }
                    }
                    if let Some(proxy) = bomb.proxy
                        && !bomb.placement_pending
                        && s2sdk::IsValidEntHandle(proxy)
                    {
                        let owner = s2sdk::GetEntityOwner(proxy);
                        let live = (0..s2sdk::GetMaxClients())
                            .filter_map(LivePlayer::resolve)
                            .find(|player| player.pawn_handle == owner);
                        let present = if live.is_none() && s2sdk::IsValidEntHandle(owner) {
                            (0..s2sdk::GetMaxClients())
                                .filter_map(LivePlayer::resolve_present)
                                .find(|player| player.pawn_handle == owner)
                        } else {
                            None
                        };
                        let previous_owner = live.or(present).or_else(|| {
                            bomb.placement
                                .and_then(|placement| placement.player.filter(|player| player.pawn_handle == owner))
                        });
                        let target = if let Some(player) = live {
                            let pose = if s2sdk::GetClientActiveWeapon(player.slot) == proxy {
                                CarryPose::Hand
                            } else {
                                CarryPose::Back
                            };
                            CarryPlacement { player: Some(player), pose }
                        } else {
                            CarryPlacement {
                                player: None,
                                pose: CarryPose::World,
                            }
                        };
                        self.queue(ActionKind::PlaceCarry {
                            round,
                            bomb: bomb.handle,
                            proxy,
                            target,
                            previous_owner,
                        });
                        self.planted.as_mut().expect("planted bomb exists").placement_pending = true;
                    }
                }
                _ => {}
            }
            if settings.bounce {
                self.update_bounce(now, rng);
            } else {
                self.bouncing.clear();
            }
        } else if settings.is_some_and(|settings| settings.bounce) {
            self.update_bounce(now, rng);
        } else {
            self.bouncing.clear();
        }
        Ok(())
    }

    fn update_bounce(&mut self, now: f32, rng: &mut impl Rng) {
        let dropped = entity_handles("weapon_c4")
            .into_iter()
            .filter(|handle| s2sdk::IsValidEntHandle(*handle) && !s2sdk::IsValidEntHandle(s2sdk::GetEntityOwner(*handle)))
            .collect::<HashSet<_>>();
        self.bouncing.retain(|handle, _| dropped.contains(handle));
        for handle in dropped {
            let due = self.bouncing.entry(handle).or_insert(now);
            if now >= *due && s2sdk::IsValidEntHandle(handle) && !s2sdk::IsValidEntHandle(s2sdk::GetEntityOwner(handle)) {
                let velocity = Vec3 {
                    x: rng.random_range(-360.0..=360.0),
                    y: rng.random_range(-360.0..=360.0),
                    z: rng.random_range(300.0..=360.0),
                };
                s2sdk::SetEntityAbsVelocity(handle, &velocity);
                *due = now + 1.0;
            }
        }
    }

    fn write_property(&mut self, handle: i32, class: &'static str, field: &'static str, value: PropertyValue) -> Result<(), Fault> {
        if !self
            .properties
            .iter()
            .any(|property| property.handle == handle && property.class == class && property.field == field)
        {
            let previous = match value {
                PropertyValue::Float(_) => PropertyValue::Float(float(handle, class, field)),
                PropertyValue::Integer(_) => PropertyValue::Integer(integer(handle, class, field)),
            };
            self.properties.push(OwnedProperty {
                handle,
                class,
                field,
                previous,
            });
        }
        match value {
            PropertyValue::Float(value) => set_float(handle, class, field, value),
            PropertyValue::Integer(value) => set_integer(handle, class, field, value),
        }
        .map_err(|error| {
            Fault::mechanism(
                if field == "m_fEffects" {
                    Mechanism::CarryBomb
                } else {
                    Mechanism::BombTiming
                },
                error,
            )
        })
    }

    fn restore_properties(&mut self, should_restore: impl Fn(&OwnedProperty) -> bool) -> Result<(), String> {
        let mut failures = Vec::new();
        for index in (0..self.properties.len()).rev() {
            if should_restore(&self.properties[index]) {
                match self.properties[index].restore() {
                    Ok(()) => {
                        self.properties.remove(index);
                    }
                    Err(error) => failures.push(error),
                }
            }
        }
        if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
    }

    fn release_completed_defuse(&mut self) {
        let Some(bomb) = self.planted else { return };
        if self.properties.is_empty() || !s2sdk::IsValidEntHandle(bomb.handle) || integer(bomb.handle, PLANTED, "m_bBombDefused") == 0 {
            return;
        }
        let pawn = self
            .defusing
            .as_ref()
            .filter(|defuse| defuse.action.handle == bomb.handle && defuse.action.task.round == bomb.round)
            .map(|defuse| defuse.action.task.player.pawn_handle as i32);
        // Native completion consumes these snapshots; replaying them revives the HUD and hidden Carry model.
        self.properties
            .retain(|property| !is_consumed_defuse_property(property, bomb.handle, pawn));
    }

    fn keep_defused_chicken(&mut self) -> Result<(), String> {
        let Some(bomb) = self
            .planted
            .filter(|bomb| bomb.kind == BombKind::Chicken && bomb.chicken.is_some() && !bomb.spawn_pending && !bomb.cleanup_pending)
        else {
            return Ok(());
        };
        if !s2sdk::IsValidEntHandle(bomb.handle) || integer(bomb.handle, PLANTED, "m_bBombDefused") == 0 {
            return Ok(());
        }
        // The native defused C4 remains until round cleanup; hide it without retiring its chicken.
        set_integer(
            bomb.handle,
            "CBaseEntity",
            "m_fEffects",
            integer(bomb.handle, "CBaseEntity", "m_fEffects") | 32,
        )?;
        self.completed_chicken = Some(bomb);
        self.planted.as_mut().expect("planted bomb exists").chicken = None;
        Ok(())
    }

    fn retire_completed_chicken(&mut self, valid: impl Fn(i32) -> bool) {
        let Some(bomb) = self.completed_chicken else { return };
        if bomb.chicken.is_some_and(valid) {
            self.queue(ActionKind::Cleanup {
                round: bomb.round,
                bomb: bomb.handle,
                chicken: bomb.chicken,
                proxy: None,
            });
        }
        self.completed_chicken = None;
    }

    fn retire_finished_entities(&mut self) {
        self.retiring.retain(|handle| s2sdk::IsValidEntHandle(*handle));
        self.retiring_pending.retain(|handle| self.retiring.contains(handle));
        let handles = self.retiring.iter().copied().collect::<Vec<_>>();
        for handle in handles {
            self.queue_remove(handle);
        }
    }

    pub fn acknowledge_shutdown_removals(&mut self) {
        self.retain_unacknowledged_removals(engine_removal_acknowledged);
    }

    fn retain_unacknowledged_removals(&mut self, mut acknowledged: impl FnMut(i32) -> bool) {
        self.retiring.retain(|handle| !acknowledged(*handle));
        self.retiring_pending.retain(|handle| self.retiring.contains(handle));
    }

    fn queue_remove(&mut self, handle: i32) {
        if !s2sdk::IsValidEntHandle(handle) {
            self.retiring.remove(&handle);
            self.retiring_pending.remove(&handle);
        } else if self.retiring_pending.insert(handle) {
            self.queue(ActionKind::RemoveEntity { handle });
        }
    }

    fn queue(&mut self, kind: ActionKind) -> u64 {
        self.next_action = self.next_action.checked_add(1).expect("bomb action sequence exhausted");
        let action = Action {
            id: self.next_action,
            map_epoch: self.map_epoch,
            kind,
        };
        self.pending_actions.insert(action.id, action);
        self.queued.push(action);
        action.id
    }

    fn queue_cleanup(&mut self) {
        let action = {
            let Some(bomb) = self.planted.as_mut() else {
                self.cleanup_requested = false;
                return;
            };
            if bomb.cleanup_pending || (bomb.spawn_pending && bomb.chicken.is_none() && bomb.proxy.is_none()) {
                return;
            }
            bomb.cleanup_pending = true;
            ActionKind::Cleanup {
                round: bomb.round,
                bomb: bomb.handle,
                chicken: bomb.chicken,
                proxy: bomb.proxy,
            }
        };
        self.queue(action);
    }

    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.queued)
    }

    pub fn action_valid(&self, action: Action, round: RoundGeneration, players: &PlayerTracker) -> bool {
        if action.map_epoch != self.map_epoch || self.pending_actions.get(&action.id) != Some(&action) {
            return false;
        }
        match action.kind {
            ActionKind::SpawnChicken { round: action_round, bomb } => self.planted.is_some_and(|planted| {
                planted.round == action_round
                    && action_round == round
                    && planted.handle == bomb
                    && planted.kind == BombKind::Chicken
                    && planted.chicken.is_none()
                    && planted.spawn_pending
                    && !self.cleanup_requested
            }),
            ActionKind::SpawnProxy {
                round: action_round,
                bomb,
                carrier,
            } => self.planted.is_some_and(|planted| {
                planted.round == action_round
                    && action_round == round
                    && planted.handle == bomb
                    && planted.kind == BombKind::Carry
                    && planted.proxy.is_none()
                    && planted.spawn_pending
                    && !self.cleanup_requested
                    && valid_player(carrier, action_round, players)
            }),
            ActionKind::PlaceCarry {
                round: action_round,
                bomb,
                proxy,
                ..
            } => self.planted.is_some_and(|planted| {
                planted.round == action_round
                    && action_round == round
                    && planted.handle == bomb
                    && planted.kind == BombKind::Carry
                    && planted.proxy == Some(proxy)
                    && planted.placement_pending
                    && !self.cleanup_requested
            }),
            ActionKind::Cleanup { .. } => true,
            ActionKind::RemoveEntity { handle } => self.retiring.contains(&handle) && self.retiring_pending.contains(&handle),
        }
    }

    pub fn action_cancelled(&mut self, action: Action) {
        if action.map_epoch != self.map_epoch || self.pending_actions.get(&action.id) != Some(&action) {
            return;
        }
        self.pending_actions.remove(&action.id);
        match action.kind {
            ActionKind::SpawnChicken { round, bomb } | ActionKind::SpawnProxy { round, bomb, .. } => {
                if let Some(planted) = self.planted.as_mut().filter(|planted| planted.round == round && planted.handle == bomb) {
                    planted.spawn_pending = false;
                }
            }
            ActionKind::PlaceCarry { round, bomb, .. } => {
                if let Some(planted) = self.planted.as_mut().filter(|planted| planted.round == round && planted.handle == bomb) {
                    planted.placement_pending = false;
                }
            }
            ActionKind::Cleanup { round, bomb, .. } => {
                if let Some(planted) = self.planted.as_mut().filter(|planted| planted.round == round && planted.handle == bomb) {
                    planted.cleanup_pending = false;
                }
            }
            ActionKind::RemoveEntity { handle } => {
                self.retiring_pending.remove(&handle);
            }
        }
    }

    pub fn action_finished(&mut self, action: Action, result: Result<ActionResult, Fault>, accept_business: bool) -> Result<(), Fault> {
        if action.map_epoch != self.map_epoch || self.pending_actions.get(&action.id) != Some(&action) {
            return Ok(());
        }
        self.pending_actions.remove(&action.id);
        let (result, creation_error) = match result {
            Ok(ActionResult::CreationFailed {
                created: Some(handle),
                error,
            }) => {
                let result = match action.kind {
                    ActionKind::SpawnChicken { .. } => ActionResult::Chicken(handle),
                    ActionKind::SpawnProxy { .. } => ActionResult::Proxy(handle),
                    _ => return Err("non-creation bomb action returned a partial creation".into()),
                };
                (Ok(result), Some(error))
            }
            Ok(ActionResult::CreationFailed { created: None, error }) => (Err(error), None),
            result => (result, None),
        };
        let completed = match (action.kind, result) {
            (ActionKind::SpawnChicken { round, bomb }, Ok(ActionResult::Chicken(chicken))) => {
                if let Some(planted) = self
                    .planted
                    .as_mut()
                    .filter(|planted| planted.round == round && planted.handle == bomb && planted.kind == BombKind::Chicken)
                {
                    planted.chicken = Some(chicken);
                    planted.spawn_pending = false;
                    if self.cleanup_requested || !accept_business || creation_error.is_some() {
                        self.include_in_queued_cleanup(round, bomb, Some(chicken), None);
                    }
                    Ok(())
                } else {
                    self.queue(ActionKind::Cleanup {
                        round,
                        bomb,
                        chicken: Some(chicken),
                        proxy: None,
                    });
                    Ok(())
                }
            }
            (ActionKind::SpawnProxy { round, bomb, .. }, Ok(ActionResult::Proxy(proxy))) => {
                if let Some(planted) = self
                    .planted
                    .as_mut()
                    .filter(|planted| planted.round == round && planted.handle == bomb && planted.kind == BombKind::Carry)
                {
                    planted.proxy = Some(proxy);
                    planted.placement = Some(CarryPlacement {
                        player: None,
                        pose: CarryPose::World,
                    });
                    planted.spawn_pending = false;
                    if self.cleanup_requested || !accept_business || creation_error.is_some() {
                        self.include_in_queued_cleanup(round, bomb, None, Some(proxy));
                    }
                    Ok(())
                } else {
                    self.queue(ActionKind::Cleanup {
                        round,
                        bomb,
                        chicken: None,
                        proxy: Some(proxy),
                    });
                    Ok(())
                }
            }
            (ActionKind::PlaceCarry { round, bomb, proxy, .. }, Ok(ActionResult::Placed(placement))) => {
                if let Some(planted) = self
                    .planted
                    .as_mut()
                    .filter(|planted| planted.round == round && planted.handle == bomb && planted.proxy == Some(proxy))
                {
                    planted.placement = Some(placement);
                    planted.placement_pending = false;
                }
                Ok(())
            }
            (ActionKind::Cleanup { round, bomb, chicken, proxy }, Ok(ActionResult::Cleaned(retiring))) => {
                self.retiring.extend(retiring);
                if let Some(planted) = self.planted.as_mut().filter(|planted| planted.round == round && planted.handle == bomb) {
                    if planted.chicken == chicken {
                        planted.chicken = None;
                    }
                    if planted.proxy == proxy {
                        planted.proxy = None;
                    }
                    planted.cleanup_pending = false;
                    if self.cleanup_requested && planted.chicken.is_none() && planted.proxy.is_none() && !planted.spawn_pending {
                        self.planted = None;
                        self.cleanup_requested = false;
                    }
                }
                Ok(())
            }
            (ActionKind::SpawnChicken { .. } | ActionKind::SpawnProxy { .. }, Err(error)) => {
                if let Some(planted) = self.planted.as_mut() {
                    planted.spawn_pending = false;
                }
                Err(error)
            }
            (ActionKind::PlaceCarry { .. }, Err(error)) => {
                if let Some(planted) = self.planted.as_mut() {
                    planted.placement_pending = false;
                }
                Err(error)
            }
            (ActionKind::Cleanup { .. }, Err(error)) => {
                if let Some(planted) = self.planted.as_mut() {
                    planted.cleanup_pending = false;
                }
                self.queue(action.kind);
                if let Some(planted) = self.planted.as_mut() {
                    planted.cleanup_pending = true;
                }
                Err(error)
            }
            (ActionKind::RemoveEntity { handle }, Ok(ActionResult::Removed(removed))) if handle == removed => {
                self.retiring_pending.remove(&handle);
                if !s2sdk::IsValidEntHandle(handle) {
                    self.retiring.remove(&handle);
                }
                Ok(())
            }
            (ActionKind::RemoveEntity { handle }, Err(error)) => {
                self.retiring_pending.remove(&handle);
                Err(error)
            }
            _ => Err("bomb engine action returned an unexpected result".into()),
        };
        if let Some(error) = creation_error {
            if accept_business || error.scope == crate::fault::Scope::Global {
                Err(error)
            } else {
                Ok(())
            }
        } else if matches!(action.kind, ActionKind::Cleanup { .. } | ActionKind::RemoveEntity { .. })
            || accept_business
            || completed.as_ref().is_err_and(|error| error.scope == crate::fault::Scope::Global)
        {
            completed
        } else {
            Ok(())
        }
    }

    fn include_in_queued_cleanup(&mut self, round: RoundGeneration, bomb: i32, chicken: Option<i32>, proxy: Option<i32>) {
        for action in &mut self.queued {
            if let ActionKind::Cleanup {
                round: action_round,
                bomb: action_bomb,
                chicken: current_chicken,
                proxy: current_proxy,
            } = action.kind
                && action_round == round
                && action_bomb == bomb
            {
                action.kind = ActionKind::Cleanup {
                    round,
                    bomb,
                    chicken: chicken.or(current_chicken),
                    proxy: proxy.or(current_proxy),
                };
                self.pending_actions.insert(action.id, *action);
                return;
            }
        }
        if let Some(planted) = self.planted.as_ref().filter(|planted| planted.round == round && planted.handle == bomb) {
            // A cleanup action already in flight cannot be patched; queue a second idempotent pass.
            let cleanup = ActionKind::Cleanup {
                round,
                bomb,
                chicken: chicken.or(planted.chicken),
                proxy: proxy.or(planted.proxy),
            };
            if let Some(planted) = self.planted.as_mut() {
                planted.cleanup_pending = false;
            }
            self.queue(cleanup);
            if let Some(planted) = self.planted.as_mut() {
                planted.cleanup_pending = true;
            }
        }
    }

    pub fn cleanup(&mut self) -> Result<(), String> {
        self.release_completed_defuse();
        self.retire_completed_chicken(s2sdk::IsValidEntHandle);
        self.cleanup_active()
    }

    pub fn cleanup_after_round_end(&mut self) -> Result<(), String> {
        self.release_completed_defuse();
        self.keep_defused_chicken()?;
        self.cleanup_active()
    }

    fn cleanup_active(&mut self) -> Result<(), String> {
        self.pending_explosion = None;
        self.arming = None;
        self.defusing = None;
        self.before_plant = None;
        self.plant_task = None;
        self.plant_weapon = None;
        self.pending_plant = None;
        self.bouncing.clear();
        self.cleanup_requested = true;
        for action in std::mem::take(&mut self.queued) {
            if matches!(action.kind, ActionKind::Cleanup { .. } | ActionKind::RemoveEntity { .. }) {
                self.queued.push(action);
            } else {
                self.action_cancelled(action);
            }
        }
        let restore = self.restore_properties(|_| true);
        self.queue_cleanup();
        self.retire_finished_entities();
        restore
    }

    pub fn discard_map_state(&mut self) {
        self.map_epoch = self.map_epoch.wrapping_add(1);
        self.arming = None;
        self.defusing = None;
        self.before_plant = None;
        self.plant_task = None;
        self.plant_weapon = None;
        self.pending_plant = None;
        self.planted = None;
        self.completed_chicken = None;
        self.cleanup_requested = false;
        self.bouncing.clear();
        self.retiring.clear();
        self.retiring_pending.clear();
        self.properties.clear();
        self.queued.clear();
        self.pending_actions.clear();
        self.pending_explosion = None;
        self.exploded.clear();
        self.active_explosion = None;
    }

    pub fn is_clean(&self) -> bool {
        self.planted.is_none()
            && self.completed_chicken.is_none()
            && self.properties.is_empty()
            && self.bouncing.is_empty()
            && self.retiring.is_empty()
            && self.arming.is_none()
            && self.defusing.is_none()
            && self.pending_plant.is_none()
            && self.pending_actions.is_empty()
            && self.queued.is_empty()
            && self.retiring_pending.is_empty()
            && self.pending_explosion.is_none()
            && self.active_explosion.is_none()
            && !self.cleanup_requested
    }
}

fn engine_removal_acknowledged(handle: i32) -> bool {
    if !s2sdk::IsValidEntHandle(handle) {
        return true;
    }
    if !schema("CEntityInstance", "m_pEntity") || !schema("CEntityIdentity", "m_flags") {
        return false;
    }
    let entity = s2sdk::EntHandleToEntPointer(handle);
    if entity == 0 {
        return false;
    }
    let identity = s2sdk::GetEntSchema2(entity, &Str::from_str("CEntityInstance"), &Str::from_str("m_pEntity"), 0) as usize;
    if identity == 0 {
        return false;
    }
    let flags = s2sdk::GetEntSchema2(identity, &Str::from_str("CEntityIdentity"), &Str::from_str("m_flags"), 0);
    // Kill queues native deletion; its full handle can survive until a later engine frame.
    flags & MARKED_FOR_DELETE != 0 && s2sdk::IsValidEntHandle(handle) && s2sdk::EntHandleToEntPointer(handle) == entity
}

fn valid_player(task: PlayerTask, round: RoundGeneration, players: &PlayerTracker) -> bool {
    LivePlayer::resolve(task.player.slot).is_some_and(|player| players.validates(task, round, player.pawn_handle as u32))
}

fn unique_new_bomb(handles: &[i32]) -> Result<i32, String> {
    match handles {
        [handle] => Ok(*handle),
        _ => Err(format!("expected one new planted bomb, found {}", handles.len())),
    }
}

pub fn execute(action: Action) -> Result<ActionResult, Fault> {
    match action.kind {
        ActionKind::SpawnChicken { bomb, .. } => {
            let mut created = None;
            Ok(match spawn_chicken(bomb, &mut created) {
                Ok(handle) => ActionResult::Chicken(handle),
                Err(error) => ActionResult::CreationFailed { created, error },
            })
        }
        ActionKind::SpawnProxy { bomb, carrier, .. } => {
            let mut created = None;
            Ok(match spawn_proxy(bomb, carrier, &mut created) {
                Ok(handle) => ActionResult::Proxy(handle),
                Err(error) => ActionResult::CreationFailed { created, error },
            })
        }
        ActionKind::PlaceCarry {
            bomb,
            proxy,
            target,
            previous_owner,
            ..
        } => place_carry(bomb, proxy, target, previous_owner)
            .map(ActionResult::Placed)
            .map_err(|error| Fault::mechanism(Mechanism::CarryBomb, error)),
        ActionKind::Cleanup { bomb, chicken, proxy, .. } => {
            cleanup_result(|| cleanup_owned(bomb, chicken, proxy).map(ActionResult::Cleaned).map_err(Fault::global))
        }
        ActionKind::RemoveEntity { handle } => cleanup_result(|| remove_owned(handle).map(ActionResult::Removed).map_err(Fault::global)),
    }
}

fn cleanup_result(execute: impl FnOnce() -> Result<ActionResult, Fault>) -> Result<ActionResult, Fault> {
    // Cleanup actions retain full handles; report panics so action_finished can queue their retry.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(execute)).unwrap_or_else(|_| Err(Fault::global("bomb cleanup panic contained")))
}

pub fn explode(explosion: Explosion) -> (i32, Result<(), Fault>) {
    let temporary = s2sdk::CreateEntityByName(&Str::from_str("env_explosion"));
    if !s2sdk::IsValidEntHandle(temporary) {
        return (
            temporary,
            Err(Fault::mechanism(Mechanism::MovingBlast, "moving C4 radius explosion creation failed")),
        );
    }
    let result = (|| {
        // Keep the native C4 effect/sound; this entity supplies only position-relative blast damage.
        set_integer(temporary, "CBaseEntity", "m_spawnflags", 4 | 8 | 16 | 32 | 64 | 256 | 512 | 1024)?;
        set_integer(temporary, BLAST, "m_iMagnitude", 500)?;
        set_float(temporary, BLAST, "m_flPlayerDamage", 500.0)?;
        set_integer(temporary, BLAST, "m_iRadiusOverride", 1750)?;
        s2sdk::SetEntSchemaEnt2(
            s2sdk::EntHandleToEntPointer(temporary),
            &Str::from_str(BLAST),
            &Str::from_str("m_hInflictor"),
            explosion.bomb,
            false,
            0,
        );
        s2sdk::SetEntityOwner(temporary, explosion.bomb);
        s2sdk::DispatchSpawn(temporary);
        if s2sdk::GetEntSchemaEnt2(
            s2sdk::EntHandleToEntPointer(temporary),
            &Str::from_str(BLAST),
            &Str::from_str("m_hInflictor"),
            0,
        ) != explosion.bomb
            || s2sdk::GetEntityOwner(temporary) != explosion.bomb
        {
            return Err("moving C4 radius explosion attribution failed".into());
        }
        s2sdk::TeleportEntity(temporary, &explosion.position, &ZERO, &ZERO);
        s2sdk::PrintToServer(&Str::from_str(&format!(
            "[FunRounds] moving C4 blast bomb={} origin={},{},{} damage=500 radius=1750",
            explosion.bomb, explosion.position.x, explosion.position.y, explosion.position.z
        )));
        s2sdk::AcceptEntityInput(
            temporary,
            &Str::from_str("Explode"),
            -1,
            -1,
            &plugify::Var::default(),
            s2sdk::FieldType::Auto,
            0,
        );
        Ok(())
    })();
    if s2sdk::IsValidEntHandle(temporary) {
        s2sdk::RemoveEntity(temporary);
    }
    (temporary, result.map_err(|error: String| Fault::mechanism(Mechanism::MovingBlast, error)))
}

fn spawn_chicken(bomb: i32, created: &mut Option<i32>) -> Result<i32, Fault> {
    if !s2sdk::IsValidEntHandle(bomb) {
        return Err(Fault::cancelled("planted bomb disappeared before chicken creation"));
    }
    let chicken = s2sdk::CreateEntityByName(&Str::from_str("chicken"));
    if !s2sdk::IsValidEntHandle(chicken) {
        return Err(Fault::mechanism(Mechanism::ChickenBomb, "chicken creation failed"));
    }
    *created = Some(chicken);
    (|| {
        s2sdk::DispatchSpawn(chicken);
        if !s2sdk::IsValidEntHandle(chicken) {
            return Err(Fault::mechanism(Mechanism::ChickenBomb, "chicken disappeared during spawn"));
        }
        s2sdk::SetEntSchema2(
            s2sdk::EntHandleToEntPointer(chicken),
            &Str::from_str("CBaseEntity"),
            &Str::from_str("m_pfnUse"),
            0,
            false,
            0,
        );
        if integer(chicken, "CBaseEntity", "m_pfnUse") != 0 {
            return Err(Fault::mechanism(Mechanism::ChickenBomb, "chicken use handler disable failed"));
        }
        s2sdk::SetEntSchema2(
            s2sdk::EntHandleToEntPointer(chicken),
            &Str::from_str("CBaseEntity"),
            &Str::from_str("m_bTakesDamage"),
            0,
            true,
            0,
        );
        let origin = s2sdk::GetEntityAbsOrigin(bomb);
        let angles = s2sdk::GetEntityAngRotation(bomb);
        s2sdk::TeleportEntity(chicken, &origin, &angles, &ZERO);
        s2sdk::SetEntityParent(bomb, chicken);
        s2sdk::SetEntityLocalOrigin(
            bomb,
            &Vec3 {
                x: 0.0,
                y: 0.0,
                z: CHICKEN_BOMB_OFFSET_Z,
            },
        );
        if integer(chicken, "CBaseEntity", "m_bTakesDamage") != 0 || s2sdk::GetEntityParent(bomb) != chicken {
            return Err(Fault::mechanism(
                Mechanism::ChickenBomb,
                "chicken invulnerability or bomb attachment failed",
            ));
        }
        Ok(chicken)
    })()
}

fn spawn_proxy(bomb: i32, carrier: PlayerTask, created: &mut Option<i32>) -> Result<i32, Fault> {
    if !valid_carrier(carrier) || !s2sdk::IsValidEntHandle(bomb) {
        return Err(Fault::cancelled("carry proxy carrier or planted bomb changed before creation"));
    }
    let before = s2sdk::GetClientWeapons(carrier.player.slot);
    let proxy = s2sdk::GiveNamedItem(carrier.player.slot, &Str::from_str("weapon_c4"));
    if s2sdk::IsValidEntHandle(proxy) && !before.iter().any(|handle| *handle == proxy) {
        *created = Some(proxy);
    }
    if *created != Some(proxy)
        || !s2sdk::GetClientWeapons(carrier.player.slot).iter().any(|handle| *handle == proxy)
        || s2sdk::GetEntityClassname(proxy).to_string() != "weapon_c4"
    {
        return Err(Fault::global("carry proxy GiveNamedItem failed"));
    }
    (|| {
        // This CC4 supplies pickup and native weapon models; the planted entity remains authoritative.
        set_integer(proxy, C4, "m_bBombPlacedAnimation", 1).map_err(|error| Fault::mechanism(Mechanism::CarryBomb, error))?;
        set_integer(proxy, C4, "m_bStartedArming", 0).map_err(|error| Fault::mechanism(Mechanism::CarryBomb, error))?;
        if s2sdk::GetEntityOwner(proxy) != carrier.player.pawn_handle as i32 {
            return Err(Fault::mechanism(Mechanism::CarryBomb, "carry proxy was not given to the planter"));
        }
        sync_carry_origin(bomb, s2sdk::GetEntityAbsOrigin(proxy), ZERO).map_err(|error| Fault::mechanism(Mechanism::CarryBomb, error))?;
        Ok(proxy)
    })()
}

fn valid_carrier(task: PlayerTask) -> bool {
    LivePlayer::resolve(task.player.slot).is_some_and(|player| {
        player.pawn_handle as u32 == task.player.pawn_handle && player.controller_handle == s2sdk::PlayerSlotToEntHandle(task.player.slot)
    })
}

fn place_carry(bomb: i32, proxy: i32, target: CarryPlacement, previous_owner: Option<LivePlayer>) -> Result<CarryPlacement, String> {
    if !s2sdk::IsValidEntHandle(bomb) || !s2sdk::IsValidEntHandle(proxy) {
        return Err("carry bomb or proxy disappeared".into());
    }
    let placement = match (target.player, target.pose) {
        (Some(player), CarryPose::Hand | CarryPose::Back) if LivePlayer::resolve(player.slot) == Some(player) => {
            let name = if target.pose == CarryPose::Hand { "weapon_hand_R" } else { "c4" };
            let attachment = s2sdk::ScriptLookupAttachment(player.pawn_handle, &Str::from_str(name));
            if attachment <= 0 {
                return Err(format!("CS2 C4 attachment {name} unavailable for slot={}", player.slot));
            }
            if s2sdk::GetEntityOwner(proxy) != player.pawn_handle {
                return place_carry_world(bomb, proxy, previous_owner);
            }
            sync_carry_origin(
                bomb,
                s2sdk::GetEntityAttachmentOrigin(player.pawn_handle, attachment),
                s2sdk::GetEntityAttachmentAngles(player.pawn_handle, attachment),
            )?;
            CarryPlacement {
                player: Some(player),
                pose: target.pose,
            }
        }
        (Some(player), _) => place_carry_world(bomb, proxy, Some(player))?,
        (None, CarryPose::World | CarryPose::Hand | CarryPose::Back) => place_carry_world(bomb, proxy, previous_owner)?,
    };
    Ok(placement)
}

fn place_carry_world(bomb: i32, proxy: i32, previous_owner: Option<LivePlayer>) -> Result<CarryPlacement, String> {
    let position = s2sdk::GetEntityAbsOrigin(bomb);
    let releasing = previous_owner.is_some() || s2sdk::IsValidEntHandle(s2sdk::GetEntityOwner(proxy));
    if let Some(player) = previous_owner
        && s2sdk::IsClientInGame(player.slot)
        && s2sdk::GetEntityOwner(proxy) == player.pawn_handle
        && LivePlayer::resolve_present(player.slot) == Some(player)
    {
        s2sdk::DropWeapon(player.slot, proxy, &ZERO, &ZERO);
    }
    if s2sdk::IsValidEntHandle(s2sdk::GetEntityOwner(proxy)) {
        clear_owner(proxy)?;
    }
    if s2sdk::IsValidEntHandle(s2sdk::GetEntityParent(proxy)) {
        clear_parent(proxy)?;
    }
    if releasing {
        s2sdk::TeleportEntity(proxy, &position, &ZERO, &ZERO);
    }
    sync_carry_origin(bomb, s2sdk::GetEntityAbsOrigin(proxy), ZERO)?;
    if s2sdk::IsValidEntHandle(s2sdk::GetEntityOwner(proxy)) {
        return Err("carry proxy world release or bomb attachment failed".into());
    }
    Ok(CarryPlacement {
        player: None,
        pose: CarryPose::World,
    })
}

fn sync_carry_origin(bomb: i32, position: Vec3, angles: Vec3) -> Result<(), String> {
    if s2sdk::IsValidEntHandle(s2sdk::GetEntityParent(bomb)) {
        clear_parent(bomb)?;
    }
    // Radar and C4 flash use the root network origin, not a parent's moving transform.
    s2sdk::TeleportEntity(bomb, &position, &angles, &ZERO);
    if s2sdk::GetEntityParent(bomb) != -1 {
        return Err("carry bomb must retain a root world origin".into());
    }
    Ok(())
}

fn cleanup_owned(bomb: i32, chicken: Option<i32>, proxy: Option<i32>) -> Result<Vec<i32>, String> {
    let mut failures = Vec::new();
    if s2sdk::IsValidEntHandle(bomb) {
        let parent = s2sdk::GetEntityParent(bomb);
        if [chicken, proxy].contains(&Some(parent)) && s2sdk::IsValidEntHandle(parent) {
            let position = s2sdk::GetEntityAbsOrigin(bomb);
            if let Err(error) = clear_parent(bomb) {
                failures.push(error);
            }
            s2sdk::SetEntityAbsOrigin(bomb, &position);
        }
    }
    let mut retiring = Vec::new();
    for handle in [chicken, proxy].into_iter().flatten() {
        if s2sdk::IsValidEntHandle(bomb) && s2sdk::GetEntityParent(bomb) == handle {
            failures.push("planted bomb remains attached to owned helper".into());
            continue;
        }
        if let Err(error) = remove_owned(handle) {
            failures.push(error);
        } else if s2sdk::IsValidEntHandle(handle) {
            retiring.push(handle);
        }
    }
    if failures.is_empty() { Ok(retiring) } else { Err(failures.join("; ")) }
}

fn clear_parent(handle: i32) -> Result<(), String> {
    let position = s2sdk::GetEntityAbsOrigin(handle);
    // S2SDK SetEntityParent rejects an invalid parent instead of detaching.
    s2sdk::AcceptEntityInput(
        handle,
        &Str::from_str("ClearParent"),
        -1,
        -1,
        &plugify::Var::default(),
        s2sdk::FieldType::Auto,
        0,
    );
    s2sdk::SetEntityAbsOrigin(handle, &position);
    if s2sdk::IsValidEntHandle(s2sdk::GetEntityParent(handle)) {
        return Err("entity parent cleanup failed".into());
    }
    Ok(())
}

fn clear_owner(handle: i32) -> Result<(), String> {
    // SetEntityOwner also rejects an invalid handle; use the typed handle setter to release it.
    s2sdk::SetEntSchemaEnt2(
        s2sdk::EntHandleToEntPointer(handle),
        &Str::from_str("CBaseEntity"),
        &Str::from_str("m_hOwnerEntity"),
        -1,
        true,
        0,
    );
    if s2sdk::IsValidEntHandle(s2sdk::GetEntityOwner(handle)) {
        return Err("carry proxy owner cleanup failed".into());
    }
    Ok(())
}

fn remove_owned(handle: i32) -> Result<i32, String> {
    if !s2sdk::IsValidEntHandle(handle) {
        return Ok(handle);
    }
    let owner = s2sdk::GetEntityOwner(handle);
    if s2sdk::IsValidEntHandle(owner) {
        let player = (0..s2sdk::GetMaxClients())
            .filter_map(LivePlayer::resolve_present)
            .find(|player| player.pawn_handle == owner);
        if let Some(player) = player
            && s2sdk::IsClientInGame(player.slot)
        {
            s2sdk::DropWeapon(player.slot, handle, &ZERO, &ZERO);
        }
        if s2sdk::IsValidEntHandle(handle) && s2sdk::IsValidEntHandle(s2sdk::GetEntityOwner(handle)) {
            clear_owner(handle)?;
        }
    }
    s2sdk::RemoveEntity(handle);
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use funrounds_core::{Capabilities, RoundController};

    fn pending_helper(kind: BombKind) -> (Bombs, Action) {
        let round = RoundController::new(None, Capabilities::default()).unwrap().generation();
        let bomb = 0x11223344;
        let mut bombs = Bombs {
            planted: Some(Planted {
                handle: bomb,
                round,
                kind,
                planter: None,
                chicken: None,
                proxy: None,
                placement: None,
                spawn_pending: true,
                placement_pending: false,
                cleanup_pending: false,
            }),
            ..Bombs::default()
        };
        let action = match kind {
            BombKind::Chicken => ActionKind::SpawnChicken { round, bomb },
            BombKind::Carry => {
                let mut players = PlayerTracker::default();
                players.connect(1);
                let player = players.spawn(1, 1, 0x1100).unwrap();
                ActionKind::SpawnProxy {
                    round,
                    bomb,
                    carrier: PlayerTask { player, round },
                }
            }
            _ => unreachable!(),
        };
        bombs.queue(action);
        let action = bombs.take_actions().pop().unwrap();
        (bombs, action)
    }

    #[test]
    fn partial_chicken_and_proxy_failures_keep_the_bomb_relationship_through_cleanup_retry() {
        for (kind, mechanism) in [(BombKind::Chicken, Mechanism::ChickenBomb), (BombKind::Carry, Mechanism::CarryBomb)] {
            let (mut bombs, spawn) = pending_helper(kind);
            let helper = 0x81002030u32 as i32;
            let fault = Fault::mechanism(mechanism, "helper setup failed after creation");
            assert_eq!(
                bombs.action_finished(
                    spawn,
                    Ok(ActionResult::CreationFailed {
                        created: Some(helper),
                        error: fault.clone(),
                    }),
                    true
                ),
                Err(fault)
            );
            let cleanup = bombs.take_actions().pop().unwrap();
            assert!(matches!(cleanup.kind, ActionKind::Cleanup { bomb: 0x11223344, .. }));
            let planted = bombs.planted.unwrap();
            assert_eq!(planted.chicken.or(planted.proxy), Some(helper));
            bombs.cleanup_requested = true;
            let restore = Fault::global("parent release failed");
            assert_eq!(bombs.action_finished(cleanup, Err(restore.clone()), true), Err(restore));
            assert!(!bombs.is_clean());
            let retry = bombs.take_actions().pop().unwrap();
            assert_ne!(cleanup.id, retry.id);
            assert_eq!(cleanup.kind, retry.kind);
            bombs.action_finished(cleanup, Ok(ActionResult::Cleaned(Vec::new())), true).unwrap();
            assert_eq!(bombs.planted.unwrap().chicken.or(bombs.planted.unwrap().proxy), Some(helper));
            assert!(bombs.pending_actions.contains_key(&retry.id));
            bombs.action_finished(retry, Ok(ActionResult::Cleaned(Vec::new())), true).unwrap();
            assert!(bombs.is_clean());
        }
    }

    #[test]
    fn cleanup_waits_for_an_in_flight_creation_and_treats_its_late_error_as_cancellation() {
        let (mut bombs, spawn) = pending_helper(BombKind::Chicken);
        bombs.cleanup().unwrap();
        assert!(bombs.take_actions().is_empty());
        assert!(!bombs.is_clean());
        bombs
            .action_finished(
                spawn,
                Ok(ActionResult::CreationFailed {
                    created: Some(1024),
                    error: Fault::mechanism(Mechanism::ChickenBomb, "old round result"),
                }),
                false,
            )
            .unwrap();
        let cleanup = bombs.take_actions().pop().unwrap();
        assert!(matches!(cleanup.kind, ActionKind::Cleanup { chicken: Some(1024), .. }));
        bombs.action_finished(spawn, Ok(ActionResult::Chicken(2048)), true).unwrap();
        assert_eq!(bombs.planted.unwrap().chicken, Some(1024));
        bombs.action_finished(cleanup, Ok(ActionResult::Cleaned(Vec::new())), true).unwrap();
        assert!(bombs.is_clean());
    }

    #[test]
    fn cancelled_current_bomb_creation_preserves_global_failures_with_or_without_partial_entities() {
        for created in [None, Some(1024)] {
            let (mut bombs, spawn) = pending_helper(BombKind::Carry);
            bombs.cleanup().unwrap();
            let error = Fault::global("unknown ownership during cancelled native call");
            let fault = bombs
                .action_finished(
                    spawn,
                    Ok(ActionResult::CreationFailed {
                        created,
                        error: error.clone(),
                    }),
                    false,
                )
                .err()
                .unwrap();
            assert_eq!(fault, error);
            let mut faults = crate::fault::Faults::new(Capabilities::default());
            faults.record(&fault);
            assert_eq!(faults.recovery, crate::fault::Recovery::Stopped);
            if let Some(handle) = created {
                assert_eq!(bombs.planted.unwrap().proxy, Some(handle));
                assert!(!bombs.is_clean());
            }
        }
    }

    #[test]
    fn old_map_partial_creations_and_explosions_cannot_claim_reused_handles() {
        let (mut bombs, spawn) = pending_helper(BombKind::Carry);
        let explosion = Explosion {
            bomb: 0x11223344,
            map_epoch: bombs.map_epoch,
            round: bombs.planted.unwrap().round,
            position: ZERO,
        };
        bombs.active_explosion = Some(explosion.bomb);
        bombs.discard_map_state();
        bombs
            .action_finished(
                spawn,
                Ok(ActionResult::CreationFailed {
                    created: Some(1024),
                    error: Fault::global("old map result"),
                }),
                true,
            )
            .unwrap();
        assert!(!bombs.finish_explosion(explosion, 1024));
        assert!(bombs.is_clean());
    }

    #[test]
    fn moving_explosion_runs_once_and_suppresses_only_its_exact_native_c4() {
        let round = RoundController::new(None, Capabilities::default()).unwrap().generation();
        let explosion = Explosion {
            bomb: 0x12340042,
            map_epoch: 0,
            round,
            position: ZERO,
        };
        let mut bombs = Bombs::default();
        bombs.remember_explosion(explosion);
        bombs.remember_explosion(Explosion {
            position: Vec3 { x: 99.0, ..ZERO },
            ..explosion
        });
        assert_eq!(bombs.pending_explosion.unwrap().position.x, 0.0);
        assert!(bombs.blocks_native_explosion(explosion.bomb));
        assert!(!bombs.blocks_native_explosion(0x22340042));
        bombs.active_explosion = Some(explosion.bomb);
        assert!(!bombs.blocks_native_explosion(explosion.bomb));
        bombs.active_explosion = None;
        bombs.pending_explosion = None;
        bombs.remember_explosion(explosion);
        assert!(bombs.pending_explosion.is_none());
        assert!(bombs.blocks_native_explosion(explosion.bomb));
        bombs.discard_map_state();
        assert!(!bombs.blocks_native_explosion(explosion.bomb));
        assert!(bombs.is_clean());
    }

    #[test]
    fn moving_explosion_does_not_cross_round_generations() {
        let mut controller = RoundController::new(None, Capabilities::default()).unwrap();
        let old_round = controller.generation();
        controller.set_enabled(false);
        let mut bombs = Bombs::default();
        bombs.remember_explosion(Explosion {
            bomb: 0x12340042,
            map_epoch: 0,
            round: old_round,
            position: ZERO,
        });
        assert!(bombs.take_explosion(controller.generation()).is_none());
        assert!(bombs.active_explosion.is_none());
    }

    #[test]
    fn planted_identity_never_selects_an_arbitrary_first_entity() {
        assert_eq!(unique_new_bomb(&[0x12340042]).unwrap(), 0x12340042);
        assert!(unique_new_bomb(&[]).is_err());
        assert!(unique_new_bomb(&[42, 43]).is_err());
    }

    #[test]
    fn carry_placement_tracks_world_hand_and_back_as_distinct_states() {
        let player = LivePlayer {
            slot: 2,
            controller_handle: 22,
            pawn_handle: 33,
        };
        let world = CarryPlacement {
            player: None,
            pose: CarryPose::World,
        };
        let held = CarryPlacement {
            player: Some(player),
            pose: CarryPose::Hand,
        };
        let back = CarryPlacement {
            player: Some(player),
            pose: CarryPose::Back,
        };
        assert_ne!(world, held);
        assert_ne!(held, back);
    }

    #[test]
    fn defuse_snapshot_selector_scopes_each_bomb_and_pawn() {
        fn begin_attempt(properties: &mut Vec<OwnedProperty>, bomb: i32, pawn: i32) {
            properties.push(OwnedProperty {
                handle: bomb,
                class: PLANTED,
                field: "m_flDefuseCountDown",
                previous: PropertyValue::Float(12.0),
            });
            properties.push(OwnedProperty {
                handle: pawn,
                class: "CCSPlayerPawn",
                field: "m_iProgressBarDuration",
                previous: PropertyValue::Integer(5),
            });
        }
        let mut properties = Vec::new();
        begin_attempt(&mut properties, 10, 20);
        properties.retain(|property| !is_defuse_property(property, 10, 20));
        begin_attempt(&mut properties, 10, 21);
        assert_eq!(properties.len(), 2);
        assert_eq!(properties[0].handle, 10);
        assert_eq!(properties[1].handle, 21);
        assert!(is_defuse_property(&properties[1], 10, 21));
    }

    #[test]
    fn applied_defuse_keeps_its_identity_until_abort_and_a_new_attempt_is_due() {
        let rounds = RoundController::new(None, Capabilities::default()).unwrap();
        let mut players = PlayerTracker::default();
        players.connect(2);
        let task = PlayerTask {
            player: players.spawn(2, 1, 0x11223344).unwrap(),
            round: rounds.generation(),
        };
        let action = TimedAction {
            task,
            handle: 99,
            tick: 10,
            kit: true,
        };
        let mut bombs = Bombs {
            defusing: Some(DefuseAction { action, apply_pending: true }),
            ..Bombs::default()
        };
        let defuse = bombs.defusing.as_mut().unwrap();
        assert!(defuse.take_due(10).is_none());
        assert_eq!(defuse.take_due(11).unwrap().task, task);
        assert!(defuse.take_due(12).is_none());
        assert_eq!(bombs.defusing.as_ref().unwrap().action.handle, 99);
        bombs.abort_defuse().unwrap();
        assert!(bombs.defusing.is_none());
        bombs.defusing = Some(DefuseAction {
            action: TimedAction {
                tick: 20,
                kit: false,
                ..action
            },
            apply_pending: true,
        });
        assert!(!bombs.defusing.as_mut().unwrap().take_due(21).unwrap().kit);
    }

    #[test]
    fn unfinished_bomb_cleanup_restores_its_timer_visibility_and_progress_snapshots() {
        let planted_timer = OwnedProperty {
            handle: 10,
            class: PLANTED,
            field: "m_flC4Blow",
            previous: PropertyValue::Float(45.0),
        };
        let c4_arming = OwnedProperty {
            handle: 11,
            class: C4,
            field: "m_fArmedTime",
            previous: PropertyValue::Float(0.0),
        };
        let carry_visibility = OwnedProperty {
            handle: 10,
            class: "CBaseEntity",
            field: "m_fEffects",
            previous: PropertyValue::Integer(0),
        };
        let progress = OwnedProperty {
            handle: 21,
            class: "CCSPlayerPawn",
            field: "m_iProgressBarDuration",
            previous: PropertyValue::Integer(0),
        };
        let unrelated = OwnedProperty {
            handle: 21,
            class: "CCSPlayerPawn",
            field: "m_iHealth",
            previous: PropertyValue::Integer(100),
        };

        assert!(is_finished_bomb_property(&planted_timer, 10));
        assert!(is_finished_bomb_property(&carry_visibility, 10));
        assert!(!is_finished_bomb_property(&carry_visibility, 20));
        assert!(is_finished_bomb_property(&c4_arming, 10));
        assert!(is_finished_bomb_property(&progress, 10));
        assert!(!is_finished_bomb_property(&unrelated, 10));
    }

    #[test]
    fn completed_defuse_consumes_only_its_bomb_and_exact_pawn_snapshots() {
        let bomb = 0x11220010;
        let pawn = 0x33440020;
        for (handle, class, field, consumed) in [
            (bomb, PLANTED, "m_flC4Blow", true),
            (bomb, PLANTED, "m_flTimerLength", true),
            (bomb, PLANTED, "m_flDefuseLength", true),
            (bomb, PLANTED, "m_flDefuseCountDown", true),
            (bomb, "CBaseEntity", "m_fEffects", true),
            (pawn, "CCSPlayerPawn", "m_iProgressBarDuration", true),
            (pawn, "CCSPlayerPawn", "m_flProgressBarStartTime", true),
            (pawn, "CBaseEntity", "m_fEffects", false),
            (0x55660020, "CCSPlayerPawn", "m_iProgressBarDuration", false),
            (0x77880010, PLANTED, "m_flDefuseCountDown", false),
            (0x77880010, "CBaseEntity", "m_fEffects", false),
            (0x99000030u32 as i32, C4, "m_fArmedTime", false),
        ] {
            let property = OwnedProperty {
                handle,
                class,
                field,
                previous: PropertyValue::Integer(5),
            };
            assert_eq!(is_consumed_defuse_property(&property, bomb, Some(pawn)), consumed, "{class}.{field}");
            assert_eq!(is_consumed_defuse_property(&property, bomb, None), handle == bomb);
        }
    }

    #[test]
    fn chicken_action_cycle_tracks_bomb_round_cleanup_and_entity_ownership() {
        let mut rounds = RoundController::new(None, Capabilities::default()).unwrap();
        let round = rounds.generation();
        let bomb = 0x11223344;
        let mut bombs = Bombs {
            planted: Some(Planted {
                handle: bomb,
                round,
                kind: BombKind::Chicken,
                planter: None,
                chicken: None,
                proxy: None,
                placement: None,
                spawn_pending: true,
                placement_pending: false,
                cleanup_pending: false,
            }),
            ..Bombs::default()
        };
        let spawn_id = bombs.queue(ActionKind::SpawnChicken { round, bomb });
        let spawn = bombs.take_actions().pop().unwrap();
        assert_eq!(spawn.id, spawn_id);
        assert!(bombs.action_valid(spawn, round, &PlayerTracker::default()));
        let wrong_handle = Action {
            kind: ActionKind::SpawnChicken { round, bomb: bomb + 1 },
            ..spawn
        };
        assert!(!bombs.action_valid(wrong_handle, round, &PlayerTracker::default()));
        rounds.reset();
        assert!(!bombs.action_valid(spawn, rounds.generation(), &PlayerTracker::default()));
        assert!(bombs.action_valid(spawn, round, &PlayerTracker::default()));

        let chicken = 0x22334455;
        bombs.action_finished(spawn, Ok(ActionResult::Chicken(chicken)), true).unwrap();
        assert_eq!(bombs.planted.unwrap().chicken, Some(chicken));
        assert!(!bombs.planted.unwrap().spawn_pending);
        bombs.cleanup().unwrap();
        let cleanup = bombs.take_actions().pop().unwrap();
        assert!(bombs.action_valid(cleanup, round, &PlayerTracker::default()));
        bombs.action_finished(cleanup, Ok(ActionResult::Cleaned(Vec::new())), true).unwrap();
        assert!(bombs.is_clean());
    }

    #[test]
    fn chicken_bomb_mount_is_lowered_from_the_floating_offset() {
        assert!((0.0..15.0).contains(&CHICKEN_BOMB_OFFSET_Z));
    }

    #[test]
    fn completed_chicken_survives_round_end_and_retires_through_owned_cleanup() {
        let (mut bombs, spawn) = pending_helper(BombKind::Chicken);
        let chicken = 0x81223345u32 as i32;
        bombs.action_finished(spawn, Ok(ActionResult::Chicken(chicken)), true).unwrap();
        let completed = bombs.planted.take().unwrap();
        bombs.completed_chicken = Some(completed);

        bombs.cleanup_after_round_end().unwrap();
        assert!(bombs.take_actions().is_empty());
        assert!(!bombs.is_clean());
        assert_eq!(bombs.completed_chicken.unwrap().chicken, Some(chicken));

        bombs.retire_completed_chicken(|handle| {
            assert_eq!(handle, chicken);
            true
        });
        assert!(bombs.completed_chicken.is_none());
        let cleanup = bombs.take_actions().pop().unwrap();
        assert_eq!(
            cleanup.kind,
            ActionKind::Cleanup {
                round: completed.round,
                bomb: completed.handle,
                chicken: Some(chicken),
                proxy: None,
            }
        );
        assert!(bombs.action_valid(cleanup, completed.round, &PlayerTracker::default()));
        let fault = Fault::global("completed chicken parent cleanup failed");
        assert_eq!(bombs.action_finished(cleanup, Err(fault.clone()), true), Err(fault));
        let retry = bombs.take_actions().pop().unwrap();
        assert!(!bombs.is_clean());
        bombs.action_finished(retry, Ok(ActionResult::Cleaned(vec![chicken])), true).unwrap();
        bombs.retain_unacknowledged_removals(|_| false);
        assert!(!bombs.is_clean());
        bombs.retain_unacknowledged_removals(|handle| handle == chicken);
        assert!(bombs.is_clean());
    }

    #[test]
    fn native_round_cleanup_of_completed_chicken_does_not_delay_the_next_round() {
        let (mut bombs, spawn) = pending_helper(BombKind::Chicken);
        let chicken = 0x81223345u32 as i32;
        bombs.action_finished(spawn, Ok(ActionResult::Chicken(chicken)), true).unwrap();
        bombs.completed_chicken = bombs.planted.take();

        bombs.retire_completed_chicken(|handle| {
            assert_eq!(handle, chicken);
            assert_ne!(handle, 0x91223345u32 as i32);
            false
        });
        assert!(bombs.take_actions().is_empty());
        assert!(bombs.is_clean());
    }

    #[test]
    fn completed_chicken_query_panic_keeps_ownership_for_cleanup_retry() {
        let (mut bombs, spawn) = pending_helper(BombKind::Chicken);
        let chicken = 0x81223345u32 as i32;
        bombs.action_finished(spawn, Ok(ActionResult::Chicken(chicken)), true).unwrap();
        bombs.completed_chicken = bombs.planted.take();
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bombs.retire_completed_chicken(|_| panic!("native handle query failed"));
        }));
        assert!(failed.is_err());
        assert_eq!(bombs.completed_chicken.as_ref().and_then(|bomb| bomb.chicken), Some(chicken));
        assert!(bombs.take_actions().is_empty());
        assert!(!bombs.is_clean());

        bombs.retire_completed_chicken(|_| true);
        let cleanup = bombs.take_actions().pop().unwrap();
        assert!(bombs.completed_chicken.is_none());
        assert!(!bombs.is_clean());
        bombs.action_finished(cleanup, Ok(ActionResult::Cleaned(Vec::new())), true).unwrap();
        assert!(bombs.is_clean());
    }

    #[test]
    fn completed_chicken_cleanup_panic_reports_global_fault_and_requeues_owned_action() {
        let (mut bombs, spawn) = pending_helper(BombKind::Chicken);
        let chicken = 0x81223345u32 as i32;
        bombs.action_finished(spawn, Ok(ActionResult::Chicken(chicken)), true).unwrap();
        bombs.completed_chicken = bombs.planted.take();
        bombs.retire_completed_chicken(|_| true);
        let cleanup = bombs.take_actions().pop().unwrap();

        let result = cleanup_result(|| panic!("native cleanup failed after queue drain"));
        let error = bombs.action_finished(cleanup, result, true).err().unwrap();
        let mut faults = crate::fault::Faults::new(Capabilities::default());
        faults.record(&error);
        assert_eq!(faults.recovery, crate::fault::Recovery::Stopped);
        assert!(!bombs.is_clean());
        let retry = bombs.take_actions().pop().unwrap();
        assert_ne!(retry.id, cleanup.id);
        assert_eq!(retry.kind, cleanup.kind);
        bombs.action_finished(retry, Ok(ActionResult::Cleaned(Vec::new())), true).unwrap();
        assert!(bombs.is_clean());
    }

    #[test]
    fn round_end_still_cleans_carry_and_does_not_keep_an_in_flight_chicken() {
        let (mut carry, spawn) = pending_helper(BombKind::Carry);
        carry.action_finished(spawn, Ok(ActionResult::Proxy(1024)), true).unwrap();
        carry.cleanup_after_round_end().unwrap();
        let cleanup = carry.take_actions().pop().unwrap();
        assert!(matches!(
            cleanup.kind,
            ActionKind::Cleanup {
                proxy: Some(1024),
                chicken: None,
                ..
            }
        ));
        assert!(carry.completed_chicken.is_none());

        let (mut chicken, spawn) = pending_helper(BombKind::Chicken);
        chicken.cleanup_after_round_end().unwrap();
        assert!(chicken.take_actions().is_empty());
        chicken.action_finished(spawn, Ok(ActionResult::Chicken(2048)), false).unwrap();
        let cleanup = chicken.take_actions().pop().unwrap();
        assert!(matches!(cleanup.kind, ActionKind::Cleanup { chicken: Some(2048), .. }));
        assert!(chicken.completed_chicken.is_none());
    }

    #[test]
    fn completed_chicken_ownership_does_not_cross_maps() {
        let (mut bombs, spawn) = pending_helper(BombKind::Chicken);
        bombs.action_finished(spawn, Ok(ActionResult::Chicken(1024)), true).unwrap();
        bombs.completed_chicken = bombs.planted.take();
        bombs.discard_map_state();
        assert!(bombs.completed_chicken.is_none());
        assert!(bombs.is_clean());
        bombs.retire_completed_chicken(|_| panic!("old map handles must not be queried"));
    }

    #[test]
    fn carry_state_machine_moves_from_world_to_hand_back_and_world() {
        let round = RoundController::new(None, Capabilities::default()).unwrap().generation();
        let bomb = 0x11223344;
        let proxy = 0x22334455;
        let player = LivePlayer {
            slot: 2,
            controller_handle: 22,
            pawn_handle: 33,
        };
        let mut bombs = Bombs {
            planted: Some(Planted {
                handle: bomb,
                round,
                kind: BombKind::Carry,
                planter: None,
                chicken: None,
                proxy: Some(proxy),
                placement: Some(CarryPlacement {
                    player: None,
                    pose: CarryPose::World,
                }),
                spawn_pending: false,
                placement_pending: false,
                cleanup_pending: false,
            }),
            ..Bombs::default()
        };
        for (target, previous_owner) in [
            (
                CarryPlacement {
                    player: Some(player),
                    pose: CarryPose::Hand,
                },
                None,
            ),
            (
                CarryPlacement {
                    player: Some(player),
                    pose: CarryPose::Back,
                },
                None,
            ),
            (
                CarryPlacement {
                    player: None,
                    pose: CarryPose::World,
                },
                Some(player),
            ),
        ] {
            bombs.planted.as_mut().unwrap().placement_pending = true;
            let id = bombs.queue(ActionKind::PlaceCarry {
                round,
                bomb,
                proxy,
                target,
                previous_owner,
            });
            let action = bombs.take_actions().pop().unwrap();
            assert_eq!(action.id, id);
            assert!(bombs.action_valid(action, round, &PlayerTracker::default()));
            bombs.action_finished(action, Ok(ActionResult::Placed(target)), true).unwrap();
            assert_eq!(bombs.planted.unwrap().placement, Some(target));
            assert!(!bombs.planted.unwrap().placement_pending);
        }
        assert_eq!(bombs.planted.unwrap().placement.unwrap().pose, CarryPose::World);
    }

    #[test]
    fn pre_disconnect_release_matches_full_pawn_and_round_and_runs_once() {
        let round = RoundController::new(None, Capabilities::default()).unwrap().generation();
        let player = LivePlayer {
            slot: 2,
            controller_handle: 22,
            pawn_handle: 0x10033,
        };
        let mut bombs = Bombs {
            planted: Some(Planted {
                handle: 44,
                round,
                kind: BombKind::Carry,
                planter: None,
                chicken: None,
                proxy: Some(55),
                placement: Some(CarryPlacement {
                    player: Some(player),
                    pose: CarryPose::Back,
                }),
                spawn_pending: false,
                placement_pending: false,
                cleanup_pending: false,
            }),
            ..Bombs::default()
        };
        let mut next_round = RoundController::new(None, Capabilities::default()).unwrap();
        next_round.reset();
        assert!(bombs.queue_carrier_release(next_round.generation(), player, player.pawn_handle).is_none());
        let mut players = PlayerTracker::default();
        players.connect(player.slot);
        players.spawn(player.slot, 1, player.pawn_handle as u32);
        players.death(player.slot);
        assert!(players.identity(player.slot).is_none());
        assert!(players.connection_identity(player.slot).is_some());
        let reused = LivePlayer {
            pawn_handle: 0x20033,
            ..player
        };
        assert!(bombs.queue_carrier_release(round, reused, player.pawn_handle).is_none());
        bombs.cleanup_requested = true;
        assert!(bombs.queue_carrier_release(round, player, player.pawn_handle).is_none());
        bombs.cleanup_requested = false;
        let action = bombs.queue_carrier_release(round, player, player.pawn_handle).unwrap();
        assert!(bombs.take_actions().is_empty());
        assert!(bombs.action_valid(action, round, &PlayerTracker::default()));
        assert!(bombs.queue_carrier_release(round, player, player.pawn_handle).is_none());
        let ActionKind::PlaceCarry { target, previous_owner, .. } = action.kind else {
            panic!("world release required")
        };
        assert_eq!(
            target,
            CarryPlacement {
                player: None,
                pose: CarryPose::World
            }
        );
        assert_eq!(previous_owner, Some(player));
        bombs.action_finished(action, Ok(ActionResult::Placed(target)), true).unwrap();
        assert_eq!(bombs.planted.unwrap().placement, Some(target));
    }

    #[test]
    fn shutdown_deletion_ack_keeps_unmarked_entities_and_pending_actions() {
        let round = RoundController::new(None, Capabilities::default()).unwrap().generation();
        let marked = 0x10001;
        let unmarked = 0x10002;
        let reused_index = 0x20001;
        let mut bombs = Bombs {
            retiring: HashSet::from([marked, unmarked]),
            retiring_pending: HashSet::from([marked]),
            ..Bombs::default()
        };
        bombs.queue(ActionKind::RemoveEntity { handle: marked });
        let action = bombs.take_actions().pop().unwrap();
        assert!(bombs.action_valid(action, round, &PlayerTracker::default()));
        assert!(!bombs.is_clean());
        bombs.retain_unacknowledged_removals(|handle| {
            assert_ne!(handle, reused_index);
            handle == marked
        });
        assert_eq!(bombs.retiring, HashSet::from([unmarked]));
        assert!(bombs.retiring_pending.is_empty());
        assert!(!bombs.action_valid(action, round, &PlayerTracker::default()));
        assert!(!bombs.is_clean());
        bombs.action_cancelled(action);
        assert!(!bombs.is_clean());
        bombs.retain_unacknowledged_removals(|_| false);
        assert!(!bombs.is_clean());
        bombs.retain_unacknowledged_removals(|handle| handle == unmarked);
        assert!(bombs.is_clean());
    }

    #[test]
    fn map_discard_forgets_old_handle_snapshots_and_in_flight_actions() {
        let round = RoundController::new(None, Capabilities::default()).unwrap().generation();
        let mut bombs = Bombs {
            planted: Some(Planted {
                handle: 0x11223344,
                round,
                kind: BombKind::Chicken,
                planter: None,
                chicken: Some(0x22334455),
                proxy: None,
                placement: None,
                spawn_pending: true,
                placement_pending: false,
                cleanup_pending: false,
            }),
            properties: vec![OwnedProperty {
                handle: 0x11223344,
                class: PLANTED,
                field: "m_flC4Blow",
                previous: PropertyValue::Float(40.0),
            }],
            ..Bombs::default()
        };
        bombs.queue(ActionKind::SpawnChicken { round, bomb: 0x11223344 });
        let stale = bombs.take_actions().pop().unwrap();
        bombs.discard_map_state();

        assert!(bombs.is_clean());
        assert!(!bombs.action_valid(stale, round, &PlayerTracker::default()));
        bombs.action_finished(stale, Ok(ActionResult::Chicken(0x33445566)), true).unwrap();
        assert!(bombs.retiring.is_empty());
        assert!(bombs.properties.is_empty());
    }
}
