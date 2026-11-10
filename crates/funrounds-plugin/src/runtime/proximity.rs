// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use funrounds_core::GrenadeKind;
use plugify::Vec3;
use std::cmp::Ordering;

const PROJECTILE_CLASS: &str = "CBaseCSGrenadeProjectile";
const TRACE_MASK: i32 = 798785;
const GUIDANCE_FUSE_HOLD_GRACE_SECONDS: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Source {
    task: PlayerTask,
    player: LivePlayer,
    kind: GrenadeKind,
    tick: i32,
    team: i32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct GuidanceSpawn {
    handle: i32,
    tick: i32,
    epoch: u64,
    game_time: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuidanceThrow {
    source: Source,
    epoch: u64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpawnObservation {
    pub(crate) handle: i32,
    pub(crate) tick: i32,
    pub(crate) epoch: u64,
    pub(crate) game_time: f32,
}
impl From<SpawnObservation> for GuidanceSpawn {
    fn from(observation: SpawnObservation) -> Self {
        Self {
            handle: observation.handle,
            tick: observation.tick,
            epoch: observation.epoch,
            game_time: observation.game_time,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct GuidanceFusePause {
    native_deadline: f64,
    held_deadline: f64,
    remaining_seconds: f64,
    confirmed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Guidance {
    target: LivePlayer,
    target_task: PlayerTask,
    spawned_at: f32,
    last_game_time: f32,
    force_retarget: bool,
}
#[derive(Clone, Copy)]
struct WaitingTarget {
    player: LivePlayer,
    task: PlayerTask,
    started_at: f32,
}
#[derive(Clone)]
struct Grenade {
    source: Source,
    waiting: Option<WaitingTarget>,
    claimed: bool,
    guidance: Option<Guidance>,
    guidance_fuse: Option<GuidanceFusePause>,
}

fn merge_proximity_grenade(grenades: &mut HashMap<i32, Grenade>, handle: i32, source: Source) -> bool {
    match grenades.entry(handle) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(Grenade {
                source,
                waiting: None,
                claimed: false,
                guidance: None,
                guidance_fuse: None,
            });
            true
        }
        std::collections::hash_map::Entry::Occupied(entry) => {
            let existing = entry.get();
            if existing.source.task != source.task || existing.source.player != source.player || existing.source.kind != source.kind {
                return false;
            }
            true
        }
    }
}

fn merge_guidance_association(
    grenades: &mut HashMap<i32, Grenade>,
    handle: i32,
    source: Source,
    selected: Option<(LivePlayer, PlayerTask)>,
    spawned_at: f32,
    game_time: f32,
) -> bool {
    let entry = grenades.entry(handle).or_insert_with(|| Grenade {
        source,
        waiting: None,
        claimed: false,
        guidance: None,
        guidance_fuse: None,
    });
    if entry.source.task != source.task || entry.source.player != source.player || entry.source.kind != GrenadeKind::He {
        return false;
    }
    if !entry.claimed {
        entry.guidance = selected.map(|(target, target_task)| Guidance {
            target,
            target_task,
            spawned_at,
            last_game_time: game_time,
            force_retarget: false,
        });
    }
    true
}

fn relinquish_stale_proximity_ownership(grenade: &mut Grenade) -> Option<GuidanceFusePause> {
    let fuse_pause = grenade.guidance_fuse?;
    grenade.guidance = None;
    Some(fuse_pause)
}

fn guidance_fuse_pause(native_deadline: f64, now: f64, remaining_guidance_seconds: f64) -> Option<GuidanceFusePause> {
    if !native_deadline.is_finite()
        || !now.is_finite()
        || !remaining_guidance_seconds.is_finite()
        || native_deadline <= now
        || remaining_guidance_seconds <= 0.0
    {
        return None;
    }
    let remaining_seconds = native_deadline - now;
    let held_deadline = f64::from((now + remaining_seconds + remaining_guidance_seconds + GUIDANCE_FUSE_HOLD_GRACE_SECONDS) as f32);
    (remaining_seconds.is_finite() && remaining_seconds > 0.0 && held_deadline.is_finite() && held_deadline > native_deadline).then_some(
        GuidanceFusePause {
            native_deadline,
            held_deadline,
            remaining_seconds,
            confirmed: false,
        },
    )
}

fn same_fuse_deadline(actual: f64, expected: f64) -> bool {
    actual.is_finite() && expected.is_finite() && actual == expected
}

fn guidance_deadline_owned(fuse_pause: GuidanceFusePause, deadline: f64, now: f64, pause_fuse: bool) -> bool {
    deadline.is_finite()
        && deadline > now
        && if fuse_pause.confirmed {
            !pause_fuse && same_fuse_deadline(deadline, fuse_pause.held_deadline)
        } else {
            pause_fuse && same_fuse_deadline(deadline, fuse_pause.native_deadline)
        }
}

fn remaining_guidance_budget(spawned_at: f32, now: f32, max_guidance_seconds: f32) -> Option<f64> {
    if !spawned_at.is_finite() || !now.is_finite() || !max_guidance_seconds.is_finite() || max_guidance_seconds <= 0.0 {
        return None;
    }
    let elapsed = f64::from(now) - f64::from(spawned_at);
    let remaining = f64::from(max_guidance_seconds) - elapsed;
    (elapsed >= 0.0 && remaining.is_finite() && remaining > 0.0).then_some(remaining)
}

fn retarget_guidance(guidance: &mut Guidance, target: LivePlayer, target_task: PlayerTask) {
    guidance.target = target;
    guidance.target_task = target_task;
}

fn request_guidance_retarget(grenades: &mut HashMap<i32, Grenade>, action: GuidanceAction) -> bool {
    if let Some(grenade) = grenades.get_mut(&action.handle)
        && grenade.source == action.source
        && grenade.guidance.is_some_and(|guidance| {
            guidance.target == action.target && guidance.target_task == action.target_task && guidance.last_game_time == action.game_time
        })
    {
        grenade.guidance.as_mut().expect("matched guidance").force_retarget = true;
        return true;
    }
    false
}
#[derive(Default)]
pub(super) struct State {
    round: Option<RoundGeneration>,
    known: HashSet<i32>,
    recent: HashMap<i32, (PlayerTask, i32)>,
    pending: Vec<Source>,
    guidance_spawns: Vec<GuidanceSpawn>,
    guidance_throws: Vec<GuidanceThrow>,
    grenades: HashMap<i32, Grenade>,
    impulse_hits: HashMap<i32, Vec<PlayerIdentity>>,
}

#[derive(Clone, Copy)]
pub(crate) struct Action {
    source: Source,
    handle: i32,
    target: LivePlayer,
    target_task: PlayerTask,
    radius: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct GuidanceAction {
    source: Source,
    handle: i32,
    target: LivePlayer,
    target_task: PlayerTask,
    settings: funrounds_core::HeGuidanceSettings,
    delta_seconds: f32,
    game_time: f32,
    fuse_pause: GuidanceFusePause,
    pause_fuse: bool,
}
impl GuidanceAction {
    pub(super) fn involves(self, slots: &[i32]) -> bool {
        slots.contains(&self.source.player.slot) || slots.contains(&self.target.slot)
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuidanceFuseRestoreAction {
    source: Source,
    handle: i32,
    fuse_pause: GuidanceFusePause,
}
impl Action {
    pub(super) fn involves(self, slots: &[i32]) -> bool {
        slots.contains(&self.source.player.slot) || slots.contains(&self.target.slot)
    }
}

pub(super) fn probe(events: &EventCapabilities, caps: &mut funrounds_core::Capabilities, reasons: &mut Vec<String>) {
    let mut common = crate::capability::Probe::default();
    common.require(events.grenade_thrown && events.combat_events, "events:grenade_thrown/lifecycle");
    crate::capability::imports!(common; __s2sdk_GetFirstActiveEntity, __s2sdk_GetNextActiveEntity, __s2sdk_IsValidEntHandle,
        __s2sdk_GetEntityClassname, __s2sdk_EntHandleToEntPointer, __s2sdk_GetEntSchemaEnt2, __s2sdk_GetEntityAbsOrigin,
        __s2sdk_GetClientAbsOrigin, __s2sdk_GetClientEyePosition, __s2sdk_TraceLine, __s2sdk_GetClientTeam,
        __s2sdk_FindConVar, __s2sdk_GetConVarValue, __s2sdk_GetGameTickCount, __s2sdk_GetGameTime);
    common.require(
        super::shooting::teammates_are_friendly().is_ok(),
        "convar:mp_teammates_are_enemies (readable boolean)",
    );
    common.schema("CBaseGrenade", "m_hThrower");
    let mut deadline = common.clone();
    crate::capability::imports!(deadline; __s2sdk_GetEntSchemaFloat2, __s2sdk_SetEntSchemaFloat2);
    deadline.schema(PROJECTILE_CLASS, "m_flDetonateTime");
    let mut impulse = crate::capability::Probe::default();
    impulse.require(events.combat_events && events.damage, "hooks:damage/lifecycle");
    crate::capability::imports!(impulse; __s2sdk_IsValidEntHandle, __s2sdk_GetEntityClassname,
        __s2sdk_GetEntityAbsOrigin, __s2sdk_GetEntityAbsVelocity, __s2sdk_SetEntityAbsVelocity);
    caps.hegrenade_impulse = impulse.record("hegrenade_impulse", reasons);
    caps.proximity_he = deadline.clone().record("proximity_he", reasons);
    let mut flash = deadline;
    crate::capability::imports!(flash; __s2sdk_GetClientEyeAngles);
    caps.proximity_flash = flash.record("proximity_flash", reasons);

    let mut guidance = crate::capability::Probe::default();
    guidance.require(events.grenade_thrown && events.combat_events, "events:grenade_thrown/lifecycle");
    guidance.require(events.spawn_listener_available, "event_listener:OnEntitySpawned");
    crate::capability::imports!(guidance;
        __s2sdk_IsClientInGame, __s2sdk_IsClientAlive, __s2sdk_GetClientHealth, __s2sdk_GetMaxClients,
        __s2sdk_IsValidEntHandle, __s2sdk_GetEntityClassname, __s2sdk_EntHandleToEntPointer,
        __s2sdk_GetEntSchemaEnt2, __s2sdk_GetEntityAbsOrigin, __s2sdk_GetClientAbsOrigin,
        __s2sdk_GetClientEyePosition, __s2sdk_TraceLine, __s2sdk_GetClientTeam, __s2sdk_GetGameTickCount,
        __s2sdk_GetGameTime, __s2sdk_FindConVar, __s2sdk_GetConVarValue, __s2sdk_PlayerSlotToEntHandle,
        __s2sdk_GetClientPawn, __s2sdk_EntPointerToEntHandle,
        __s2sdk_GetEntSchema2, __s2sdk_GetEntSchemaFloat2, __s2sdk_GetEntityAbsVelocity, __s2sdk_SetEntityAbsVelocity,
    );
    guidance.require(
        super::shooting::teammates_are_friendly().is_ok(),
        "convar:mp_teammates_are_enemies (readable boolean)",
    );
    guidance.schema("CBaseGrenade", "m_hThrower");
    guidance.schema("CBasePlayerPawn", "m_hController");
    guidance.schema(PROJECTILE_CLASS, "m_flDetonateTime");
    caps.he_guidance = guidance.record("he_guidance", reasons);
    for (kind, capability, name) in [
        (1, &mut caps.proximity_smoke, "proximity_smoke"),
        (2, &mut caps.proximity_molotov, "proximity_molotov"),
        (3, &mut caps.proximity_incendiary, "proximity_incendiary"),
    ] {
        let mut probe = common.clone();
        probe.require(
            unsafe { crate::funrounds_native::__funrounds_native_HasGrenadeDetonateSupport }.is_some_and(|query| unsafe { query(kind) }),
            "native:typed_grenade_detonation",
        );
        if kind == 1 {
            probe.require(
                unsafe { crate::funrounds_native::__funrounds_native_DetonateGrenade }.is_some(),
                "native:DetonateGrenade",
            );
        } else {
            probe.require(
                unsafe { crate::funrounds_native::__funrounds_native_GetGrenadeDetonationState }.is_some(),
                "native:GetGrenadeDetonationState",
            );
            crate::capability::imports!(probe; __s2sdk_GetEntityAbsVelocity, __s2sdk_SetEntityAbsVelocity);
        }
        *capability = probe.record(name, reasons);
    }
}

fn classname(kind: GrenadeKind) -> &'static str {
    match kind {
        GrenadeKind::He => "hegrenade_projectile",
        GrenadeKind::Flash => "flashbang_projectile",
        GrenadeKind::Smoke => "smokegrenade_projectile",
        GrenadeKind::Molotov | GrenadeKind::Incendiary => "molotov_projectile",
        GrenadeKind::Decoy => "decoy_projectile",
    }
}

fn native_kind(kind: GrenadeKind) -> Option<i32> {
    match kind {
        GrenadeKind::Smoke => Some(1),
        GrenadeKind::Molotov => Some(2),
        GrenadeKind::Incendiary => Some(3),
        _ => None,
    }
}

fn thrower(handle: i32) -> i32 {
    s2sdk::GetEntSchemaEnt2(
        s2sdk::EntHandleToEntPointer(handle),
        &Str::from_str("CBaseGrenade"),
        &Str::from_str("m_hThrower"),
        0,
    )
}

fn all_projectiles() -> Vec<i32> {
    [
        "hegrenade_projectile",
        "flashbang_projectile",
        "smokegrenade_projectile",
        "molotov_projectile",
    ]
    .into_iter()
    .flat_map(game::entity_handles)
    .collect()
}

fn guidance_tick_pair(throw_tick: i32, spawn_tick: i32) -> bool {
    throw_tick.abs_diff(spawn_tick) <= 1
}

#[derive(Default)]
struct GuidanceResolution {
    pairs: Vec<(usize, usize)>,
    discard_throws: HashSet<usize>,
    discard_spawns: HashSet<usize>,
    ambiguous_components: Vec<(usize, usize)>,
}

fn resolve_guidance_candidates(
    candidates: &[(usize, usize)],
    throws: &[GuidanceThrow],
    spawns: &[GuidanceSpawn],
    current_tick: i32,
) -> GuidanceResolution {
    let mut throw_edges = HashMap::<usize, Vec<usize>>::new();
    let mut spawn_edges = HashMap::<usize, Vec<usize>>::new();
    for &(throw_index, spawn_index) in candidates {
        throw_edges.entry(throw_index).or_default().push(spawn_index);
        spawn_edges.entry(spawn_index).or_default().push(throw_index);
    }

    let mut resolution = GuidanceResolution::default();
    let mut visited_throws = HashSet::new();
    let mut visited_spawns = HashSet::new();
    for &first_throw in throw_edges.keys() {
        if !visited_throws.insert(first_throw) {
            continue;
        }
        let mut component_throws = vec![first_throw];
        let mut component_spawns = Vec::new();
        let mut pending_throws = vec![first_throw];
        while let Some(throw_index) = pending_throws.pop() {
            for &spawn_index in throw_edges.get(&throw_index).into_iter().flatten() {
                if visited_spawns.insert(spawn_index) {
                    component_spawns.push(spawn_index);
                    for &neighbor_throw in spawn_edges.get(&spawn_index).into_iter().flatten() {
                        if visited_throws.insert(neighbor_throw) {
                            component_throws.push(neighbor_throw);
                            pending_throws.push(neighbor_throw);
                        }
                    }
                }
            }
        }

        let closed = component_throws
            .iter()
            .all(|index| current_tick > throws[*index].source.tick.saturating_add(1))
            && component_spawns.iter().all(|index| current_tick > spawns[*index].tick.saturating_add(1));
        if !closed {
            continue;
        }
        if let ([throw_index], [spawn_index]) = (component_throws.as_slice(), component_spawns.as_slice()) {
            resolution.pairs.push((*throw_index, *spawn_index));
        } else {
            resolution.discard_throws.extend(component_throws.iter().copied());
            resolution.discard_spawns.extend(component_spawns.iter().copied());
            resolution.ambiguous_components.push((component_throws.len(), component_spawns.len()));
        }
    }

    for (index, throw) in throws.iter().enumerate() {
        if !throw_edges.contains_key(&index) && current_tick > throw.source.tick.saturating_add(1) {
            resolution.discard_throws.insert(index);
        }
    }
    for (index, spawn) in spawns.iter().enumerate() {
        if !spawn_edges.contains_key(&index) && current_tick > spawn.tick.saturating_add(1) {
            resolution.discard_spawns.insert(index);
        }
    }
    resolution
}

fn guidance_candidate_pairs(
    throws: &[GuidanceThrow],
    spawns: &[GuidanceSpawn],
    owners: &HashMap<usize, i32>,
    round: RoundGeneration,
    epoch: u64,
) -> Vec<(usize, usize)> {
    let mut candidates = Vec::new();
    for (throw_index, source) in throws.iter().enumerate() {
        for (spawn_index, spawn) in spawns.iter().enumerate() {
            if source.source.kind == GrenadeKind::He
                && source.epoch == epoch
                && source.source.task.round == round
                && spawn.epoch == epoch
                && guidance_tick_pair(source.source.tick, spawn.tick)
                && owners.get(&spawn_index) == Some(&source.source.player.pawn_handle)
            {
                candidates.push((throw_index, spawn_index));
            }
        }
    }
    candidates
}

fn guidance_target_order(left: (f64, i32, i32), right: (f64, i32, i32)) -> Ordering {
    left.0
        .total_cmp(&right.0)
        .then_with(|| left.1.cmp(&right.1))
        .then_with(|| left.2.cmp(&right.2))
}

fn grenade_detonate_deadline(handle: i32) -> f64 {
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(PROJECTILE_CLASS), &Str::from_str("m_flDetonateTime"), 0)
}

fn set_grenade_detonate_deadline(handle: i32, deadline: f64) {
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    s2sdk::SetEntSchemaFloat2(
        pointer,
        &Str::from_str(PROJECTILE_CLASS),
        &Str::from_str("m_flDetonateTime"),
        deadline,
        true,
        0,
    );
}

fn he_damage_is_detonation(damage_type: u32, skip_plugin_rules: bool) -> bool {
    !skip_plugin_rules && damage_type & crate::damage::DMG_BLAST != 0
}

impl Runtime {
    fn proximity_active(&self) -> bool {
        self.classic_round()
            && active_combat_phase(self.controller.phase())
            && self.controller.current_mode().is_some_and(|m| m.proximity_fuse.is_some())
    }

    fn proximity_source_current(&self, source: Source) -> bool {
        self.proximity_active()
            && s2sdk::GetClientTeam(source.player.slot) as i32 == source.team
            && self.controllers.get(&source.player.slot) == Some(&source.player.controller_handle)
            && LivePlayer::resolve_present(source.player.slot) == Some(source.player)
            && self
                .players
                .validates_damage_source(source.task, self.controller.generation(), source.player.pawn_handle as u32)
            && source.player.pawn_controller().is_some()
    }

    fn guidance_source_current(&self, source: Source) -> bool {
        source.kind == GrenadeKind::He
            && self.he_guidance_active()
            && s2sdk::GetClientTeam(source.player.slot) as i32 == source.team
            && self.controllers.get(&source.player.slot) == Some(&source.player.controller_handle)
            && LivePlayer::resolve(source.player.slot) == Some(source.player)
            && self.combat_task(source.player) == Some(source.task)
            && self
                .players
                .validates(source.task, self.controller.generation(), source.player.pawn_handle as u32)
            && source.player.pawn_controller().is_some()
    }

    pub(super) fn observe_proximity_throw(&mut self, slot: i32, kind: Option<GrenadeKind>) {
        self.sync_he_guidance_spawn_epoch();
        let guidance_enabled = self.he_guidance_active();
        let proximity_enabled = self.proximity_active();
        let old_path_kind = kind.filter(|kind| {
            proximity_enabled
                && self
                    .controller
                    .current_mode()
                    .is_some_and(|mode| mode.proximity_fuse.as_ref().is_some_and(|settings| settings.grenades.contains(kind)))
        });
        let guidance_he = guidance_enabled && kind == Some(GrenadeKind::He);
        let Some(kind) = old_path_kind.or_else(|| guidance_he.then_some(GrenadeKind::He)) else {
            return;
        };
        let Some(player) = LivePlayer::resolve(slot) else { return };
        let Some(task) = self.combat_task(player) else { return };
        let tick = s2sdk::GetGameTickCount();
        if tick < 0 {
            return;
        }
        let source = Source {
            task,
            player,
            kind,
            tick,
            team: s2sdk::GetClientTeam(slot) as i32,
        };
        if old_path_kind.is_some() && !self.match_proximity_throw(source) {
            self.proximity.pending.push(source);
        }
        if guidance_he {
            self.proximity.guidance_throws.push(GuidanceThrow {
                source,
                epoch: crate::HE_GUIDANCE_SPAWN_EPOCH.load(std::sync::atomic::Ordering::Acquire),
            });
        }
    }

    fn match_proximity_throw(&mut self, source: Source) -> bool {
        let candidates: Vec<_> = game::entity_handles(classname(source.kind))
            .into_iter()
            .filter(|handle| {
                (!self.proximity.known.contains(handle)
                    || self
                        .proximity
                        .recent
                        .get(handle)
                        .is_some_and(|(task, tick)| *task == source.task && source.tick >= *tick && source.tick <= tick.saturating_add(1)))
                    && !self.projectiles.manages_projectile(*handle)
                    && thrower(*handle) == source.player.pawn_handle
            })
            .collect();
        if let [handle] = candidates.as_slice() {
            self.proximity.known.insert(*handle);
            self.proximity.recent.remove(handle);
            if !merge_proximity_grenade(&mut self.proximity.grenades, *handle, source) {
                log(&format!(
                    "proximity association conflicts with existing projectile owner: handle={handle} slot={}",
                    source.player.slot
                ));
            }
            true
        } else {
            // Multiple unclaimed projectiles cannot be attributed to one real
            // throw. Never choose an arbitrary entity or infer its grenade kind.
            if candidates.len() > 1 {
                self.proximity.known.extend(candidates);
                self.handle_fault(Fault::mechanism(
                    Mechanism::ProximityFuse(source.kind),
                    "ambiguous projectile source; native grenades left unchanged",
                ));
            }
            false
        }
    }

    fn queue_guidance_fuse_restore(&mut self, handle: i32, source: Source, fuse_pause: GuidanceFusePause) {
        let action = GuidanceFuseRestoreAction { source, handle, fuse_pause };
        if !self
            .engine_actions
            .iter()
            .any(|queued| matches!(queued, EngineAction::HeGuidanceFuseRestore(existing) if *existing == action))
        {
            self.engine_actions.push(EngineAction::HeGuidanceFuseRestore(action));
        }
    }

    fn stop_grenade_guidance(&mut self, handle: i32) {
        let Some((source, fuse_pause)) = self.proximity.grenades.get_mut(&handle).map(|grenade| {
            grenade.guidance = None;
            (grenade.source, grenade.guidance_fuse)
        }) else {
            return;
        };
        if let Some(fuse_pause) = fuse_pause {
            self.queue_guidance_fuse_restore(handle, source, fuse_pause);
        }
    }

    fn stop_all_guidance(&mut self) {
        self.proximity.guidance_throws.clear();
        self.proximity.guidance_spawns.clear();
        let handles = self.proximity.grenades.keys().copied().collect::<Vec<_>>();
        for handle in handles {
            self.stop_grenade_guidance(handle);
        }
    }

    fn reset_proximity_preserving_fuse_recovery(&mut self) {
        self.stop_all_guidance();
        self.proximity.pending.clear();
        self.proximity.known.clear();
        self.proximity.recent.clear();
        self.proximity.guidance_throws.clear();
        self.proximity.guidance_spawns.clear();
        self.proximity.round = None;
        self.proximity.grenades.retain(|_, grenade| grenade.guidance_fuse.is_some());
    }

    pub(super) fn stop_he_guidance_for_cleanup(&mut self) {
        self.proximity.impulse_hits.clear();
        self.reset_proximity_preserving_fuse_recovery();
    }

    pub(super) fn has_guidance_fuse_recovery(&self) -> bool {
        self.proximity.grenades.values().any(|grenade| grenade.guidance_fuse.is_some())
    }

    pub(super) fn update_proximity_with_spawned_entities(&mut self, tick: i32, spawned_entities: &[SpawnObservation]) {
        self.sync_he_guidance_spawn_epoch();
        self.proximity.impulse_hits.retain(|handle, _| s2sdk::IsValidEntHandle(*handle));
        let proximity_active = self.proximity_active();
        let guidance_active = self.he_guidance_active();
        let guidance_epoch = crate::HE_GUIDANCE_SPAWN_EPOCH.load(std::sync::atomic::Ordering::Acquire);
        if !proximity_active && !guidance_active {
            self.reset_proximity_preserving_fuse_recovery();
            return;
        }
        let round = self.controller.generation();
        let new_round = self.proximity.round != Some(round);
        if new_round {
            let stale_handles = self
                .proximity
                .grenades
                .iter()
                .filter_map(|(handle, grenade)| (grenade.source.task.round != round).then_some(*handle))
                .collect::<Vec<_>>();
            for handle in stale_handles {
                self.stop_grenade_guidance(handle);
            }
            self.proximity.round = Some(round);
            self.proximity
                .grenades
                .retain(|_, grenade| grenade.source.task.round == round || grenade.guidance_fuse.is_some());
            self.proximity.pending.retain(|source| source.task.round == round);
            self.proximity
                .guidance_throws
                .retain(|throw| throw.source.task.round == round && throw.epoch == guidance_epoch);
            self.proximity.guidance_spawns.clear();
            self.proximity.known.clear();
            self.proximity.recent.clear();
        }
        if guidance_active {
            for spawn in spawned_entities.iter().filter(|spawn| spawn.epoch == guidance_epoch && spawn.tick >= 0) {
                if !self
                    .proximity
                    .guidance_spawns
                    .iter()
                    .any(|existing| existing.handle == spawn.handle && existing.epoch == spawn.epoch)
                {
                    self.proximity.guidance_spawns.push((*spawn).into());
                }
            }
            self.proximity.guidance_spawns.retain(|spawn| {
                spawn.epoch == guidance_epoch
                    && s2sdk::IsValidEntHandle(spawn.handle)
                    && !self.projectiles.manages_projectile(spawn.handle)
                    && s2sdk::GetEntityClassname(spawn.handle).as_str() == "hegrenade_projectile"
            });
            let throws = std::mem::take(&mut self.proximity.guidance_throws);
            self.proximity.guidance_throws = throws
                .into_iter()
                .filter(|throw| throw.epoch == guidance_epoch && self.guidance_source_current(throw.source))
                .collect();
            if let Err(error) = self.associate_he_guidance_throws(tick) {
                self.handle_fault(error);
                return;
            }
        } else {
            self.proximity.guidance_spawns.clear();
            self.proximity.guidance_throws.clear();
            self.stop_all_guidance();
        }

        if !proximity_active {
            self.proximity.pending.clear();
            self.proximity.known.clear();
            self.proximity.recent.clear();
        } else {
            for source in std::mem::take(&mut self.proximity.pending) {
                if self.proximity_source_current(source)
                    && tick >= source.tick
                    && tick <= source.tick.saturating_add(1)
                    && !self.match_proximity_throw(source)
                    && tick == source.tick
                {
                    self.proximity.pending.push(source);
                }
            }
            self.proximity.known.retain(|handle| s2sdk::IsValidEntHandle(*handle));
            self.proximity
                .recent
                .retain(|handle, (_, seen)| s2sdk::IsValidEntHandle(*handle) && tick >= *seen && tick <= seen.saturating_add(1));
            for handle in all_projectiles() {
                if self.proximity.known.insert(handle) && !new_round && !self.projectiles.manages_projectile(handle) {
                    let owner = thrower(handle);
                    if let Some(task) = (0..s2sdk::GetMaxClients()).find_map(|slot| {
                        let player = LivePlayer::resolve(slot)?;
                        (player.pawn_handle == owner).then(|| self.combat_task(player)).flatten()
                    }) {
                        self.proximity.recent.insert(handle, (task, tick));
                    }
                }
            }
            let settings = self.controller.current_mode().and_then(|m| m.proximity_fuse.clone());
            let now = settings.as_ref().map_or(0.0, |_| s2sdk::GetGameTime());
            let mut team_relation_fault = None;
            for (handle, mut grenade) in std::mem::take(&mut self.proximity.grenades) {
                if !s2sdk::IsValidEntHandle(handle)
                    || self.projectiles.manages_projectile(handle)
                    || s2sdk::GetEntityClassname(handle).as_str() != classname(grenade.source.kind)
                {
                    continue;
                }
                let proximity_current = self.proximity_source_current(grenade.source) && thrower(handle) == grenade.source.player.pawn_handle;
                if !proximity_current {
                    if let Some(fuse_pause) = relinquish_stale_proximity_ownership(&mut grenade) {
                        // The proximity manager no longer owns this source, but it
                        // must not drop a deadline lease still owned by HE guidance.
                        self.queue_guidance_fuse_restore(handle, grenade.source, fuse_pause);
                        self.proximity.grenades.insert(handle, grenade);
                    }
                    continue;
                }
                let Some(settings) = settings
                    .as_ref()
                    .filter(|settings| proximity_current && settings.grenades.contains(&grenade.source.kind))
                else {
                    self.proximity.grenades.insert(handle, grenade);
                    continue;
                };
                if !grenade.claimed {
                    // A delayed fuse uses the game's clock and one target identity.
                    // If that player ceases to qualify, discard the timer before
                    // selecting any other currently eligible enemy.
                    if settings.delay_seconds > 0.0 && !now.is_finite() {
                        grenade.waiting = None;
                        self.proximity.grenades.insert(handle, grenade);
                        continue;
                    }
                    grenade.waiting = grenade.waiting.filter(|waiting| {
                        if self.combat_task(waiting.player) != Some(waiting.task) || LivePlayer::resolve(waiting.player.slot) != Some(waiting.player)
                        {
                            return false;
                        }
                        match qualifies(grenade.source.player, waiting.player, grenade.source.kind, handle, settings.radius) {
                            Ok(qualifies) => qualifies,
                            Err(error) => {
                                team_relation_fault.get_or_insert((Mechanism::ProximityFuse(grenade.source.kind), error));
                                false
                            }
                        }
                    });
                    if grenade.waiting.is_none() {
                        grenade.waiting = (0..s2sdk::GetMaxClients()).find_map(|slot| {
                            let player = LivePlayer::resolve(slot)?;
                            let task = self.combat_task(player)?;
                            match qualifies(grenade.source.player, player, grenade.source.kind, handle, settings.radius) {
                                Ok(true) => Some(WaitingTarget {
                                    player,
                                    task,
                                    started_at: now,
                                }),
                                Ok(false) => None,
                                Err(error) => {
                                    team_relation_fault.get_or_insert((Mechanism::ProximityFuse(grenade.source.kind), error));
                                    None
                                }
                            }
                        });
                    }
                    if let Some(waiting) = grenade.waiting
                        && elapsed(now, waiting.started_at, settings.delay_seconds)
                    {
                        let action = Action {
                            source: grenade.source,
                            handle,
                            target: waiting.player,
                            target_task: waiting.task,
                            radius: settings.radius,
                        };
                        grenade.claimed = true;
                        self.engine_actions.push(EngineAction::Proximity(action));
                    }
                }
                self.proximity.grenades.insert(handle, grenade);
            }
            if let Some((mechanism, error)) = team_relation_fault {
                self.handle_fault(Fault::mechanism(mechanism, error));
                return;
            }
        }
        if guidance_active {
            self.update_he_guidance();
        }
    }

    fn associate_he_guidance_throws(&mut self, tick: i32) -> Result<(), Fault> {
        let round = self.controller.generation();
        let epoch = crate::HE_GUIDANCE_SPAWN_EPOCH.load(std::sync::atomic::Ordering::Acquire);
        let throws = self.proximity.guidance_throws.clone();
        let spawns = self.proximity.guidance_spawns.clone();
        let owners = spawns
            .iter()
            .enumerate()
            .map(|(index, spawn)| (index, thrower(spawn.handle)))
            .collect::<HashMap<_, _>>();
        let candidates = guidance_candidate_pairs(&throws, &spawns, &owners, round, epoch);
        let resolution = resolve_guidance_candidates(&candidates, &throws, &spawns, tick);
        let now = s2sdk::GetGameTime();
        let settings = self.controller.current_mode().and_then(|mode| mode.he_guidance);
        let mut discard_throws = resolution.discard_throws;
        let mut discard_spawns = resolution.discard_spawns;
        for (throw_count, spawn_count) in resolution.ambiguous_components {
            log(&format!(
                "HE guidance association ambiguous; native trajectory retained: throws={throw_count} projectiles={spawn_count}"
            ));
        }
        for (throw_index, spawn_index) in resolution.pairs {
            let source = throws[throw_index].source;
            let spawn = spawns[spawn_index];
            let handle = spawn.handle;
            discard_throws.insert(throw_index);
            discard_spawns.insert(spawn_index);
            if !now.is_finite() || !spawn.game_time.is_finite() || now < spawn.game_time || settings.is_none() {
                continue;
            }
            let settings = settings.expect("checked above");
            let selected = self
                .select_he_guidance_target(source, handle, settings)
                .map_err(|error| Fault::mechanism(Mechanism::HeGuidance, error))?;
            if !merge_guidance_association(&mut self.proximity.grenades, handle, source, selected, spawn.game_time, now) {
                log(&format!(
                    "HE guidance association conflicts with existing projectile owner: handle={handle} slot={}",
                    source.player.slot
                ));
            } else if selected.is_none() {
                log(&format!(
                    "HE guidance found no eligible target at spawn; native fuse retained: handle={handle} slot={}",
                    source.player.slot
                ));
            }
        }

        for (index, throw) in throws.iter().enumerate() {
            if discard_throws.contains(&index)
                && !candidates.iter().any(|(throw_index, _)| *throw_index == index)
                && self.guidance_source_current(throw.source)
            {
                log(&format!(
                    "HE guidance source has no matching projectile; native trajectory retained: slot={} pawn={} tick={}",
                    throw.source.player.slot, throw.source.player.pawn_handle, throw.source.tick
                ));
            }
        }
        self.proximity.guidance_throws = throws
            .into_iter()
            .enumerate()
            .filter_map(|(index, source)| {
                (!discard_throws.contains(&index) && source.epoch == epoch && self.guidance_source_current(source.source)).then_some(source)
            })
            .collect();
        self.proximity.guidance_spawns = spawns
            .into_iter()
            .enumerate()
            .filter_map(|(index, spawn)| {
                (!discard_spawns.contains(&index) && spawn.epoch == epoch && s2sdk::IsValidEntHandle(spawn.handle)).then_some(spawn)
            })
            .collect();
        Ok(())
    }

    fn select_he_guidance_target(
        &self,
        source: Source,
        handle: i32,
        settings: funrounds_core::HeGuidanceSettings,
    ) -> Result<Option<(LivePlayer, PlayerTask)>, &'static str> {
        let origin = s2sdk::GetEntityAbsOrigin(handle);
        if ![origin.x, origin.y, origin.z].iter().all(|value| value.is_finite()) {
            return Ok(None);
        }
        let mut nearest: Option<(f64, LivePlayer, PlayerTask)> = None;
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(target) = LivePlayer::resolve(slot) else { continue };
            let Some(task) = self.combat_task(target) else { continue };
            if !qualifies(source.player, target, GrenadeKind::He, handle, settings.range)? {
                continue;
            }
            let feet = s2sdk::GetClientAbsOrigin(slot);
            let eyes = s2sdk::GetClientEyePosition(slot);
            let center = Vec3 {
                x: feet.x + (eyes.x - feet.x) * 0.5,
                y: feet.y + (eyes.y - feet.y) * 0.5,
                z: feet.z + (eyes.z - feet.z) * 0.5,
            };
            let distance_squared =
                f64::from(origin.x - center.x).powi(2) + f64::from(origin.y - center.y).powi(2) + f64::from(origin.z - center.z).powi(2);
            if distance_squared.is_finite()
                && nearest.is_none_or(|(distance, player, _)| {
                    guidance_target_order(
                        (distance_squared, target.slot, target.pawn_handle),
                        (distance, player.slot, player.pawn_handle),
                    )
                    .is_lt()
                })
            {
                nearest = Some((distance_squared, target, task));
            }
        }
        Ok(nearest.map(|(_, player, task)| (player, task)))
    }

    fn update_he_guidance(&mut self) {
        let Some(settings) = self.controller.current_mode().and_then(|mode| mode.he_guidance) else {
            self.stop_all_guidance();
            return;
        };
        let now = s2sdk::GetGameTime();
        if !now.is_finite() {
            return;
        }
        let handles = self.proximity.grenades.keys().copied().collect::<Vec<_>>();
        for handle in handles {
            let Some(grenade) = self.proximity.grenades.get(&handle).cloned() else {
                continue;
            };
            let Some(mut guidance) = grenade.guidance else { continue };
            let projectile_valid = s2sdk::IsValidEntHandle(handle)
                && !self.projectiles.manages_projectile(handle)
                && s2sdk::GetEntityClassname(handle).as_str() == "hegrenade_projectile"
                && thrower(handle) == grenade.source.player.pawn_handle;
            if !self.guidance_source_current(grenade.source) || !projectile_valid || grenade.claimed {
                self.stop_grenade_guidance(handle);
                continue;
            }

            let Some(remaining_budget) = remaining_guidance_budget(guidance.spawned_at, now, settings.max_guidance_seconds) else {
                self.stop_grenade_guidance(handle);
                continue;
            };

            let target_valid = if !guidance.force_retarget
                && self.combat_task(guidance.target) == Some(guidance.target_task)
                && LivePlayer::resolve(guidance.target.slot) == Some(guidance.target)
            {
                match qualifies(grenade.source.player, guidance.target, GrenadeKind::He, handle, settings.range) {
                    Ok(valid) => valid,
                    Err(error) => {
                        self.handle_fault(Fault::mechanism(Mechanism::HeGuidance, error));
                        return;
                    }
                }
            } else {
                false
            };
            if !target_valid {
                let selected = match self.select_he_guidance_target(grenade.source, handle, settings) {
                    Ok(selected) => selected,
                    Err(error) => {
                        self.handle_fault(Fault::mechanism(Mechanism::HeGuidance, error));
                        return;
                    }
                };
                let Some((target, target_task)) = selected else {
                    self.stop_grenade_guidance(handle);
                    continue;
                };
                retarget_guidance(&mut guidance, target, target_task);
                guidance.force_retarget = false;
            }

            let delta = now - guidance.last_game_time;
            if !delta.is_finite() || delta < 0.0 {
                self.stop_grenade_guidance(handle);
                continue;
            }

            let deadline = grenade_detonate_deadline(handle);
            let (fuse_pause, pause_fuse) = if let Some(fuse_pause) = grenade.guidance_fuse {
                let pause_fuse = !fuse_pause.confirmed;
                if !guidance_deadline_owned(fuse_pause, deadline, f64::from(now), pause_fuse) {
                    // A near-miss, native detonation, or another owner changed the
                    // deadline. Its earlier/current value wins; never write over it.
                    if let Some(entry) = self.proximity.grenades.get_mut(&handle) {
                        entry.guidance = None;
                        entry.guidance_fuse = None;
                    }
                    continue;
                }
                (fuse_pause, pause_fuse)
            } else {
                if !deadline.is_finite() {
                    self.handle_fault(Fault::mechanism(Mechanism::HeGuidance, "native HE fuse deadline is unavailable"));
                    self.stop_grenade_guidance(handle);
                    continue;
                }
                if deadline <= f64::from(now) {
                    self.stop_grenade_guidance(handle);
                    continue;
                }
                let Some(fuse_pause) = guidance_fuse_pause(deadline, f64::from(now), remaining_budget) else {
                    self.handle_fault(Fault::mechanism(
                        Mechanism::HeGuidance,
                        "HE guidance fuse hold deadline cannot be represented",
                    ));
                    self.stop_grenade_guidance(handle);
                    continue;
                };
                (fuse_pause, true)
            };

            guidance.last_game_time = now;
            if let Some(entry) = self.proximity.grenades.get_mut(&handle) {
                entry.guidance = Some(guidance);
                entry.guidance_fuse = Some(fuse_pause);
            }
            self.engine_actions.push(EngineAction::HeGuidance(GuidanceAction {
                source: grenade.source,
                handle,
                target: guidance.target,
                target_task: guidance.target_task,
                settings,
                delta_seconds: delta,
                game_time: now,
                fuse_pause,
                pause_fuse,
            }));
        }
    }

    pub(crate) fn cancel_proximity_action(&mut self, action: Action) {
        if let Some(grenade) = self.proximity.grenades.get_mut(&action.handle)
            && grenade.source.task == action.source.task
            && grenade.source.kind == action.source.kind
        {
            grenade.claimed = false;
            grenade.waiting = None;
        }
    }

    pub(super) fn proximity_action_valid(&self, action: Action) -> bool {
        self.proximity_active()
            && self.proximity_source_current(action.source)
            && self.combat_task(action.target) == Some(action.target_task)
            && LivePlayer::resolve(action.target.slot) == Some(action.target)
            && s2sdk::IsValidEntHandle(action.handle)
            && !self.projectiles.manages_projectile(action.handle)
            && thrower(action.handle) == action.source.player.pawn_handle
            && s2sdk::GetEntityClassname(action.handle).as_str() == classname(action.source.kind)
            && qualifies(action.source.player, action.target, action.source.kind, action.handle, action.radius).is_ok_and(|qualifies| qualifies)
    }

    pub(crate) fn mark_he_guidance_detonated(&mut self, inflictor: i32, damage_type: u32, skip_plugin_rules: bool) {
        if !he_damage_is_detonation(damage_type, skip_plugin_rules) {
            return;
        }
        if let Some(grenade) = self.proximity.grenades.get_mut(&inflictor)
            && grenade.source.kind == GrenadeKind::He
        {
            grenade.guidance = None;
            // The native detonation already owns this entity. Do not restore a
            // held fuse after its real blast has started.
            grenade.guidance_fuse = None;
        }
    }

    pub(crate) fn reject_he_guidance_action(&mut self, action: GuidanceAction) {
        let matches = self.proximity.grenades.get(&action.handle).is_some_and(|grenade| {
            grenade.source == action.source
                && grenade.guidance_fuse == Some(action.fuse_pause)
                && grenade.guidance.is_some_and(|guidance| {
                    guidance.target == action.target && guidance.target_task == action.target_task && guidance.last_game_time == action.game_time
                })
        });
        if !matches {
            return;
        }
        if self.he_guidance_action_base_valid(action) && !self.he_guidance_target_valid(action) {
            request_guidance_retarget(&mut self.proximity.grenades, action);
        } else {
            self.stop_grenade_guidance(action.handle);
        }
    }

    fn he_guidance_action_base_valid(&self, action: GuidanceAction) -> bool {
        let now = s2sdk::GetGameTime();
        self.he_guidance_active()
            && self.on_game_thread()
            && action.source.kind == GrenadeKind::He
            && action.delta_seconds.is_finite()
            && action.delta_seconds >= 0.0
            && action.game_time.is_finite()
            && self.controller.current_mode().and_then(|mode| mode.he_guidance) == Some(action.settings)
            && now.is_finite()
            && self.guidance_source_current(action.source)
            && s2sdk::IsValidEntHandle(action.handle)
            && !self.projectiles.manages_projectile(action.handle)
            && s2sdk::GetEntityClassname(action.handle).as_str() == "hegrenade_projectile"
            && thrower(action.handle) == action.source.player.pawn_handle
            && self.proximity.grenades.get(&action.handle).is_some_and(|grenade| {
                !grenade.claimed
                    && grenade.source == action.source
                    && grenade.guidance_fuse == Some(action.fuse_pause)
                    && grenade.guidance.is_some_and(|guidance| {
                        !guidance.force_retarget
                            && guidance.target == action.target
                            && guidance.target_task == action.target_task
                            && guidance.last_game_time == action.game_time
                            && remaining_guidance_budget(guidance.spawned_at, now, action.settings.max_guidance_seconds).is_some()
                    })
            })
            && {
                let deadline = grenade_detonate_deadline(action.handle);
                guidance_deadline_owned(action.fuse_pause, deadline, f64::from(now), action.pause_fuse)
            }
    }

    fn he_guidance_target_valid(&self, action: GuidanceAction) -> bool {
        self.combat_task(action.target) == Some(action.target_task)
            && LivePlayer::resolve(action.target.slot) == Some(action.target)
            && qualifies(action.source.player, action.target, GrenadeKind::He, action.handle, action.settings.range).is_ok_and(|qualifies| qualifies)
    }

    pub(super) fn he_guidance_action_valid(&self, action: GuidanceAction) -> bool {
        self.he_guidance_action_base_valid(action) && self.he_guidance_target_valid(action)
    }

    pub(crate) fn confirm_he_guidance_fuse_pause(&mut self, action: GuidanceAction) -> bool {
        if !action.pause_fuse
            || action.fuse_pause.confirmed
            || !self.on_game_thread()
            || !s2sdk::IsValidEntHandle(action.handle)
            || s2sdk::GetEntityClassname(action.handle).as_str() != "hegrenade_projectile"
            || thrower(action.handle) != action.source.player.pawn_handle
            || !same_fuse_deadline(grenade_detonate_deadline(action.handle), action.fuse_pause.held_deadline)
        {
            return false;
        }
        let Some(grenade) = self.proximity.grenades.get_mut(&action.handle) else {
            return false;
        };
        if grenade.source != action.source
            || grenade.guidance_fuse != Some(action.fuse_pause)
            || !grenade.guidance.is_some_and(|guidance| {
                guidance.target == action.target && guidance.target_task == action.target_task && guidance.last_game_time == action.game_time
            })
        {
            return false;
        }
        grenade.guidance_fuse.as_mut().expect("matched fuse snapshot").confirmed = true;
        true
    }

    pub(crate) fn he_guidance_fuse_restore_action_valid(&self, action: GuidanceFuseRestoreAction) -> bool {
        self.on_game_thread()
            && self.proximity.grenades.get(&action.handle).is_some_and(|grenade| {
                grenade.source == action.source && grenade.guidance.is_none() && grenade.guidance_fuse == Some(action.fuse_pause)
            })
            && s2sdk::IsValidEntHandle(action.handle)
            && !self.projectiles.manages_projectile(action.handle)
            && s2sdk::GetEntityClassname(action.handle).as_str() == "hegrenade_projectile"
            && thrower(action.handle) == action.source.player.pawn_handle
    }

    pub(crate) fn complete_he_guidance_fuse_restore(&mut self, action: GuidanceFuseRestoreAction) {
        let may_discard_grenade = !self.proximity_active();
        let remove = self.proximity.grenades.get_mut(&action.handle).is_some_and(|grenade| {
            if grenade.source != action.source || grenade.guidance_fuse != Some(action.fuse_pause) {
                return false;
            }
            grenade.guidance_fuse = None;
            grenade.guidance.is_none() && may_discard_grenade
        });
        if remove {
            self.proximity.grenades.remove(&action.handle);
        }
    }

    pub(crate) fn take_he_guidance_fuse_restore_actions(&mut self) -> Vec<GuidanceFuseRestoreAction> {
        let mut actions = Vec::new();
        self.engine_actions.retain(|action| {
            if let EngineAction::HeGuidanceFuseRestore(action) = action {
                actions.push(*action);
                false
            } else {
                true
            }
        });
        actions
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ShockwaveAction {
    task: PlayerTask,
    player: LivePlayer,
    delta: Vec3,
}
impl ShockwaveAction {
    pub(super) fn involves(self, slots: &[i32]) -> bool {
        slots.contains(&self.player.slot)
    }

    pub(crate) fn player(self) -> LivePlayer {
        self.player
    }
}

impl Runtime {
    fn he_impulse_strength(&self) -> Option<f32> {
        if !self.classic_round() || !active_combat_phase(self.controller.phase()) {
            return None;
        }
        self.controller.current_mode()?.hegrenade_impulse.map(|settings| settings.impulse)
    }

    pub(crate) fn observe_he_blast(&mut self, damage: &crate::damage::Damage, victim: Option<LivePlayer>) {
        if damage.damage_type & crate::damage::DMG_BLAST == 0 || damage.amount <= 0.0 {
            return;
        }
        let Some(impulse) = self.he_impulse_strength().filter(|strength| *strength > 0.0) else {
            return;
        };
        let Some(player) = victim.filter(|player| LivePlayer::resolve(player.slot) == Some(*player)) else {
            return;
        };
        let Some(task) = self.combat_task(player) else { return };
        let victims = self.proximity.impulse_hits.entry(damage.inflictor).or_default();
        if victims.contains(&task.player) {
            return;
        }
        victims.push(task.player);
        let origin = s2sdk::GetEntityAbsOrigin(damage.inflictor);
        let mut center = s2sdk::GetEntityAbsOrigin(player.pawn_handle);
        center.z += 36.0;
        if let Some(delta) = funrounds_core::shockwave_impulse([origin.x, origin.y, origin.z], [center.x, center.y, center.z], impulse) {
            self.engine_actions.push(EngineAction::Shockwave(ShockwaveAction {
                task,
                player,
                delta: Vec3 {
                    x: delta[0],
                    y: delta[1],
                    z: delta[2],
                },
            }));
        } else {
            log(&format!(
                "HE shockwave has no finite direction: projectile={} victim={}",
                damage.inflictor, player.pawn_handle
            ));
        }
    }

    pub(super) fn takeover_shockwave_targets(&mut self, owners: &[i32], task: PlayerTask) {
        for victims in self.proximity.impulse_hits.values_mut() {
            for victim in victims {
                if owners.contains(&victim.slot) && victim.pawn_handle == task.player.pawn_handle {
                    *victim = task.player;
                }
            }
        }
    }

    pub(super) fn shockwave_action_valid(&self, action: ShockwaveAction) -> bool {
        self.he_impulse_strength().is_some()
            && self.combat_task(action.player) == Some(action.task)
            && LivePlayer::resolve(action.player.slot) == Some(action.player)
            && action.player.pawn_controller().is_some()
    }
}

pub(crate) fn execute_shockwave(action: ShockwaveAction) -> Result<(), Fault> {
    let before = s2sdk::GetEntityAbsVelocity(action.player.pawn_handle);
    let after = Vec3 {
        x: before.x + action.delta.x,
        y: before.y + action.delta.y,
        z: before.z + action.delta.z,
    };
    if ![before.x, before.y, before.z, after.x, after.y, after.z].iter().all(|v| v.is_finite()) {
        return Err(Fault::mechanism(Mechanism::HeGrenadeImpulse, "HE shockwave velocity unavailable"));
    }
    s2sdk::SetEntityAbsVelocity(action.player.pawn_handle, &after);
    if s2sdk::IsValidEntHandle(action.player.pawn_handle) {
        let actual = s2sdk::GetEntityAbsVelocity(action.player.pawn_handle);
        if ![(actual.x, after.x), (actual.y, after.y), (actual.z, after.z)]
            .iter()
            .all(|(a, b)| a.is_finite() && (a - b).abs() <= 0.1)
        {
            return Err(Fault::mechanism(
                Mechanism::HeGrenadeImpulse,
                "HE shockwave velocity write/readback failed",
            ));
        }
    }
    Ok(())
}

pub(crate) fn execute_guidance(
    mut action: GuidanceAction,
    current: impl Fn(GuidanceAction) -> bool,
    confirm_fuse_pause: impl FnOnce() -> bool,
) -> Result<(), Fault> {
    let fault = |reason| Fault::mechanism(Mechanism::HeGuidance, reason);
    if !current(action) {
        return Ok(());
    }
    if !s2sdk::IsValidEntHandle(action.handle)
        || s2sdk::GetEntityClassname(action.handle).as_str() != "hegrenade_projectile"
        || thrower(action.handle) != action.source.player.pawn_handle
        || !action.delta_seconds.is_finite()
        || action.delta_seconds < 0.0
    {
        return Err(Fault::cancelled("HE guidance projectile or action changed before execution"));
    }
    let deadline = grenade_detonate_deadline(action.handle);
    let now = s2sdk::GetGameTime();
    if !now.is_finite() {
        return Err(fault("HE guidance game time is unavailable before fuse ownership check"));
    }
    if action.pause_fuse && !action.fuse_pause.confirmed && guidance_deadline_owned(action.fuse_pause, deadline, f64::from(now), action.pause_fuse) {
        if !current(action)
            || !s2sdk::IsValidEntHandle(action.handle)
            || s2sdk::GetEntityClassname(action.handle).as_str() != "hegrenade_projectile"
            || thrower(action.handle) != action.source.player.pawn_handle
            || !same_fuse_deadline(grenade_detonate_deadline(action.handle), action.fuse_pause.native_deadline)
        {
            return Ok(());
        }
        set_grenade_detonate_deadline(action.handle, action.fuse_pause.held_deadline);
        let actual_deadline = grenade_detonate_deadline(action.handle);
        if !s2sdk::IsValidEntHandle(action.handle)
            || s2sdk::GetEntityClassname(action.handle).as_str() != "hegrenade_projectile"
            || thrower(action.handle) != action.source.player.pawn_handle
            || !same_fuse_deadline(actual_deadline, action.fuse_pause.held_deadline)
        {
            return Err(fault("HE guidance native fuse pause write/readback failed"));
        }
        if !confirm_fuse_pause() {
            return Err(fault("HE guidance fuse pause lost runtime ownership before confirmation"));
        }
        action.fuse_pause.confirmed = true;
        action.pause_fuse = false;
    } else if !guidance_deadline_owned(action.fuse_pause, deadline, f64::from(now), action.pause_fuse) {
        return Err(fault("HE guidance native fuse deadline changed before pause"));
    }
    if !current(action) || action.delta_seconds == 0.0 {
        return Ok(());
    }
    let velocity = s2sdk::GetEntityAbsVelocity(action.handle);
    let origin = s2sdk::GetEntityAbsOrigin(action.handle);
    let feet = s2sdk::GetClientAbsOrigin(action.target.slot);
    let eyes = s2sdk::GetClientEyePosition(action.target.slot);
    let target = Vec3 {
        x: feet.x + (eyes.x - feet.x) * 0.5,
        y: feet.y + (eyes.y - feet.y) * 0.5,
        z: feet.z + (eyes.z - feet.z) * 0.5,
    };
    let direction = [target.x - origin.x, target.y - origin.y, target.z - origin.z];
    let velocity = [velocity.x, velocity.y, velocity.z];
    if velocity.iter().chain(direction.iter()).any(|value| !value.is_finite()) {
        return Err(fault("HE guidance projectile velocity or target direction is unavailable"));
    }
    let desired = funrounds_core::turn_projectile_velocity(velocity, direction, action.delta_seconds, action.settings)
        .ok_or_else(|| fault("HE guidance velocity calculation rejected current motion"))?;
    let output = Vec3 {
        x: desired[0],
        y: desired[1],
        z: desired[2],
    };
    if !current(action) {
        return Ok(());
    }
    s2sdk::SetEntityAbsVelocity(action.handle, &output);
    if !s2sdk::IsValidEntHandle(action.handle)
        || s2sdk::GetEntityClassname(action.handle).as_str() != "hegrenade_projectile"
        || thrower(action.handle) != action.source.player.pawn_handle
    {
        return Err(fault("HE guidance projectile identity changed after velocity write"));
    }
    let actual = s2sdk::GetEntityAbsVelocity(action.handle);
    if ![(actual.x, output.x), (actual.y, output.y), (actual.z, output.z)]
        .iter()
        .all(|(actual, expected)| actual.is_finite() && (actual - expected).abs() <= 0.1)
    {
        return Err(fault("HE guidance velocity write/readback failed"));
    }
    Ok(())
}

pub(crate) fn execute_guidance_fuse_restore(action: GuidanceFuseRestoreAction, current: impl Fn() -> bool) -> Result<(), Fault> {
    let fault = |reason| Fault::mechanism(Mechanism::HeGuidance, reason);
    if !current() {
        return Ok(());
    }
    if !s2sdk::IsValidEntHandle(action.handle)
        || s2sdk::GetEntityClassname(action.handle).as_str() != "hegrenade_projectile"
        || thrower(action.handle) != action.source.player.pawn_handle
    {
        return Ok(());
    }
    let now = s2sdk::GetGameTime();
    if !now.is_finite() {
        return Err(fault("HE guidance fuse restore has no finite game time"));
    }
    let deadline = grenade_detonate_deadline(action.handle);
    // Trust the live deadline, not confirmation bookkeeping: a setter may have
    // landed even when its immediate readback failed. Only our held value is
    // ours to restore; native or any other value belongs to another outcome.
    if same_fuse_deadline(deadline, action.fuse_pause.native_deadline)
        || !same_fuse_deadline(deadline, action.fuse_pause.held_deadline)
        || action.fuse_pause.held_deadline <= f64::from(now)
    {
        return Ok(());
    }
    let resume_deadline = f64::from((f64::from(now) + action.fuse_pause.remaining_seconds) as f32);
    if !resume_deadline.is_finite() || resume_deadline <= f64::from(now) {
        return Err(fault("HE guidance fuse remaining time cannot be represented"));
    }
    if !current()
        || !s2sdk::IsValidEntHandle(action.handle)
        || s2sdk::GetEntityClassname(action.handle).as_str() != "hegrenade_projectile"
        || thrower(action.handle) != action.source.player.pawn_handle
        || !same_fuse_deadline(grenade_detonate_deadline(action.handle), action.fuse_pause.held_deadline)
    {
        return Ok(());
    }
    set_grenade_detonate_deadline(action.handle, resume_deadline);
    let actual = grenade_detonate_deadline(action.handle);
    if !s2sdk::IsValidEntHandle(action.handle)
        || s2sdk::GetEntityClassname(action.handle).as_str() != "hegrenade_projectile"
        || thrower(action.handle) != action.source.player.pawn_handle
        || !same_fuse_deadline(actual, resume_deadline)
    {
        return Err(fault("HE guidance native fuse restore write/readback failed"));
    }
    Ok(())
}

fn clear_line(start: Vec3, end: Vec3, ignore: i32, target: Option<i32>) -> bool {
    let mut position = Vec3 {
        x: f32::NAN,
        y: f32::NAN,
        z: f32::NAN,
    };
    let mut fraction = f64::NAN;
    let mut hit = true;
    let mut entity = -1;
    let mut solid = false;
    let _ = s2sdk::TraceLine(
        &start,
        &end,
        TRACE_MASK,
        ignore,
        &mut position,
        &mut fraction,
        &mut hit,
        &mut entity,
        &mut solid,
    );
    fraction.is_finite()
        && (0.0..=1.0).contains(&fraction)
        && [position.x, position.y, position.z].iter().all(|v| v.is_finite())
        && !solid
        && (target.is_some_and(|target| hit && target == entity) || !hit && fraction >= 0.999)
}

fn qualifies(source: LivePlayer, target: LivePlayer, kind: GrenadeKind, handle: i32, radius: f32) -> Result<bool, &'static str> {
    let source_team = s2sdk::GetClientTeam(source.slot) as i32;
    if source == target || !matches!(source_team, 2 | 3) {
        return Ok(false);
    }
    if s2sdk::GetClientTeam(target.slot) as i32 == source_team && super::shooting::teammates_are_friendly()? {
        return Ok(false);
    }
    let origin = s2sdk::GetEntityAbsOrigin(handle);
    let feet = s2sdk::GetClientAbsOrigin(target.slot);
    let eyes = s2sdk::GetClientEyePosition(target.slot);
    let body = Vec3 {
        x: feet.x + (eyes.x - feet.x) * 0.5,
        y: feet.y + (eyes.y - feet.y) * 0.5,
        z: feet.z + (eyes.z - feet.z) * 0.5,
    };
    let distance = f64::from(origin.x - body.x).powi(2) + f64::from(origin.y - body.y).powi(2) + f64::from(origin.z - body.z).powi(2);
    if !distance.is_finite() || distance > f64::from(radius).powi(2) {
        return Ok(false);
    }
    if needs_facing(kind) {
        // Flash proximity is only useful while the target faces the flash and
        // its eyes have a clear path to the projectile.
        Ok(faces_point(eyes, s2sdk::GetClientEyeAngles(target.slot), origin) && clear_line(eyes, origin, target.pawn_handle, Some(handle)))
    } else {
        Ok(clear_line(origin, body, handle, Some(target.pawn_handle)))
    }
}

fn elapsed(now: f32, started_at: f32, delay_seconds: f32) -> bool {
    delay_seconds == 0.0 || now.is_finite() && started_at.is_finite() && f64::from(now) - f64::from(started_at) >= f64::from(delay_seconds)
}

fn needs_facing(kind: GrenadeKind) -> bool {
    kind == GrenadeKind::Flash
}

fn faces_point(eyes: Vec3, angles: Vec3, point: Vec3) -> bool {
    if ![eyes.x, eyes.y, eyes.z, angles.x, angles.y, angles.z, point.x, point.y, point.z]
        .iter()
        .all(|value| value.is_finite())
    {
        return false;
    }
    let forward = crate::projectiles::forward(angles);
    let toward_point = [
        f64::from(point.x) - f64::from(eyes.x),
        f64::from(point.y) - f64::from(eyes.y),
        f64::from(point.z) - f64::from(eyes.z),
    ];
    let dot = f64::from(forward.x) * toward_point[0] + f64::from(forward.y) * toward_point[1] + f64::from(forward.z) * toward_point[2];
    dot.is_finite() && dot > 0.0
}

pub(crate) fn execute(action: Action) -> Result<(), Fault> {
    if matches!(action.source.kind, GrenadeKind::Molotov | GrenadeKind::Incendiary) {
        return descend(action.handle, action.source.kind);
    }
    if let Some(kind) = native_kind(action.source.kind) {
        let Some(request) = (unsafe { crate::funrounds_native::__funrounds_native_DetonateGrenade }) else {
            return Err(Fault::mechanism(
                Mechanism::ProximityFuse(action.source.kind),
                "native grenade detonation unavailable",
            ));
        };
        if !matches!(unsafe { request(action.handle, kind) }, 0 | 1) {
            return Err(Fault::mechanism(
                Mechanism::ProximityFuse(action.source.kind),
                "native grenade detonation rejected",
            ));
        }
    } else {
        let pointer = s2sdk::EntHandleToEntPointer(action.handle);
        let now = f64::from(s2sdk::GetGameTime());
        let previous = s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(PROJECTILE_CLASS), &Str::from_str("m_flDetonateTime"), 0);
        // Do not revive or delay an already due natural detonation.
        if previous.is_finite() && previous > now {
            s2sdk::SetEntSchemaFloat2(
                pointer,
                &Str::from_str(PROJECTILE_CLASS),
                &Str::from_str("m_flDetonateTime"),
                now,
                true,
                0,
            );
            if s2sdk::IsValidEntHandle(action.handle)
                && s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(PROJECTILE_CLASS), &Str::from_str("m_flDetonateTime"), 0) != now
            {
                return Err(Fault::mechanism(
                    Mechanism::ProximityFuse(action.source.kind),
                    "grenade deadline write/readback failed",
                ));
            }
        }
    }
    Ok(())
}
// One velocity impulse: keep horizontal travel, original fuse and collision
// handling. A committed impulse is not a property lease to restore on disable.
fn descend(handle: i32, kind: GrenadeKind) -> Result<(), Fault> {
    let fault = |reason| Fault::mechanism(Mechanism::ProximityFuse(kind), reason);
    let query =
        unsafe { crate::funrounds_native::__funrounds_native_GetGrenadeDetonationState }.ok_or_else(|| fault("native grenade state unavailable"))?;
    match unsafe { query(handle, native_kind(kind).expect("fire grenade kind")) } {
        1 => return Ok(()), // Natural collision/detonation already owns this entity.
        0 => {}
        _ => return Err(fault("native grenade state rejected")),
    }
    let previous = s2sdk::GetEntityAbsVelocity(handle);
    if ![previous.x, previous.y, previous.z].iter().all(|value| value.is_finite()) {
        return Err(fault("grenade velocity read failed"));
    }
    let desired = Vec3 {
        z: previous.z.min(-1200.0),
        ..previous
    };
    s2sdk::SetEntityAbsVelocity(handle, &desired);
    if s2sdk::IsValidEntHandle(handle) {
        let actual = s2sdk::GetEntityAbsVelocity(handle);
        if ![(actual.x, desired.x), (actual.y, desired.y), (actual.z, desired.z)]
            .iter()
            .all(|(actual, desired)| actual.is_finite() && (actual - desired).abs() <= 0.1)
        {
            return Err(fault("grenade descent write/readback failed"));
        }
    }
    Ok(())
}
