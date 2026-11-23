// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use plugify::{Vec3, Vec4};
use std::collections::{HashMap, VecDeque};

const IN_JUMP: u64 = 1 << 1;
const IN_USE: u64 = 1 << 5;
const TRACE_MASK: i32 = 798737;
const GRAPPLE_GRAVITY: f32 = 0.001;
const BEAM_WIDTH: f64 = 3.0;
const BEAM_COLOR: Vec4 = Vec4 {
    x: 255.0,
    y: 60.0,
    z: 60.0,
    w: 255.0,
};
const WORLD_CLASS: &str = "worldent";
const BEAM_CLASS: &str = "beam";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Input {
    identity: PlayerIdentity,
    sampled_down: bool,
    armed: bool,
    attempt_consumed: bool,
    native_target_won: bool,
}

impl Input {
    fn new(identity: PlayerIdentity, down: bool) -> Self {
        Self {
            identity,
            sampled_down: down,
            armed: !down,
            attempt_consumed: down,
            native_target_won: false,
        }
    }

    fn sample(&mut self, down: bool) {
        if !down {
            self.sampled_down = false;
            self.armed = true;
            self.attempt_consumed = false;
            self.native_target_won = false;
        } else if !self.sampled_down {
            self.sampled_down = true;
            self.attempt_consumed = !self.armed;
        }
    }

    fn can_attempt(&self) -> bool {
        self.sampled_down && self.armed && !self.attempt_consumed && !self.native_target_won
    }
}

#[derive(Clone, Copy)]
struct Observation {
    identity: PlayerIdentity,
    tick: i32,
    target: i32,
    in_use: bool,
    target_valid: bool,
    chicken_claimed: bool,
}

impl Observation {
    fn new(identity: PlayerIdentity, tick: i32, target: i32, in_use: bool) -> Self {
        Self {
            identity,
            tick,
            target,
            in_use,
            // UseTarget's native callback is invoked only after it has checked
            // IsValid, round-tripped the complete handle, and committed the
            // selected target. Preserve that callback-time winner even if the
            // entity is removed before this tick's observations are drained.
            target_valid: in_use && target != -1,
            chicken_claimed: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectorDecision {
    NativeTarget,
    Empty,
}

#[derive(Default)]
struct SelectorBatch {
    native_target: bool,
    empty_target: bool,
}

impl SelectorBatch {
    fn observe(&mut self, observation: Observation) {
        if !observation.in_use {
            return;
        }
        if observation.chicken_claimed || observation.target_valid {
            self.native_target = true;
        } else if observation.target == -1 {
            self.empty_target = true;
        }
    }

    fn decision(&self) -> Option<SelectorDecision> {
        if self.native_target {
            Some(SelectorDecision::NativeTarget)
        } else if self.empty_target {
            Some(SelectorDecision::Empty)
        } else {
            None
        }
    }
}

struct Active {
    task: PlayerTask,
    player: LivePlayer,
    anchor: Vec3,
    settings: funrounds_core::GrappleSettings,
    beam: Option<i32>,
}

#[derive(Default)]
pub(super) struct State {
    input: HashMap<i32, Input>,
    observations: VecDeque<Observation>,
    active: HashMap<i32, Active>,
    retired_beams: HashMap<i32, bool>,
    bomb_use: HashMap<i32, PlayerIdentity>,
}

#[derive(Clone, Copy)]
pub(crate) enum Action {
    Acquire {
        task: PlayerTask,
        player: LivePlayer,
        settings: funrounds_core::GrappleSettings,
    },
    Pull {
        task: PlayerTask,
        player: LivePlayer,
        anchor: Vec3,
        beam: i32,
        settings: funrounds_core::GrappleSettings,
    },
    RemoveBeam {
        handle: i32,
    },
}

impl State {
    fn sample(&mut self, identity: PlayerIdentity, down: bool) {
        let state = self.input.entry(identity.slot).or_insert_with(|| Input::new(identity, down));
        if state.identity != identity {
            *state = Input::new(identity, down);
        } else {
            state.sample(down);
        }
    }

    fn queue_observation(&mut self, observation: Observation) {
        if observation.in_use && (observation.target_valid || observation.chicken_claimed) {
            self.confirm_native_target(observation.identity);
        }
        self.observations.push_back(observation);
    }

    fn take_closed_tick_observations(&mut self, tick: i32) -> Vec<Observation> {
        let mut closed = Vec::new();
        while self.observations.front().is_some_and(|observation| observation.tick < tick) {
            closed.push(self.observations.pop_front().expect("front observation exists"));
        }
        closed
    }

    fn consume_attempt(&mut self, identity: PlayerIdentity) -> bool {
        let Some(input) = self
            .input
            .get_mut(&identity.slot)
            .filter(|input| input.identity == identity && input.can_attempt())
        else {
            return false;
        };
        input.attempt_consumed = true;
        true
    }

    fn win_native_target(&mut self, identity: PlayerIdentity) {
        if let Some(input) = self
            .input
            .get_mut(&identity.slot)
            .filter(|input| input.identity == identity && input.sampled_down)
        {
            input.attempt_consumed = true;
            input.native_target_won = true;
        }
    }

    fn confirm_native_target(&mut self, identity: PlayerIdentity) {
        let input = self.input.entry(identity.slot).or_insert_with(|| Input::new(identity, false));
        if input.identity != identity {
            *input = Input::new(identity, false);
        }
        input.attempt_consumed = true;
        input.native_target_won = true;
    }

    fn input_allows_action(&self, identity: PlayerIdentity) -> bool {
        self.input
            .get(&identity.slot)
            .is_some_and(|input| input.identity == identity && input.sampled_down && !input.native_target_won)
    }

    fn begin_bomb_use(&mut self, identity: PlayerIdentity) {
        self.bomb_use.insert(identity.slot, identity);
        self.confirm_native_target(identity);
    }

    fn end_bomb_use(&mut self, identity: PlayerIdentity) {
        if self.bomb_use.get(&identity.slot) == Some(&identity) {
            self.bomb_use.remove(&identity.slot);
        }
    }

    fn native_bomb_use(&self, identity: PlayerIdentity) -> bool {
        self.bomb_use.get(&identity.slot) == Some(&identity)
    }

    fn retire_beam(&mut self, handle: i32) {
        if handle != -1 {
            self.retired_beams.entry(handle).or_insert(false);
        }
    }

    fn take_beam_cleanup_actions(&mut self) -> Vec<Action> {
        self.retired_beams.keys().copied().map(|handle| Action::RemoveBeam { handle }).collect()
    }

    fn beam_cleanup_complete(&mut self, handle: i32, complete: bool) {
        if complete {
            self.retired_beams.remove(&handle);
        } else if let Some(requested) = self.retired_beams.get_mut(&handle) {
            *requested = true;
        }
    }

    pub(super) fn is_clean(&self) -> bool {
        self.active.is_empty() && self.retired_beams.is_empty() && self.observations.is_empty() && self.bomb_use.is_empty()
    }

    fn claim_chicken_target(&mut self, identity: PlayerIdentity, tick: i32, target: i32) {
        let matched = if let Some(observation) =
            self.observations.iter_mut().rev().find(|observation| {
                observation.identity == identity && observation.tick == tick && observation.target == target && observation.in_use
            }) {
            observation.chicken_claimed = true;
            true
        } else {
            let mut observation = Observation::new(identity, tick, target, true);
            observation.chicken_claimed = true;
            self.queue_observation(observation);
            false
        };
        if matched {
            self.win_native_target(identity);
        }
    }
}

pub(super) fn capability_probe(reasons: &mut Vec<String>) -> bool {
    let mut probe = crate::capability::Probe::default();
    crate::capability::imports!(probe;
        __s2sdk_GetClientButtons, __s2sdk_GetGameTickCount, __s2sdk_GetClientEyePosition, __s2sdk_GetClientEyeAngles,
        __s2sdk_TraceLine, __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer, __s2sdk_EntIndexToEntHandle,
        __s2sdk_GetEntityClassname, __s2sdk_GetEntityMoveType, __s2sdk_GetEntityAbsOrigin, __s2sdk_GetEntityAbsVelocity,
        __s2sdk_SetEntityAbsVelocity, __s2sdk_GetClientGravity, __s2sdk_SetClientGravity,
        __s2sdk_GetSchemaOffset, __s2sdk_CreateEntityByName, __s2sdk_DispatchSpawn, __s2sdk_TeleportEntity,
        __s2sdk_RemoveEntity, __s2sdk_SetEntSchemaFloat2, __s2sdk_GetEntSchemaFloat2,
        __s2sdk_SetEntSchemaVector3D2, __s2sdk_GetEntSchemaVector3D2, __s2sdk_SetEntSchemaColor,
        __s2sdk_GetEntSchemaColor,
    );
    probe.require(
        unsafe { crate::funrounds_native::__funrounds_native_HasUseTargetSupport }.is_some(),
        "native:HasUseTargetSupport",
    );
    probe.require(
        unsafe { crate::funrounds_native::__funrounds_native_HookUseTarget }.is_some(),
        "native:HookUseTarget",
    );
    probe.require(
        unsafe { crate::funrounds_native::__funrounds_native_UnhookUseTarget }.is_some(),
        "native:UnhookUseTarget",
    );
    probe.schema("CBeam", "m_vecEndPos");
    probe.schema("CBeam", "m_fWidth");
    probe.schema("CBaseModelEntity", "m_clrRender");
    probe.record("grapple", reasons)
}

fn finite_vec(value: Vec3) -> bool {
    value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
}

fn vector_near(actual: Vec3, expected: Vec3, tolerance: f32) -> bool {
    finite_vec(actual)
        && finite_vec(expected)
        && (actual.x - expected.x).abs() <= tolerance
        && (actual.y - expected.y).abs() <= tolerance
        && (actual.z - expected.z).abs() <= tolerance
}

fn color_near(actual: Vec4, expected: Vec4, tolerance: f32) -> bool {
    (actual.x - expected.x).abs() <= tolerance
        && (actual.y - expected.y).abs() <= tolerance
        && (actual.z - expected.z).abs() <= tolerance
        && (actual.w - expected.w).abs() <= tolerance
}

fn beam_pointer(handle: i32) -> Option<usize> {
    if !s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    (pointer != 0).then_some(pointer)
}

fn trace_world_anchor(player: LivePlayer, settings: funrounds_core::GrappleSettings) -> Option<Vec3> {
    let start = s2sdk::GetClientEyePosition(player.slot);
    let angles = s2sdk::GetClientEyeAngles(player.slot);
    if !finite_vec(start) || !finite_vec(angles) {
        return None;
    }
    let pitch = angles.x.to_radians();
    let yaw = angles.y.to_radians();
    let forward = Vec3 {
        x: pitch.cos() * yaw.cos(),
        y: pitch.cos() * yaw.sin(),
        z: -pitch.sin(),
    };
    let end = Vec3 {
        x: start.x + forward.x * settings.max_distance,
        y: start.y + forward.y * settings.max_distance,
        z: start.z + forward.z * settings.max_distance,
    };
    let mut hit_position = Vec3::default();
    let mut fraction = 1.0;
    let mut hit = false;
    let mut hit_handle = -1;
    let mut start_solid = false;
    let traced = s2sdk::TraceLine(
        &start,
        &end,
        TRACE_MASK,
        player.pawn_handle,
        &mut hit_position,
        &mut fraction,
        &mut hit,
        &mut hit_handle,
        &mut start_solid,
    );
    let world = s2sdk::EntIndexToEntHandle(0);
    (traced
        && hit
        && !start_solid
        && fraction.is_finite()
        && (0.0..1.0).contains(&fraction)
        && finite_vec(hit_position)
        && hit_handle == world
        && beam_pointer(hit_handle).is_some()
        && s2sdk::GetEntityClassname(hit_handle).as_str() == WORLD_CLASS)
        .then_some(hit_position)
}

fn create_beam(player: LivePlayer, anchor: Vec3) -> Result<i32, (String, Option<i32>)> {
    let start = s2sdk::GetClientEyePosition(player.slot);
    if !finite_vec(start) {
        return Err(("player eye position is not finite".into(), None));
    }
    let handle = s2sdk::CreateEntityByName(&Str::from_str(BEAM_CLASS));
    if beam_pointer(handle).is_none() {
        return Err(("beam entity creation returned no full entity handle".into(), None));
    }
    let result = (|| {
        if s2sdk::GetEntityClassname(handle).as_str() != BEAM_CLASS {
            return Err("created beam handle resolved to another entity class".into());
        }
        let pointer = beam_pointer(handle).ok_or_else(|| "created beam handle became invalid before setup".to_owned())?;
        let end_class = Str::from_str("CBeam");
        let end_field = Str::from_str("m_vecEndPos");
        let width_class = Str::from_str("CBeam");
        let width_field = Str::from_str("m_fWidth");
        let color_class = Str::from_str("CBaseModelEntity");
        let color_field = Str::from_str("m_clrRender");
        let zero = Vec3::default();
        s2sdk::SetEntSchemaFloat2(pointer, &width_class, &width_field, BEAM_WIDTH, true, 0);
        s2sdk::SetEntSchemaColor(handle, &color_class, &color_field, &BEAM_COLOR, true, 0);
        s2sdk::SetEntSchemaVector3D2(pointer, &end_class, &end_field, &anchor, true, 0);
        s2sdk::TeleportEntity(handle, &start, &zero, &zero);
        if beam_pointer(handle).is_none() || LivePlayer::resolve(player.slot) != Some(player) {
            return Err("player or beam identity changed before beam spawn".into());
        }
        s2sdk::DispatchSpawn(handle);
        let pointer = beam_pointer(handle).ok_or_else(|| "beam disappeared during spawn".to_owned())?;
        if s2sdk::GetEntityClassname(handle).as_str() != BEAM_CLASS {
            return Err("spawned entity no longer resolves to beam".into());
        }
        let origin = s2sdk::GetEntityAbsOrigin(handle);
        let end = s2sdk::GetEntSchemaVector3D2(pointer, &end_class, &end_field, 0);
        let width = s2sdk::GetEntSchemaFloat2(pointer, &width_class, &width_field, 0) as f32;
        let color = s2sdk::GetEntSchemaColor(handle, &color_class, &color_field, 0);
        if !vector_near(origin, start, 0.25)
            || !vector_near(end, anchor, 0.25)
            || (width - BEAM_WIDTH as f32).abs() > 0.01
            || !color_near(color, BEAM_COLOR, 1.0)
        {
            return Err("beam setup readback did not match endpoint, width, or color".into());
        }
        Ok(())
    })();
    if let Err(error) = result {
        if beam_pointer(handle).is_some() {
            s2sdk::RemoveEntity(handle);
        }
        return Err((error, beam_pointer(handle).is_some().then_some(handle)));
    }
    Ok(handle)
}

fn update_beam(handle: i32, start: Vec3, anchor: Vec3) -> Result<(), String> {
    if s2sdk::GetEntityClassname(handle).as_str() != BEAM_CLASS {
        return Err("owned beam handle or classname was lost".into());
    }
    let pointer = beam_pointer(handle).ok_or_else(|| "owned beam handle no longer resolves".to_owned())?;
    let class = Str::from_str("CBeam");
    let end_field = Str::from_str("m_vecEndPos");
    let zero = Vec3::default();
    s2sdk::TeleportEntity(handle, &start, &zero, &zero);
    s2sdk::SetEntSchemaVector3D2(pointer, &class, &end_field, &anchor, true, 0);
    let origin = s2sdk::GetEntityAbsOrigin(handle);
    let endpoint = s2sdk::GetEntSchemaVector3D2(pointer, &class, &end_field, 0);
    if !vector_near(origin, start, 0.25) || !vector_near(endpoint, anchor, 0.25) {
        return Err("beam endpoint update failed readback".into());
    }
    Ok(())
}

fn remove_beam(handle: i32) -> bool {
    if beam_pointer(handle).is_none() {
        return true;
    }
    s2sdk::RemoveEntity(handle);
    beam_pointer(handle).is_none()
}

impl Runtime {
    pub(super) fn refresh_grapple_capability(&mut self) {
        if self.faults.capabilities().grapple || self.faults.recovery == Recovery::Stopped || !self.use_target_hook_ready {
            return;
        }
        self.grapple_use_hook_ready();
    }

    pub(super) fn grapple_configured(&self) -> bool {
        self.controller
            .config()
            .is_some_and(|config| mode_configured(config, |mode| mode.grapple.is_some()))
    }

    pub(super) fn use_target_probe_ready(&self) -> bool {
        (0..s2sdk::GetMaxClients()).any(|slot| {
            LivePlayer::resolve(slot).is_some_and(|player| {
                self.players
                    .identity(slot)
                    .is_some_and(|identity| identity.pawn_handle == player.pawn_handle as u32)
                    && self.controllers.get(&slot) == Some(&player.controller_handle)
            })
        })
    }

    pub(super) fn grapple_use_hook_ready(&mut self) {
        if !self.grapple_configured() {
            return;
        }
        if !self.grapple_bomb_use_ready {
            self.grapple_use_hook_failure("required C4 plant/defuse lifecycle events unavailable");
            return;
        }
        self.capability_reasons.retain(|reason| !reason.starts_with("grapple:"));
        if !capability_probe(&mut self.capability_reasons) || !self.faults.enable_grapple() {
            return;
        }
        self.capability_reasons.retain(|reason| !reason.starts_with("grapple:"));
        self.controller.set_capabilities(self.faults.capabilities());
        log("grapple capability is ready after the live use-target observer was installed");
    }

    pub(super) fn grapple_use_hook_failure(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        let first = self.faults.exclude(Mechanism::Grapple, reason.clone());
        if first {
            self.capability_reasons.push(format!("grapple:[{reason}]"));
        }
        self.controller.set_capabilities(self.faults.capabilities());
        if first {
            log(&format!("grapple capability unavailable: {reason}"));
        }
    }

    pub(super) fn wants_use_target_hook(&self) -> bool {
        self.chicken_use_configured() || self.grapple_configured()
    }

    pub(super) fn grapple_mode_active(&self) -> bool {
        self.controller.enabled()
            && active_combat_phase(self.controller.phase())
            && self.faults.capabilities().grapple
            && self.controller.current_mode().is_some_and(|mode| mode.grapple.is_some())
    }

    pub(super) fn observe_grapple_buttons(&mut self, tick: i32) {
        let mode = self.controller.current_mode().cloned();
        let enabled = self.grapple_mode_active() && self.use_target_hook_ready;
        if !enabled && self.grapple.active.is_empty() {
            self.grapple.input.clear();
            self.grapple.observations.clear();
            return;
        }
        let mut live = HashSet::new();
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let Some(identity) = self
                .players
                .identity(slot)
                .filter(|identity| identity.pawn_handle == player.pawn_handle as u32)
            else {
                continue;
            };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || player.pawn_controller() != Some(player.controller_handle) {
                continue;
            }
            if !enabled && !self.grapple.active.contains_key(&slot) {
                continue;
            }
            live.insert(slot);
            let buttons = s2sdk::GetClientButtons(slot, 0);
            if let Some(effect) = self.effects.get_mut(&slot).filter(|effect| effect.identity == identity) {
                effect.observe_movement_buttons(buttons);
            }
            self.grapple.sample(identity, buttons & IN_USE != 0);
        }
        self.grapple
            .input
            .retain(|slot, input| live.contains(slot) || self.players.identity(*slot) == Some(input.identity));

        let mut batches: Vec<(PlayerIdentity, i32, SelectorBatch)> = Vec::new();
        for observation in self.grapple.take_closed_tick_observations(tick) {
            let Some(player) = LivePlayer::resolve(observation.identity.slot) else {
                continue;
            };
            if player.pawn_handle as u32 != observation.identity.pawn_handle
                || self.players.identity(observation.identity.slot) != Some(observation.identity)
                || self.controllers.get(&player.slot) != Some(&player.controller_handle)
            {
                continue;
            }
            let batch = if let Some(batch) = batches
                .iter_mut()
                .find(|(identity, tick, _)| *identity == observation.identity && *tick == observation.tick)
            {
                batch
            } else {
                batches.push((observation.identity, observation.tick, SelectorBatch::default()));
                batches.last_mut().expect("batch was just inserted")
            };
            batch.2.observe(observation);
        }
        for (identity, _, batch) in batches {
            let decision = batch.decision();
            match decision {
                Some(SelectorDecision::NativeTarget) => {
                    self.grapple.win_native_target(identity);
                    self.cancel_grapple(identity, StopReason::NativeTarget);
                }
                Some(SelectorDecision::Empty) if enabled => {
                    if let Some(settings) = mode.as_ref().and_then(|mode| mode.grapple) {
                        let task = PlayerTask {
                            player: identity,
                            round: self.controller.generation(),
                        };
                        let Some(player) = LivePlayer::resolve(identity.slot) else { continue };
                        let not_suicide = !self.suicides.active(player);
                        if not_suicide && self.grapple.consume_attempt(identity) && !self.grapple.active.contains_key(&identity.slot) {
                            self.engine_actions
                                .push(EngineAction::Grapple(Action::Acquire { task, player, settings }));
                        }
                    }
                }
                Some(SelectorDecision::Empty) | None => {}
            }
        }

        for slot in 0..s2sdk::GetMaxClients() {
            let Some(active) = self.grapple.active.get(&slot) else { continue };
            let identity = active.task.player;
            let Some(player) = LivePlayer::resolve(slot) else {
                self.cancel_grapple(identity, StopReason::LifeEnded);
                continue;
            };
            let invalid = identity != self.players.identity(slot).unwrap_or(identity)
                || player != active.player
                || self.controllers.get(&slot) != Some(&player.controller_handle)
                || self.controller.generation() != active.task.round
                || self.suicides.active(player)
                || s2sdk::GetEntityMoveType(player.pawn_handle) != s2sdk::MoveType::Walk
                || self
                    .effects
                    .get(&slot)
                    .is_none_or(|effect| effect.temporary_movement_owner() != Some(crate::effects::TemporaryMovementOwner::Grapple));
            let buttons = s2sdk::GetClientButtons(slot, 0);
            if invalid || buttons & IN_USE == 0 {
                self.cancel_grapple(
                    identity,
                    if invalid {
                        StopReason::MovementInvalidated
                    } else {
                        StopReason::InputReleased
                    },
                );
                continue;
            }
            if let Some(active) = self.grapple.active.get(&slot)
                && let Some(beam) = active.beam
            {
                self.engine_actions.push(EngineAction::Grapple(Action::Pull {
                    task: active.task,
                    player: active.player,
                    anchor: active.anchor,
                    beam,
                    settings: active.settings,
                }));
            }
        }

        for handle in self.grapple.retired_beams.keys().copied() {
            self.engine_actions.push(EngineAction::Grapple(Action::RemoveBeam { handle }));
        }
    }

    pub(crate) fn observe_grapple_target(&mut self, pawn_handle: i32, target_handle: i32, in_use: bool) {
        if !self.grapple_configured() && self.grapple.active.is_empty() {
            return;
        }
        let Some(identity) = self
            .controllers
            .keys()
            .find_map(|slot| self.players.identity(*slot).filter(|identity| identity.pawn_handle == pawn_handle as u32))
        else {
            return;
        };
        let observation = Observation::new(identity, s2sdk::GetGameTickCount(), target_handle, in_use);
        self.grapple.queue_observation(observation);
    }

    pub(super) fn mark_chicken_target_claimed(&mut self, identity: PlayerIdentity, tick: i32, target: i32) {
        if self.grapple_configured() || !self.grapple.active.is_empty() {
            self.grapple.claim_chicken_target(identity, tick, target);
        }
    }

    pub(super) fn cancel_grapple(&mut self, identity: PlayerIdentity, reason: StopReason) {
        let Some(active) = self
            .grapple
            .active
            .get(&identity.slot)
            .filter(|active| active.task.player.pawn_handle == identity.pawn_handle)
        else {
            return;
        };
        let task = active.task;
        if let Some(beam) = active.beam {
            self.grapple.retire_beam(beam);
        }
        self.grapple.active.remove(&identity.slot);
        let wait = match reason {
            StopReason::InputReleased => IN_USE,
            StopReason::NativeTarget
            | StopReason::Defusing
            | StopReason::Planting
            | StopReason::MovementInvalidated
            | StopReason::Impulse
            | StopReason::LifeEnded
            | StopReason::RoundEnded => IN_USE | IN_JUMP,
        };
        if let Some(effect) = self
            .effects
            .get_mut(&identity.slot)
            .filter(|effect| effect.identity == task.player && effect.round == task.round)
            && let Err(error) = effect.stop_temporary_movement(wait)
        {
            self.handle_fault(error.context(format!("grapple release slot={}", identity.slot)));
        }
        if self.diagnostics() {
            log(&format!(
                "grapple=stopped slot={} pawn={} reason={reason:?}",
                identity.slot, identity.pawn_handle
            ));
        }
    }

    pub(super) fn cancel_grapple_player(&mut self, slot: i32) {
        if let Some(active) = self.grapple.active.get(&slot) {
            self.cancel_grapple(active.task.player, StopReason::LifeEnded);
        }
        self.grapple.input.remove(&slot);
        self.grapple.observations.retain(|observation| observation.identity.slot != slot);
        self.grapple.bomb_use.remove(&slot);
    }

    pub(super) fn cancel_all_grapples(&mut self, reason: StopReason) {
        let identities = self.grapple.active.values().map(|active| active.task.player).collect::<Vec<_>>();
        for identity in identities {
            self.cancel_grapple(identity, reason);
        }
        self.grapple.input.clear();
        self.grapple.observations.clear();
        self.grapple.bomb_use.clear();
    }

    pub(crate) fn take_grapple_cleanup(&mut self) -> Vec<Action> {
        self.grapple.take_beam_cleanup_actions()
    }

    pub(super) fn grapple_beam_cleanup_complete(&mut self, handle: i32, complete: bool) {
        self.grapple.beam_cleanup_complete(handle, complete);
    }

    pub(super) fn retain_grapple_beam(&mut self, handle: i32) {
        self.grapple.retire_beam(handle);
    }

    pub(super) fn grapple_action_valid(&self, action: &Action) -> bool {
        match action {
            Action::Acquire { task, player, .. } => {
                self.grapple_run_valid(*task, *player)
                    && self.grapple.input_allows_action(task.player)
                    && self
                        .grapple
                        .input
                        .get(&task.player.slot)
                        .is_some_and(|input| input.identity == task.player && input.attempt_consumed && !input.native_target_won)
                    && !self.grapple.active.contains_key(&player.slot)
            }
            Action::Pull { task, player, beam, .. } => self.grapple.active.get(&player.slot).is_some_and(|active| {
                active.task == *task
                    && active.player == *player
                    && active.beam == Some(*beam)
                    && self.grapple.input_allows_action(task.player)
                    && self.grapple_run_valid(*task, *player)
            }),
            Action::RemoveBeam { handle } => self.grapple.retired_beams.contains_key(handle),
        }
    }

    fn grapple_run_valid(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.grapple_mode_active()
            && self.use_target_hook_ready
            && self.controller.generation() == task.round
            && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
            && LivePlayer::resolve(player.slot) == Some(player)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && !self.grapple.native_bomb_use(task.player)
            && self
                .effects
                .get(&player.slot)
                .is_some_and(|effect| effect.identity == task.player && effect.round == task.round && !effect.movement_waits_for_release(IN_USE))
            && !self.suicides.active(player)
            && s2sdk::GetClientButtons(player.slot, 0) & IN_USE != 0
    }

    pub(super) fn start_grapple(
        &mut self,
        task: PlayerTask,
        player: LivePlayer,
        anchor: Vec3,
        settings: funrounds_core::GrappleSettings,
    ) -> Result<(), Fault> {
        if !self.grapple_run_valid(task, player) || self.grapple.active.contains_key(&player.slot) {
            return Err(Fault::cancelled("grapple attempt changed before anchor acquisition"));
        }
        if s2sdk::GetEntityMoveType(player.pawn_handle) != s2sdk::MoveType::Walk {
            return Err(Fault::cancelled("grapple pawn left WALK movement before anchor acquisition"));
        }
        let Some(effect) = self
            .effects
            .get_mut(&player.slot)
            .filter(|effect| effect.identity == task.player && effect.round == task.round)
        else {
            return Err(Fault::cancelled("grapple player effect is not current"));
        };
        effect.acquire_temporary_movement(crate::effects::TemporaryMovementOwner::Grapple, GRAPPLE_GRAVITY)?;
        self.grapple.active.insert(
            player.slot,
            Active {
                task,
                player,
                anchor,
                settings,
                beam: None,
            },
        );
        Ok(())
    }

    pub(super) fn attach_grapple_beam(&mut self, task: PlayerTask, player: LivePlayer, handle: i32) -> bool {
        if self
            .grapple
            .active
            .get(&player.slot)
            .is_some_and(|active| active.task == task && active.player == player)
        {
            self.grapple.active.get_mut(&player.slot).expect("active grapple exists").beam = Some(handle);
            true
        } else {
            self.grapple.retire_beam(handle);
            false
        }
    }

    pub(super) fn fail_grapple(&mut self, task: PlayerTask, player: LivePlayer, reason: String) {
        let current = self
            .grapple
            .active
            .get(&player.slot)
            .is_some_and(|active| active.task == task && active.player == player);
        if !current {
            return;
        }
        self.cancel_grapple(task.player, StopReason::MovementInvalidated);
        self.handle_fault(Fault::mechanism(Mechanism::Grapple, reason).context(format!("slot={} pawn={}", player.slot, player.pawn_handle)));
    }

    pub(super) fn grapple_pending_beam_current(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.grapple
            .active
            .get(&player.slot)
            .is_some_and(|active| active.task == task && active.player == player && active.beam.is_none() && self.grapple_run_valid(task, player))
    }

    pub(super) fn stop_grapple_for_bomb_use(&mut self, slot: i32, defusing: bool) {
        if self.grapple_mode_active()
            && let Some(identity) = self.players.identity(slot)
        {
            let down = s2sdk::GetClientButtons(slot, 0) & IN_USE != 0;
            self.grapple.sample(identity, down);
            self.grapple.begin_bomb_use(identity);
        }
        if let Some(active) = self.grapple.active.get(&slot) {
            self.cancel_grapple(active.task.player, if defusing { StopReason::Defusing } else { StopReason::Planting });
        }
    }

    pub(super) fn finish_grapple_bomb_use(&mut self, slot: i32) {
        if let Some(identity) = self.players.identity(slot) {
            self.grapple.end_bomb_use(identity);
        }
    }

    pub(super) fn discard_grapple_map_state(&mut self) {
        self.grapple = State::default();
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum StopReason {
    InputReleased,
    NativeTarget,
    Defusing,
    Planting,
    MovementInvalidated,
    Impulse,
    LifeEnded,
    RoundEnded,
}

pub(crate) fn execute(action: Action, still_current: impl Fn() -> bool) -> Result<(), Fault> {
    match action {
        Action::Acquire { task, player, settings } => {
            if !still_current() {
                return Ok(());
            }
            let Some(anchor) = trace_world_anchor(player, settings) else {
                return Ok(());
            };
            if !still_current() {
                return Ok(());
            }
            {
                let mut guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let Some(runtime) = guard.as_mut().filter(|runtime| runtime.on_game_thread()) else {
                    return Ok(());
                };
                if let Err(error) = runtime.start_grapple(task, player, anchor, settings) {
                    return if error.scope == Scope::Cancelled { Ok(()) } else { Err(error) };
                }
            }
            let pending_current = {
                let guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.as_ref().is_some_and(|runtime| runtime.grapple_pending_beam_current(task, player))
            };
            if !pending_current {
                let mut guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(runtime) = guard.as_mut() {
                    runtime.fail_grapple(task, player, "grapple input or owner changed after anchor lock".into());
                }
                return Ok(());
            }
            match create_beam(player, anchor) {
                Ok(handle) => {
                    let mut guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(runtime) = guard.as_mut() {
                        if runtime.attach_grapple_beam(task, player, handle) && runtime.diagnostics() {
                            log(&format!(
                                "grapple=latched slot={} pawn={} beam={handle} anchor={anchor:?}",
                                player.slot, player.pawn_handle
                            ));
                        }
                    } else {
                        let _ = remove_beam(handle);
                    }
                }
                Err((error, leaked_handle)) => {
                    let mut guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(runtime) = guard.as_mut() {
                        if let Some(handle) = leaked_handle {
                            runtime.retain_grapple_beam(handle);
                        }
                        runtime.fail_grapple(task, player, format!("visible beam setup failed: {error}"));
                    }
                }
            }
            Ok(())
        }
        Action::Pull {
            task,
            player,
            anchor,
            beam,
            settings,
        } => {
            if !still_current() {
                return Ok(());
            }
            let gravity = s2sdk::GetClientGravity(player.slot);
            if !gravity.is_finite() || (gravity - GRAPPLE_GRAVITY).abs() > 0.0001 {
                let mut guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(runtime) = guard.as_mut() {
                    runtime.cancel_grapple(task.player, StopReason::MovementInvalidated);
                }
                return Ok(());
            }
            let start = s2sdk::GetClientEyePosition(player.slot);
            if !finite_vec(start) {
                let mut guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(runtime) = guard.as_mut() {
                    runtime.fail_grapple(task, player, "grapple eye position became non-finite".into());
                }
                return Ok(());
            }
            if let Err(error) = update_beam(beam, start, anchor) {
                let mut guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(runtime) = guard.as_mut() {
                    runtime.fail_grapple(task, player, format!("visible beam update failed: {error}"));
                }
                return Ok(());
            }
            if !still_current() {
                return Ok(());
            }
            let origin = s2sdk::GetEntityAbsOrigin(player.pawn_handle);
            let delta = Vec3 {
                x: anchor.x - origin.x,
                y: anchor.y - origin.y,
                z: anchor.z - origin.z,
            };
            let distance_squared = delta.x * delta.x + delta.y * delta.y + delta.z * delta.z;
            if !distance_squared.is_finite() || distance_squared < 0.0 {
                let mut guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(runtime) = guard.as_mut() {
                    runtime.fail_grapple(task, player, "grapple pull distance became non-finite".into());
                }
                return Ok(());
            }
            let distance = distance_squared.sqrt();
            let speed = settings.max_speed.min(distance * settings.pull_gain);
            let velocity = if distance > 0.0 {
                Vec3 {
                    x: delta.x * speed / distance,
                    y: delta.y * speed / distance,
                    z: delta.z * speed / distance,
                }
            } else {
                Vec3::default()
            };
            if !finite_vec(velocity) {
                return Err(Fault::mechanism(Mechanism::Grapple, "grapple pull velocity became non-finite"));
            }
            if still_current() {
                s2sdk::SetEntityAbsVelocity(player.pawn_handle, &velocity);
            }
            Ok(())
        }
        Action::RemoveBeam { handle } => {
            let complete = remove_beam(handle);
            let mut guard = crate::RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(runtime) = guard.as_mut() {
                runtime.grapple_beam_cleanup_complete(handle, complete);
            }
            Ok(())
        }
    }
}
