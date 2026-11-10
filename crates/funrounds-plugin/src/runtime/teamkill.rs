// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use funrounds_core::TeamkillCounter;
use plugify::Vec3;

#[derive(Default)]
pub(super) struct State {
    entries: HashMap<i32, Entry>,
    executing: Option<PlayerTask>,
}
struct Entry {
    task: PlayerTask,
    team: i32,
    counter: TeamkillCounter,
    pending: bool,
}
#[derive(Clone, Copy)]
pub(crate) struct Action {
    pub player: LivePlayer,
    pub origin: Vec3,
    task: PlayerTask,
    team: i32,
}
impl Action {
    pub(super) fn involves(self, slots: &[i32]) -> bool {
        slots.contains(&self.player.slot)
    }
}

pub(super) fn probe(events: &EventCapabilities, caps: &mut funrounds_core::Capabilities, reasons: &mut Vec<String>) {
    let mut probe = crate::suicide::resource_probe();
    probe.require(events.combat_events && events.damage, "events:combat/damage/lifecycle");
    probe.require(crate::damage::damage_frame_token_available(), "native:damage_frame");
    probe.require(crate::damage::reflection_available(), "native:TakeEntityDamageWithWeapon");
    crate::capability::imports!(probe; __s2sdk_GetClientTeam, __s2sdk_GetClientHealth, __s2sdk_GetClientArmor,
        __s2sdk_GetEntityAbsOrigin, __s2sdk_FindConVar, __s2sdk_GetConVarValue, __s2sdk_CreateEntityByName,
        __s2sdk_DispatchSpawn2, __s2sdk_AcceptEntityInput, __s2sdk_TeleportEntity, __s2sdk_RemoveEntity);
    probe.require(
        shooting::teammates_are_friendly().is_ok(),
        "convar:mp_teammates_are_enemies (readable boolean)",
    );
    caps.teamkill_punishment = probe.record("teamkill_punishment", reasons);
}

impl Runtime {
    pub(super) fn update_teamkills(&mut self) {
        if !self.classic_round() || !active_combat_phase(self.controller.phase()) {
            self.teamkills.entries.clear();
            return;
        }
        let entries = std::mem::take(&mut self.teamkills.entries);
        self.teamkills.entries = entries
            .into_iter()
            .filter(|(slot, entry)| {
                self.players
                    .validates(entry.task, self.controller.generation(), entry.task.player.pawn_handle)
                    && LivePlayer::resolve(*slot).is_some_and(|p| p.pawn_handle as u32 == entry.task.player.pawn_handle)
                    && s2sdk::GetClientTeam(*slot) as i32 == entry.team
            })
            .collect();
    }

    pub(super) fn is_teamkill_punishment(&self, slot: i32) -> bool {
        self.teamkills
            .executing
            .is_some_and(|task| task.player.slot == slot && self.players.identity(slot) == Some(task.player))
    }

    pub(super) fn count_teamkill(&mut self, victim: i32, attacker: i32) {
        if !self.classic_round() || !active_combat_phase(self.controller.phase()) || victim == attacker {
            return;
        }
        let Some(limit) = self.controller.current_mode().and_then(|mode| mode.teamkill_limit) else {
            return;
        };
        let Some(player) = LivePlayer::resolve(attacker) else { return };
        if player.pawn_controller().is_none() {
            return;
        }
        let Some(task) = self.combat_task(player) else { return };
        let team = s2sdk::GetClientTeam(attacker) as i32;
        if !matches!(team, 2 | 3) || s2sdk::GetClientTeam(victim) as i32 != team {
            return;
        }
        match shooting::teammates_are_friendly() {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                self.handle_fault(Fault::mechanism(Mechanism::TeamkillPunishment, error));
                return;
            }
        }
        let Some(victim_identity) = self.players.identity(victim) else { return };
        let entry = self.teamkills.entries.entry(attacker).or_insert_with(|| Entry {
            task,
            team,
            counter: TeamkillCounter::default(),
            pending: false,
        });
        if entry.task != task || entry.team != team {
            *entry = Entry {
                task,
                team,
                counter: TeamkillCounter::default(),
                pending: false,
            };
        }
        if !entry.pending && entry.counter.record(victim_identity, limit) {
            entry.pending = true;
            self.engine_actions.push(EngineAction::Teamkill(Action {
                player,
                task,
                team,
                origin: s2sdk::GetEntityAbsOrigin(player.pawn_handle),
            }));
        }
    }

    pub(crate) fn teamkill_action_valid(&self, action: Action) -> bool {
        self.teamkill_action_valid_inner(action).is_ok_and(|valid| valid)
    }

    fn teamkill_action_valid_inner(&self, action: Action) -> Result<bool, &'static str> {
        if !self.classic_round() || !active_combat_phase(self.controller.phase()) {
            return Ok(false);
        }
        if !self.controller.current_mode().is_some_and(|mode| mode.teamkill_limit.is_some())
            || self.combat_task(action.player) != Some(action.task)
            || LivePlayer::resolve(action.player.slot) != Some(action.player)
            || action.player.pawn_controller().is_none()
            || s2sdk::GetClientTeam(action.player.slot) as i32 != action.team
            || !self
                .teamkills
                .entries
                .get(&action.player.slot)
                .is_some_and(|entry| entry.task == action.task && entry.pending)
        {
            return Ok(false);
        }
        shooting::teammates_are_friendly()
    }

    pub(crate) fn take_teamkill_actions(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        self.engine_actions.retain(|action| {
            if let EngineAction::Teamkill(action) = action {
                actions.push(*action);
                false
            } else {
                true
            }
        });
        actions
    }

    pub(crate) fn begin_teamkill(&mut self, action: Action) -> bool {
        match self.teamkill_action_valid_inner(action) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                self.handle_fault(Fault::mechanism(Mechanism::TeamkillPunishment, error));
                return false;
            }
        }
        self.teamkills.executing = Some(action.task);
        true
    }
    pub(crate) fn finish_teamkill(&mut self, action: Action) {
        if self.teamkills.executing == Some(action.task) {
            self.teamkills.executing = None;
        }
        if let Some(entry) = self.teamkills.entries.get_mut(&action.player.slot).filter(|e| e.task == action.task) {
            entry.pending = false;
        }
    }
    pub(super) fn forget_teamkill(&mut self, slot: i32) {
        self.teamkills.entries.remove(&slot);
    }

    pub(super) fn takeover_teamkills(&mut self, owners: &[i32], task: PlayerTask, player: LivePlayer) {
        let mut transferred = None;
        for slot in owners.iter().copied().chain(std::iter::once(player.slot)) {
            if let Some(entry) = self.teamkills.entries.remove(&slot)
                && entry.task.round == task.round
                && entry.task.player.pawn_handle == task.player.pawn_handle
                && entry.team == s2sdk::GetClientTeam(player.slot) as i32
            {
                transferred = Some(Entry {
                    task,
                    pending: false,
                    ..entry
                });
            }
        }
        if let Some(entry) = transferred {
            self.teamkills.entries.insert(player.slot, entry);
        }
    }
}

pub(crate) fn punish(player: LivePlayer) -> Result<(), Fault> {
    let health = s2sdk::GetClientHealth(player.slot).max(1) as f32;
    let armor = s2sdk::GetClientArmor(player.slot).max(0) as f32;
    let zero = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    crate::damage::with_internal_damage_context(
        crate::damage::InternalDamageContext {
            victim: player.pawn_handle,
            attacker: player.pawn_handle,
            inflictor: player.pawn_handle,
            weapon: -1,
            victim_slot: player.slot,
            attacker_slot: player.slot,
            headshot: false,
            check_protection: false,
            suppress_lifesteal: true,
            random_heal: None,
            death_weapon: None,
        },
        || {
            crate::damage::apply_native_damage(
                crate::damage::NativeDamageCall {
                    victim: player.pawn_handle,
                    attacker: player.pawn_handle,
                    inflictor: player.pawn_handle,
                    force: &zero,
                    position: &zero,
                    amount: (health + armor + 1.0) * 10.0,
                    damage_type: crate::damage::DMG_HEADSHOT,
                    weapon: -1,
                    skip_plugin_rules: true,
                },
                Mechanism::TeamkillPunishment,
                "teamkill punishment damage rejected",
            )
        },
    )?;
    if LivePlayer::resolve(player.slot) == Some(player) {
        return Err(Fault::mechanism(
            Mechanism::TeamkillPunishment,
            "teamkill offender survived native punishment",
        ));
    }
    Ok(())
}
