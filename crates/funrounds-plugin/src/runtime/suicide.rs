// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    Effect, EngineAction, Fault, LivePlayer, Mechanism, PendingAction, Phase, PlayerIdentity, PlayerTask, Runtime, Str, active_combat_phase, s2sdk,
};
use crate::{RUNTIME, callbacks_active, engine_action_current, equipment_fault, execute_valid_action, process_suicide_action};
use funrounds_core::{PlayerTracker, RoundKey};
use plugify::Vec3;
use rand::seq::SliceRandom;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct DeathExplosions {
    pub(super) armed: HashMap<i32, PlayerTask>,
    candidates: Option<Vec<DeathCandidate>>,
}

struct DeathCandidate {
    life: PlayerIdentity,
    team: i32,
    round_respawn: bool,
}

impl DeathExplosions {
    pub(super) fn takeover(&mut self, owners: &[i32], task: PlayerTask, players: &PlayerTracker) -> bool {
        if let Some(candidates) = &mut self.candidates {
            candidates.retain_mut(|candidate| {
                if owners.contains(&candidate.life.slot)
                    && candidate.life.pawn_handle == task.player.pawn_handle
                    && players.identity(candidate.life.slot) == Some(candidate.life)
                {
                    candidate.life = task.player;
                    candidate.round_respawn = false;
                    return true;
                }
                candidate.life.slot != task.player.slot
            });
        }
        let mut moved = false;
        for &slot in owners {
            if self.armed.remove(&slot).is_some_and(|old| {
                old.round == task.round && old.player.pawn_handle == task.player.pawn_handle && players.identity(slot) == Some(old.player)
            }) {
                moved = true;
            }
        }
        self.armed.remove(&task.player.slot);
        if moved {
            self.armed.insert(task.player.slot, task);
        }
        moved
    }

    pub(super) fn forget(&mut self, slot: i32) {
        self.armed.remove(&slot);
        if let Some(candidates) = &mut self.candidates {
            candidates.retain(|candidate| candidate.life.slot != slot);
        }
    }

    pub(super) fn spawned(&mut self, slot: i32, life: Option<PlayerIdentity>, players: &PlayerTracker) {
        self.armed.remove(&slot);
        if let Some(candidates) = &mut self.candidates {
            candidates.retain_mut(|candidate| {
                if candidate.life.slot != slot {
                    return true;
                }
                if let Some(life) = life
                    && candidate.round_respawn
                    && players.validates_connection(candidate.life)
                {
                    candidate.life = life;
                    candidate.round_respawn = false;
                    return true;
                }
                false
            });
        }
    }

    pub(super) fn bind_candidate(&mut self, pending: PlayerIdentity, bound: PlayerIdentity) {
        if let Some(candidates) = &mut self.candidates
            && let Some(candidate) = candidates.iter_mut().find(|candidate| candidate.life == pending)
        {
            candidate.life = bound;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DeathBlast {
    victim: PlayerIdentity,
    pub player: LivePlayer,
    pub origin: Vec3,
    round_key: RoundKey,
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

fn lethal_suicide_finish_amount(anchor: f32, health: i32) -> Option<f32> {
    if !anchor.is_finite() || anchor <= 0.0 || health <= 0 {
        return None;
    }
    let current_health_amount = health as f32 * 10.0;
    current_health_amount.is_finite().then_some(anchor.max(current_health_amount))
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

impl Runtime {
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
            .any(|queued| matches!(queued, EngineAction::Suicide(Action::GrantC4(queued_task, queued_player)) if *queued_task == task && *queued_player == player))
        {
            self.engine_actions.push(EngineAction::Suicide(Action::GrantC4(task, player)));
        }
        true
    }

    pub(super) fn queue_c4_assignment(&mut self) {
        if !active_combat_phase(self.controller.phase())
            || self.suicides.c4_assigned
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
        self.suicides.c4_assigned = true;
        self.engine_actions.extend(
            players
                .into_iter()
                .map(|(task, player, native)| EngineAction::Suicide(Action::RemoveC4(task, player, native))),
        );
        candidates.shuffle(&mut self.rng);
        self.engine_actions.extend(
            candidates
                .into_iter()
                .take(count)
                .map(|(task, player)| EngineAction::Suicide(Action::GrantC4(task, player))),
        );
    }

    pub fn track_suicide_feedback(&mut self, handle: i32, now: f32) -> bool {
        self.suicides.feedback.track(handle, now + crate::suicide::FEEDBACK_LIFETIME)
    }

    pub fn take_suicide_feedback_cleanup(&mut self) -> Vec<i32> {
        self.suicides.feedback.take_removals()
    }

    pub(super) fn suicide_enabled(&self) -> bool {
        self.classic_enabled() && active_combat_phase(self.controller.phase()) && self.controller.current_mode().is_some_and(|mode| mode.suicide)
    }

    pub(super) fn c4_purchase_enabled(&self, start: bool) -> bool {
        self.classic_enabled()
            && active_combat_phase(self.controller.phase())
            && self
                .controller
                .current_mode()
                .is_some_and(|mode| if start { mode.suicide } else { mode.c4_buy })
    }

    pub(super) fn qualifying_c4(&self, player: LivePlayer) -> Option<i32> {
        s2sdk::GetClientWeapons(player.slot)
            .iter()
            .copied()
            .find(|weapon| crate::suicide::real_c4(player, *weapon) && !self.bombs.excludes_suicide_c4(player, *weapon))
    }

    pub(super) fn request_suicide(&mut self, slot: i32, start: bool) -> Result<String, String> {
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

    pub(super) fn update_suicides(&mut self, tick: i32) -> Vec<EngineAction> {
        if !self.suicide_enabled() {
            let mut due = Vec::new();
            for action in std::mem::take(&mut self.suicides.pending) {
                match action {
                    crate::suicide::Action::Purchase { .. } => due.push(EngineAction::Suicide(Action::SelfDestruct(action))),
                    crate::suicide::Action::Finish {
                        after_round_end: true,
                        tick: queued,
                        ..
                    } if self.action_valid(&EngineAction::Suicide(Action::SelfDestruct(action))) => {
                        if round_end_finish_due(tick, queued) {
                            due.push(EngineAction::Suicide(Action::SelfDestruct(action)));
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
        let mut failure = None;
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
                    failure = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = failure {
            self.handle_fault(Fault::mechanism(Mechanism::Suicide, error));
        }
        self.suicides
            .take_due_actions(tick)
            .map(|action| EngineAction::Suicide(Action::SelfDestruct(action)))
            .collect()
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
        if !self.action_valid(&EngineAction::Suicide(Action::SelfDestruct(action))) {
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

    pub(super) fn start_death_explosives(&mut self) {
        if !self.controller.current_mode().is_some_and(|mode| mode.death_explosion.enabled()) {
            return;
        }
        // Fix the roster at freeze end, even if a round-start respawn delays the draw.
        self.death_explosives.candidates = Some(
            (0..s2sdk::GetMaxClients())
                .filter(|&slot| s2sdk::IsClientInGame(slot))
                .filter_map(|slot| {
                    let team = s2sdk::GetClientTeam(slot) as i32;
                    if !matches!(team, 2 | 3) {
                        return None;
                    }
                    let round_respawn = self.respawning.round.iter().any(|(task, ..)| task.player.slot == slot);
                    let life = if round_respawn {
                        self.players.connection_identity(slot)
                    } else {
                        self.players.identity(slot).or_else(|| self.players.pending_identity(slot))
                    }?;
                    Some(DeathCandidate { life, team, round_respawn })
                })
                .collect(),
        );
        self.arm_death_explosives();
    }

    pub(super) fn arm_death_explosives(&mut self) {
        // With zero freeze time the existing round-start respawns may still be
        // queued. Sample the new lives after those actions have run.
        if !self.classic_enabled() || !self.respawning.round.is_empty() {
            return;
        }
        let Some(mut candidates) = self.death_explosives.candidates.take() else {
            return;
        };
        let Some(quota) = self
            .controller
            .current_mode()
            .map(|mode| mode.death_explosion)
            .filter(|quota| quota.enabled())
        else {
            return;
        };
        let mut awaiting_pawn = false;
        let mut eligible = Vec::new();
        let round = self.controller.generation();
        for candidate in &mut candidates {
            let slot = candidate.life.slot;
            if (if candidate.team == 2 { quota.t } else { quota.ct }) == 0
                || !s2sdk::IsClientInGame(slot)
                || s2sdk::GetClientTeam(slot) as i32 != candidate.team
            {
                continue;
            }
            let player = LivePlayer::resolve(slot).filter(|player| {
                self.controllers.get(&slot) == Some(&player.controller_handle) && player.pawn_controller() == Some(player.controller_handle)
            });
            if self.players.pending_identity(slot) == Some(candidate.life) {
                let Some(player) = player else {
                    awaiting_pawn = true;
                    continue;
                };
                let pending = PlayerTask {
                    player: candidate.life,
                    round,
                };
                let Some(bound) = self.bind_pending_spawn(pending, player) else {
                    continue;
                };
                candidate.life = bound;
                // The loadout and other queued work still belong to this same life.
                for (task, ..) in &mut self.pending {
                    if *task == pending {
                        task.player = bound;
                    }
                }
            }
            if let Some(task) = player.and_then(|player| self.combat_task(player))
                && task.player == candidate.life
                && !eligible.contains(&(candidate.team, task))
            {
                eligible.push((candidate.team, task));
            }
        }
        if awaiting_pawn {
            self.death_explosives.candidates = Some(candidates);
            return;
        }
        for (team, count) in [(2, quota.t), (3, quota.ct)] {
            let mut eligible = eligible
                .iter()
                .filter_map(|(candidate_team, task)| (*candidate_team == team).then_some(*task))
                .collect::<Vec<_>>();
            eligible.shuffle(&mut self.rng);
            for task in eligible.into_iter().take(usize::from(count)) {
                self.death_explosives.armed.insert(task.player.slot, task);
                self.say(task.player.slot, "death_explosion.armed", &[]);
                if self.diagnostics() {
                    super::log(&format!(
                        "death_explosion=armed slot={} pawn={} team={team}",
                        task.player.slot, task.player.pawn_handle
                    ));
                }
            }
        }
    }

    pub(super) fn track_player_death(&mut self, slot: i32) {
        let nominated = if self.death_explosives.armed.is_empty() || self.controller.phase() != Phase::Running {
            None
        } else {
            LivePlayer::resolve_present(slot).filter(|player| self.claim_death_nomination(*player))
        };
        self.death_explosives.forget(slot);
        self.players.death(slot);
        if let (Some(player), Some(round_key), Some(victim)) = (nominated, self.round, self.players.connection_identity(slot)) {
            self.engine_actions.push(EngineAction::Suicide(Action::DeathBlast(DeathBlast {
                victim,
                player,
                origin: s2sdk::GetEntityAbsOrigin(player.pawn_handle),
                round_key,
            })));
        }
    }

    fn claim_death_nomination(&mut self, player: LivePlayer) -> bool {
        let Some(controller) = player.pawn_controller().filter(|controller| *controller == player.controller_handle) else {
            return false;
        };
        if self
            .combat_task(player)
            .is_some_and(|task| self.death_explosives.armed.get(&player.slot) == Some(&task))
        {
            return true;
        }
        // The native death event may precede the next game-thread takeover update.
        // Claim from the old owner without adopting a dead pawn or moving live effects.
        let Some(takeover) = self
            .pending_takeovers
            .iter()
            .find(|pending| pending.slot == player.slot && pending.controller_handle == controller && self.takeover_current(**pending))
        else {
            return false;
        };
        let Some(owner) = self.death_explosives.armed.iter().find_map(|(&slot, old)| {
            (old.round == takeover.round && old.player.pawn_handle == player.pawn_handle as u32 && self.players.identity(slot) == Some(old.player))
                .then_some(slot)
        }) else {
            return false;
        };
        self.death_explosives.armed.remove(&owner);
        true
    }

    fn death_blast_current(&self, blast: DeathBlast) -> bool {
        self.classic_enabled()
            && self.round == Some(blast.round_key)
            && matches!(self.controller.phase(), Phase::Running | Phase::Idle)
            && self.controllers.get(&blast.player.slot) == Some(&blast.player.controller_handle)
            && self.players.connection_identity(blast.player.slot) == Some(blast.victim)
            && LivePlayer::resolve_present(blast.player.slot) == Some(blast.player)
    }

    pub fn prepare_suicide_finish_after_blast(&mut self, action: crate::suicide::Action) {
        let source = action.player();
        let tick = s2sdk::GetGameTickCount();
        let self_amount = s2sdk::GetClientHealth(source.slot) as f32 * 10.0;
        self.suicides.pending.push(crate::suicide::Action::Finish {
            task: action.task(),
            player: source,
            amount: self_amount,
            tick,
            after_round_end: false,
        });
    }

    pub fn prepare_blast_targets(&mut self, source: LivePlayer, origin: Vec3) -> Result<Vec<(PlayerTask, LivePlayer, f32)>, Fault> {
        let tick = s2sdk::GetGameTickCount();
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
        if targets.is_empty() {
            return Ok(targets);
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
        if !self.action_valid(&EngineAction::Suicide(Action::SelfDestruct(action))) {
            return None;
        }
        lethal_suicide_finish_amount(amount, s2sdk::GetClientHealth(player.slot))
    }

    pub fn blast_target_valid(&self, task: PlayerTask, player: LivePlayer) -> bool {
        active_combat_phase(self.controller.phase()) && self.combat_task(player) == Some(task) && LivePlayer::resolve(player.slot) == Some(player)
    }

    pub(super) fn suicide_hurt(&mut self, event: usize, victim: i32, attacker: i32) {
        if !crate::damage::internal_suicide_hurt(victim, attacker) {
            return;
        }
        let Some(player) = LivePlayer::resolve_present(victim) else { return };
        let Some(task) = self.combat_task(player) else { return };
        if let Some((_, life)) = self.combat.lives.get_mut(&victim).filter(|(expected, _)| *expected == task) {
            life.lose_native_armor(s2sdk::GetEventInt(event, &Str::from_str("dmg_armor")));
        }
    }

    pub fn finish_suicide_blast(&mut self) {
        if let Err(error) = self.suicides.ff.restore() {
            self.handle_fault(Fault::global(error));
        }
    }

    pub(super) fn self_destruct_action_current(&self, action: &crate::suicide::Action) -> bool {
        match action {
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
        }
    }
}

pub enum Action {
    GrantC4(PlayerTask, LivePlayer),
    RemoveC4(PlayerTask, LivePlayer, Vec<i32>),
    SelfDestruct(crate::suicide::Action),
    DeathBlast(DeathBlast),
    RemoveFeedback(i32),
}

impl Action {
    pub(super) fn retain_for_takeover(
        &mut self,
        slots: &[i32],
        suicide_source: Option<i32>,
        takeover_task: PlayerTask,
        takeover_player: LivePlayer,
    ) -> bool {
        match self {
            Self::GrantC4(task, _) | Self::RemoveC4(task, ..) => !slots.contains(&task.player.slot),
            Self::DeathBlast(blast) => !slots.contains(&blast.player.slot),
            Self::RemoveFeedback(_) => true,
            Self::SelfDestruct(suicide) => {
                let old = suicide.task();
                if !slots.contains(&old.player.slot) {
                    return true;
                }
                if matches!(suicide, crate::suicide::Action::Purchase { .. })
                    || suicide_source != Some(old.player.slot)
                    || old.round != takeover_task.round
                    || old.player.pawn_handle != takeover_task.player.pawn_handle
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
        }
    }
}

impl Runtime {
    pub(super) fn suicide_action_current(&self, action: &Action) -> bool {
        match action {
            Action::GrantC4(task, player) => {
                self.classic_round()
                    && !self
                        .effects
                        .get(&player.slot)
                        .is_some_and(|effect| effect.identity == task.player && effect.round == task.round && effect.has_pending_loadout())
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            Action::RemoveC4(task, player, _) => {
                self.classic_round()
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            Action::SelfDestruct(action) => self.self_destruct_action_current(action),
            Action::DeathBlast(blast) => self.death_blast_current(*blast),
            Action::RemoveFeedback(_) => true,
        }
    }
}

pub(crate) fn execute(action: Action) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        Action::GrantC4(task, player) => {
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
                return Ok(());
            }
            let action = EngineAction::Suicide(Action::GrantC4(task, player));
            let result = crate::suicide::grant_c4(player, || engine_action_current(&action));
            if let Err(error) = result
                && engine_action_current(&action)
            {
                equipment_fault(error);
            }
        }
        Action::RemoveC4(task, player, handles) => {
            execute_valid_action(EngineAction::Suicide(Action::RemoveC4(task, player, handles.clone())), || {
                remove_native_c4(player, &handles)
            })?;
        }
        Action::SelfDestruct(action) => process_suicide_action(action),
        Action::DeathBlast(blast) => crate::process_player_blast(
            blast.player,
            blast.origin,
            EngineAction::Suicide(Action::DeathBlast(blast)),
            Mechanism::DeathExplosion,
        ),
        Action::RemoveFeedback(handle) => crate::suicide::remove_feedback(handle),
    }
    Ok(())
}
