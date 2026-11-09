// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use funrounds_core::regions::{
    MEDICAL_SMOKE_HEAL, MEDICAL_SMOKE_INTERVAL_SECONDS, MEDICAL_SMOKE_RADIUS, Position, TrampolineLatch, TrampolineRegion, fixed_trampoline_region,
    medical_smoke_team,
};
use plugify::{Str, Vec3};
use std::collections::{HashMap, HashSet};

const SMOKE_PROJECTILE: &str = "smokegrenade_projectile";
const GRENADE_CLASS: &str = "CBaseGrenade";
const THROWER_FIELD: &str = "m_hThrower";
const ON_GROUND: i32 = 1;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Prerequisites {
    trampoline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SmokeThrow {
    player: PlayerIdentity,
    team: Option<i32>,
    round: RoundGeneration,
    tick: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SmokeProjectile {
    handle: i32,
    thrower: i32,
    tick: i32,
    round: RoundGeneration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SmokeReceipt {
    team: i32,
    round: RoundGeneration,
}

#[derive(Clone, Copy)]
struct SmokeDetonation {
    handle: i32,
    entity_index: i32,
    origin: Position,
    round: RoundGeneration,
    tick: i32,
    time: f64,
}

impl SmokeDetonation {
    fn can_wait_for_receipt(self, tick: i32, round: RoundGeneration) -> bool {
        // At most one tick separates the observations; both adjacent-tick
        // windows must close before the pair can be accepted.
        self.round == round && tick <= self.tick.saturating_add(3)
    }
}

#[derive(Clone, Copy)]
struct SmokeCloud {
    handle: i32,
    entity_index: i32,
    team: i32,
    origin: Position,
    round: RoundGeneration,
}

#[derive(Default)]
pub(super) struct State {
    prerequisites: Prerequisites,
    smoke_active: bool,
    pending_throws: Vec<SmokeThrow>,
    smoke_projectiles: Vec<SmokeProjectile>,
    smoke_receipts: HashMap<i32, SmokeReceipt>,
    pending_detonations: HashMap<i32, SmokeDetonation>,
    observed_smokes: HashSet<i32>,
    smokes: HashMap<i32, SmokeCloud>,
    next_smoke_pulse: Option<f64>,
    map_name: Option<String>,
    trampoline_region: Option<TrampolineRegion>,
    trampoline_latches: HashMap<i32, TrampolineLatch>,
}

impl State {
    fn begin_smoke_activation(&mut self, existing: impl IntoIterator<Item = i32>) {
        self.smoke_active = true;
        self.pending_throws.clear();
        self.smoke_projectiles.clear();
        self.smoke_receipts.clear();
        self.pending_detonations.clear();
        self.smokes.clear();
        self.next_smoke_pulse = None;
        self.observed_smokes = existing.into_iter().collect();
    }

    fn disable_smoke(&mut self) {
        self.smoke_active = false;
        self.pending_throws.clear();
        self.smoke_projectiles.clear();
        self.smoke_receipts.clear();
        self.pending_detonations.clear();
        self.observed_smokes.clear();
        self.smokes.clear();
        self.next_smoke_pulse = None;
    }

    fn observe_smoke_throw(&mut self, throw: SmokeThrow) {
        self.pending_throws.push(throw);
    }

    fn observe_smoke_projectile(&mut self, projectile: SmokeProjectile) {
        if !self.observed_smokes.insert(projectile.handle) {
            return;
        }
        self.smoke_projectiles.push(projectile);
    }

    /// Pair by thrower handle and the adjacent engine tick window. Ambiguous many-to-many pairs are discarded.
    fn resolve_smoke_projectiles(&mut self, tick: i32, round: RoundGeneration) {
        let mut throws_by_projectile = vec![Vec::new(); self.smoke_projectiles.len()];
        let mut projectiles_by_throw = vec![Vec::new(); self.pending_throws.len()];
        for (throw_index, throw) in self.pending_throws.iter().enumerate() {
            for (projectile_index, projectile) in self.smoke_projectiles.iter().enumerate() {
                if throw.round == round
                    && projectile.round == round
                    && projectile.thrower == throw.player.pawn_handle as i32
                    && throw.tick.abs_diff(projectile.tick) <= 1
                {
                    projectiles_by_throw[throw_index].push(projectile_index);
                    throws_by_projectile[projectile_index].push(throw_index);
                }
            }
        }

        let mut resolved_throws = HashSet::new();
        let mut resolved_projectiles = HashSet::new();
        for (throw_index, projectile_indexes) in projectiles_by_throw.iter().enumerate() {
            if projectile_indexes.len() > 1 || projectile_indexes.iter().any(|index| throws_by_projectile[*index].len() > 1) {
                // Do not let an ambiguous association become a match as older
                // candidates independently age out of the window.
                resolved_throws.insert(throw_index);
                resolved_projectiles.extend(projectile_indexes.iter().copied());
                continue;
            }
            let [projectile_index] = projectile_indexes.as_slice() else { continue };
            let throw = self.pending_throws[throw_index];
            let projectile = self.smoke_projectiles[*projectile_index];
            if tick <= throw.tick.saturating_add(1) || tick <= projectile.tick.saturating_add(1) {
                continue;
            }
            let detonation = self.pending_detonations.remove(&projectile.handle);
            if let Some(team) = throw.team {
                if let Some(detonation) = detonation {
                    if detonation.can_wait_for_receipt(tick, round) {
                        self.open_smoke_cloud(detonation, team);
                    }
                } else {
                    self.smoke_receipts.insert(projectile.handle, SmokeReceipt { team, round: throw.round });
                }
            }
            resolved_throws.insert(throw_index);
            resolved_projectiles.insert(*projectile_index);
        }

        self.pending_throws = self
            .pending_throws
            .drain(..)
            .enumerate()
            .filter_map(|(index, throw)| {
                (!resolved_throws.contains(&index)
                    && throw.round == round
                    // A projectile can arrive one tick after its throw and needs
                    // its own adjacent tick to leave the association window.
                    && tick <= throw.tick.saturating_add(2))
                .then_some(throw)
            })
            .collect();
        self.smoke_projectiles = self
            .smoke_projectiles
            .drain(..)
            .enumerate()
            .filter_map(|(index, projectile)| {
                (!resolved_projectiles.contains(&index) && projectile.round == round && tick <= projectile.tick.saturating_add(2))
                    .then_some(projectile)
            })
            .collect();
        self.pending_detonations
            .retain(|_, detonation| detonation.can_wait_for_receipt(tick, round));
    }

    fn observe_smoke_detonation(&mut self, detonation: SmokeDetonation) -> bool {
        let handle = detonation.handle;
        if self.smokes.contains_key(&handle) {
            return false;
        }
        if let Some(team) = self.take_smoke_receipt(handle, detonation.round) {
            self.open_smoke_cloud(detonation, team);
            return true;
        }
        // Keep the first native event, not a borrowed event pointer. Only a
        // same-round receipt for the exact full handle can turn it into a cloud.
        self.pending_detonations.entry(handle).or_insert(detonation);
        false
    }

    fn open_smoke_cloud(&mut self, detonation: SmokeDetonation, team: i32) {
        self.smokes.insert(
            detonation.handle,
            SmokeCloud {
                handle: detonation.handle,
                entity_index: detonation.entity_index,
                team,
                origin: detonation.origin,
                round: detonation.round,
            },
        );
        self.next_smoke_pulse.get_or_insert(detonation.time + MEDICAL_SMOKE_INTERVAL_SECONDS);
    }

    fn take_smoke_receipt(&mut self, handle: i32, round: RoundGeneration) -> Option<i32> {
        let receipt = self.smoke_receipts.remove(&handle)?;
        (receipt.round == round).then_some(receipt.team)
    }

    fn forget_player_latch(&mut self, slot: i32) {
        self.trampoline_latches.remove(&slot);
    }
}

pub(super) fn probe(events: &EventCapabilities, capabilities: &mut funrounds_core::Capabilities, reasons: &mut Vec<String>) -> State {
    let mut smoke = crate::capability::Probe::default();
    crate::capability::imports!(smoke;
        __s2sdk_GetGameTime, __s2sdk_GetGameTickCount, __s2sdk_GetFirstActiveEntity, __s2sdk_GetNextActiveEntity,
        __s2sdk_GetMaxClients, __s2sdk_IsClientInGame, __s2sdk_IsClientAlive,
        __s2sdk_GetClientTeam, __s2sdk_GetClientAbsOrigin, __s2sdk_GetClientHealth, __s2sdk_SetClientHealth,
        __s2sdk_PlayerSlotToEntHandle, __s2sdk_GetEventPlayerSlot, __s2sdk_GetEventString,
        __s2sdk_GetEventInt, __s2sdk_GetEventFloat, __s2sdk_EntIndexToEntHandle,
        __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer, __s2sdk_EntPointerToEntHandle,
        __s2sdk_GetEntityClassname, __s2sdk_GetEntSchemaEnt2, __s2sdk_FindConVar, __s2sdk_GetConVarValue,
    );
    smoke.require(events.grenade_thrown, "event:grenade_thrown");
    smoke.require(events.smoke_events, "events:smokegrenade_detonate/smokegrenade_expired");
    smoke.require(events.combat_events, "events:player_spawn/player_death");
    smoke.require(
        shooting::teammates_are_friendly().is_ok(),
        "convar:mp_teammates_are_enemies (readable boolean)",
    );
    smoke.schema(GRENADE_CLASS, THROWER_FIELD);
    capabilities.medical_smoke = smoke.record("medical_smoke", reasons);

    let mut trampoline = crate::capability::Probe::default();
    crate::capability::imports!(trampoline;
        __s2sdk_GetCurrentMap, __s2sdk_GetGameTickCount, __s2sdk_GetMaxClients,
        __s2sdk_GetClientAbsOrigin, __s2sdk_GetClientAbsVelocity, __s2sdk_SetClientAbsVelocity,
        __s2sdk_GetClientFlags,
    );
    trampoline.require(events.round_prestart, "event:round_prestart");
    trampoline.require(events.combat_events, "events:player_spawn/player_death");
    let trampoline_ready = trampoline.record("trampoline", reasons);
    capabilities.trampoline = false;
    if trampoline_ready {
        reasons.push("trampoline:[waiting for map readiness]".into());
    }

    State {
        prerequisites: Prerequisites {
            trampoline: trampoline_ready,
        },
        ..State::default()
    }
}

impl Runtime {
    fn medical_smoke_enabled(&self) -> bool {
        self.classic_enabled()
            && self.effects_ready
            && active_combat_phase(self.controller.phase())
            && self.faults.capabilities().medical_smoke
            && self
                .controller
                .current_mode()
                .is_some_and(|mode| mode.medical_smoke && mode.teammates_as_enemies != Some(true))
    }

    fn trampoline_enabled(&self) -> bool {
        self.classic_enabled()
            && self.effects_ready
            && active_combat_phase(self.controller.phase())
            && self.faults.capabilities().trampoline
            && self.controller.current_mode().is_some_and(|mode| mode.trampoline)
            && self.regions.trampoline_region.is_some()
    }

    fn sync_smoke_tracking(&mut self, enabled: bool) {
        match (enabled, self.regions.smoke_active) {
            (true, false) => {
                let existing = game::entity_handles(SMOKE_PROJECTILE)
                    .into_iter()
                    .filter(|handle| complete_entity(*handle))
                    .collect::<Vec<_>>();
                self.regions.begin_smoke_activation(existing);
            }
            (false, true) => self.regions.disable_smoke(),
            _ => {}
        }
    }

    fn scan_smoke_projectiles(&mut self, tick: i32) {
        if tick < 0 || !self.regions.smoke_active {
            return;
        }
        let round = self.controller.generation();
        self.regions.observed_smokes.retain(|handle| complete_entity(*handle));
        self.regions.smoke_receipts.retain(|handle, _| complete_entity(*handle));
        self.regions
            .smoke_projectiles
            .retain(|projectile| projectile.round == round && is_smoke_projectile(projectile.handle));

        // Discovery needs the active entity list only while a throw is awaiting
        // attribution. Existing candidates can be checked by full handle.
        if !self.regions.pending_throws.is_empty() {
            for projectile in &mut self.regions.smoke_projectiles {
                let pointer = s2sdk::EntHandleToEntPointer(projectile.handle);
                projectile.thrower = s2sdk::GetEntSchemaEnt2(pointer, &Str::from_str(GRENADE_CLASS), &Str::from_str(THROWER_FIELD), 0);
            }
            for handle in game::entity_handles(SMOKE_PROJECTILE) {
                if self.regions.observed_smokes.contains(&handle) || !complete_entity(handle) {
                    continue;
                }
                let pointer = s2sdk::EntHandleToEntPointer(handle);
                let thrower = s2sdk::GetEntSchemaEnt2(pointer, &Str::from_str(GRENADE_CLASS), &Str::from_str(THROWER_FIELD), 0);
                self.regions.observe_smoke_projectile(SmokeProjectile {
                    handle,
                    thrower,
                    tick,
                    round,
                });
            }
        }
        self.regions.pending_detonations.retain(|handle, _| is_smoke_projectile(*handle));
        self.regions.resolve_smoke_projectiles(tick, round);
    }

    /// Read the event while its borrowed engine pointer is valid; only owned identities and values persist.
    pub(super) fn on_region_event(&mut self, name: &str, event: usize) {
        match name {
            "smokegrenade_detonate" => self.start_smoke(event),
            "smokegrenade_expired" => self.expire_smoke(event),
            _ => {}
        }
    }

    pub(super) fn record_smoke_throw(&mut self, slot: i32) {
        let enabled = self.medical_smoke_enabled();
        self.sync_smoke_tracking(enabled);
        if !enabled {
            return;
        }
        let friendly = match shooting::teammates_are_friendly() {
            Ok(friendly) => friendly,
            Err(error) => {
                self.handle_fault(Fault::mechanism(Mechanism::MedicalSmoke, error));
                return;
            }
        };
        let Some(player) = LivePlayer::resolve(slot).filter(|player| player.pawn_controller() == Some(player.controller_handle)) else {
            return;
        };
        let Some(task) = self.combat_task(player) else { return };
        // Consume native throws even when they cannot heal, so their older
        // projectiles are not rediscovered after the friendly-fire policy changes.
        let team = medical_smoke_team(s2sdk::GetClientTeam(slot) as i32, !friendly);
        let tick = s2sdk::GetGameTickCount();
        if tick < 0 {
            return;
        }
        self.regions.observe_smoke_throw(SmokeThrow {
            player: task.player,
            team,
            round: task.round,
            tick,
        });
        self.scan_smoke_projectiles(tick);
    }

    fn start_smoke(&mut self, event: usize) {
        let enabled = self.medical_smoke_enabled();
        self.sync_smoke_tracking(enabled);
        if !enabled {
            return;
        }
        let entity_index = s2sdk::GetEventInt(event, &Str::from_str("entityid"));
        let handle = s2sdk::EntIndexToEntHandle(entity_index);
        if !is_smoke_projectile(handle) {
            return;
        }
        let origin = Position {
            x: s2sdk::GetEventFloat(event, &Str::from_str("x")),
            y: s2sdk::GetEventFloat(event, &Str::from_str("y")),
            z: s2sdk::GetEventFloat(event, &Str::from_str("z")),
        };
        if !origin.is_finite() {
            return;
        }
        let now = f64::from(s2sdk::GetGameTime());
        if !now.is_finite() {
            self.handle_fault(Fault::mechanism(
                Mechanism::MedicalSmoke,
                "game time is not finite when a smoke cloud starts",
            ));
            return;
        }
        let tick = s2sdk::GetGameTickCount();
        if tick < 0 {
            return;
        }
        let matched = self.regions.observe_smoke_detonation(SmokeDetonation {
            handle,
            entity_index,
            origin,
            round: self.controller.generation(),
            tick,
            time: now,
        });
        if self.diagnostics() {
            log(&format!("medical_smoke=detonate tick={tick} handle={handle} matched={matched}"));
        }
    }

    fn expire_smoke(&mut self, event: usize) {
        let entity_index = s2sdk::GetEventInt(event, &Str::from_str("entityid"));
        let event_handle = s2sdk::EntIndexToEntHandle(entity_index);
        let mut expired = self
            .regions
            .smokes
            .iter()
            .filter_map(|(handle, smoke)| {
                (smoke.entity_index == entity_index && (event_handle == *handle || !complete_entity(*handle))).then_some(*handle)
            })
            .collect::<HashSet<_>>();
        expired.extend(
            self.regions
                .observed_smokes
                .iter()
                .copied()
                .filter(|handle| event_handle == *handle || !complete_entity(*handle)),
        );
        expired.extend(
            self.regions
                .smoke_receipts
                .keys()
                .copied()
                .filter(|handle| event_handle == *handle || !complete_entity(*handle)),
        );
        self.regions
            .pending_detonations
            .retain(|handle, _| event_handle != *handle && complete_entity(*handle));
        for handle in expired {
            self.regions.smokes.remove(&handle);
            self.regions.smoke_receipts.remove(&handle);
            self.regions.smoke_projectiles.retain(|projectile| projectile.handle != handle);
        }
        self.regions.observed_smokes.retain(|handle| complete_entity(*handle));
        // The native expiry event can arrive before deletion. Never rediscover
        // that same full handle as a new projectile during another throw.
        if is_smoke_projectile(event_handle) {
            self.regions.observed_smokes.insert(event_handle);
        }
        if self.regions.smokes.is_empty() {
            self.regions.next_smoke_pulse = None;
        }
    }

    /// Called from round prestart/start only when the map-query imports passed the startup probe.
    pub(super) fn on_region_map_ready(&mut self) {
        let Some(map_name) = map_name_when_ready(
            self.regions.prerequisites,
            || s2sdk::GetGameTickCount(),
            || s2sdk::GetCurrentMap().to_string(),
        ) else {
            return;
        };
        let region = fixed_trampoline_region(&map_name);
        if self.regions.map_name.as_deref() != Some(map_name.as_str()) {
            self.clear_regions_round();
        }
        self.regions.map_name = Some(map_name);
        self.regions.trampoline_region = region;
        self.refresh_trampoline_availability();
    }

    fn refresh_trampoline_availability(&mut self) {
        let ready = self.regions.prerequisites.trampoline && self.regions.trampoline_region.is_some();
        if self.faults.set_trampoline_available(ready) {
            self.controller.set_capabilities(self.faults.capabilities());
        }
    }

    pub(crate) fn update_regions(&mut self, now: f64) {
        let smoke_enabled = self.medical_smoke_enabled();
        self.sync_smoke_tracking(smoke_enabled);
        if smoke_enabled
            && (!self.regions.pending_throws.is_empty() || !self.regions.smoke_projectiles.is_empty() || !self.regions.pending_detonations.is_empty())
        {
            self.scan_smoke_projectiles(s2sdk::GetGameTickCount());
        }
        if self.regions.smokes.is_empty() {
            self.regions.next_smoke_pulse = None;
        } else {
            let generation = self.controller.generation();
            self.regions
                .smokes
                .retain(|_, smoke| smoke.round == generation && complete_entity(smoke.handle));
            if self.regions.smokes.is_empty() {
                self.regions.next_smoke_pulse = None;
            } else if !now.is_finite() {
                self.regions.smokes.clear();
                self.regions.next_smoke_pulse = None;
                self.handle_fault(Fault::mechanism(Mechanism::MedicalSmoke, "game time is not finite during smoke healing"));
            } else {
                let due = *self.regions.next_smoke_pulse.get_or_insert(now + MEDICAL_SMOKE_INTERVAL_SECONDS);
                if now >= due {
                    self.regions.next_smoke_pulse = Some(now + MEDICAL_SMOKE_INTERVAL_SECONDS);
                    self.heal_smoke_recipients();
                }
            }
        }

        if !self.trampoline_enabled() {
            self.regions.trampoline_latches.clear();
            return;
        }
        self.update_trampoline_players();
    }

    fn heal_smoke_recipients(&mut self) {
        let Some(healing_cap) = self.controller.current_mode().map(|mode| mode.healing_cap_base_multiple) else {
            return;
        };
        match shooting::teammates_are_friendly() {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                self.handle_fault(Fault::mechanism(Mechanism::MedicalSmoke, error));
                return;
            }
        }
        let smokes = self.regions.smokes.values().copied().collect::<Vec<_>>();
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let team = s2sdk::GetClientTeam(slot) as i32;
            let position = position(s2sdk::GetClientAbsOrigin(slot));
            if !position.is_finite() {
                continue;
            }
            if !smokes
                .iter()
                .any(|smoke| smoke.team == team && position.within_sphere(smoke.origin, MEDICAL_SMOKE_RADIUS))
            {
                continue;
            }
            let Some(task) = self.combat_task(player) else { continue };
            let Some(base) = self
                .combat
                .lives
                .get(&slot)
                .filter(|(expected, _)| *expected == task)
                .map(|(_, life)| i32::from(life.attributes().health))
            else {
                continue;
            };
            if self.effects.get(&slot).is_none_or(|effect| effect.has_pending_loadout()) {
                continue;
            }
            if let Err(error) = super::composed::heal(
                slot,
                base,
                funrounds_core::HealingAmount::Fixed(MEDICAL_SMOKE_HEAL),
                healing_cap,
                Mechanism::MedicalSmoke,
            ) {
                self.handle_fault(error);
                self.regions.smokes.clear();
                self.regions.next_smoke_pulse = None;
                return;
            }
        }
    }

    fn update_trampoline_players(&mut self) {
        let Some(region) = self.regions.trampoline_region else { return };
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else {
                self.regions.trampoline_latches.remove(&slot);
                continue;
            };
            let Some(task) = self.combat_task(player) else {
                self.regions.trampoline_latches.remove(&slot);
                continue;
            };
            if self.effects.get(&slot).is_none_or(|effect| effect.has_pending_loadout()) {
                continue;
            }
            let position = position(s2sdk::GetClientAbsOrigin(slot));
            let grounded = s2sdk::GetClientFlags(slot) & ON_GROUND != 0;
            let step = self
                .regions
                .trampoline_latches
                .entry(slot)
                .or_default()
                .observe(task.player, position, grounded, region);
            if step.triggered {
                self.launch_trampoline(player, task.player);
            }
        }
    }

    fn launch_trampoline(&mut self, player: LivePlayer, identity: PlayerIdentity) {
        if let Err(error) = self.interrupt_movement_for_impulse(player) {
            self.handle_fault(error.context("trampoline movement interruption"));
            return;
        }
        if LivePlayer::resolve(player.slot) != Some(player) || self.players.identity(player.slot) != Some(identity) || !self.trampoline_enabled() {
            return;
        }
        let before = s2sdk::GetClientAbsVelocity(player.slot);
        if !finite_velocity(before) {
            self.handle_fault(Fault::mechanism(Mechanism::Trampoline, "grounded player velocity is not finite"));
            return;
        }
        let requested = Vec3 {
            x: before.x,
            y: before.y,
            z: funrounds_core::regions::TRAMPOLINE_LAUNCH_Z,
        };
        s2sdk::SetClientAbsVelocity(player.slot, &requested);
        if LivePlayer::resolve(player.slot) != Some(player) || self.players.identity(player.slot) != Some(identity) {
            return;
        }
        let actual = s2sdk::GetClientAbsVelocity(player.slot);
        if !velocity_matches(actual, requested) {
            self.handle_fault(Fault::mechanism(Mechanism::Trampoline, "vertical launch velocity write/readback failed"));
            return;
        }
        self.say(player.slot, "trampoline.launched", &[]);
    }

    pub(super) fn clear_regions_round(&mut self) {
        self.regions.disable_smoke();
        self.regions.trampoline_latches.clear();
    }

    pub(super) fn clear_regions_map(&mut self) {
        self.clear_regions_round();
        self.regions.map_name = None;
        self.regions.trampoline_region = None;
        self.refresh_trampoline_availability();
    }

    pub(super) fn clear_region_player_life(&mut self, slot: i32) {
        self.regions.forget_player_latch(slot);
    }

    pub(super) fn takeover_trampoline(&mut self, previous_owners: &[i32], identity: PlayerIdentity) {
        let mut transferred = None;
        for slot in previous_owners {
            if let Some(mut latch) = self.regions.trampoline_latches.remove(slot)
                && latch.rebind(identity)
            {
                transferred = Some(latch);
            }
        }
        self.regions.trampoline_latches.remove(&identity.slot);
        if let Some(latch) = transferred {
            self.regions.trampoline_latches.insert(identity.slot, latch);
        }
    }
}

fn map_name_when_ready(prerequisites: Prerequisites, tick_count: impl FnOnce() -> i32, current_map: impl FnOnce() -> String) -> Option<String> {
    if !prerequisites.trampoline || tick_count() < 0 {
        return None;
    }
    Some(current_map())
}

fn is_smoke_projectile(handle: i32) -> bool {
    complete_entity(handle) && s2sdk::GetEntityClassname(handle).as_str() == SMOKE_PROJECTILE
}

fn complete_entity(handle: i32) -> bool {
    if !s2sdk::IsValidEntHandle(handle) {
        return false;
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    pointer != 0 && s2sdk::EntPointerToEntHandle(pointer) == handle
}

fn finite_velocity(velocity: Vec3) -> bool {
    velocity.x.is_finite() && velocity.y.is_finite() && velocity.z.is_finite()
}

fn velocity_matches(actual: Vec3, requested: Vec3) -> bool {
    finite_velocity(actual)
        && [(actual.x, requested.x), (actual.y, requested.y), (actual.z, requested.z)]
            .iter()
            .all(|(actual, requested)| (actual - requested).abs() <= 0.1)
}

fn position(value: Vec3) -> Position {
    Position {
        x: value.x,
        y: value.y,
        z: value.z,
    }
}
