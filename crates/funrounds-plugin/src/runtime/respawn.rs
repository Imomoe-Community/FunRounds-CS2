// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{EngineAction, Runtime, Str, log};
use crate::{damage, execute_valid_action, fault};
use crate::{game::LivePlayer, s2sdk};
use funrounds_core::{Phase, PlayerTask, PlayerTracker, RoundGeneration, RoundRespawn};

#[derive(Default)]
pub(super) struct State {
    pub(super) dead: Vec<(PlayerTask, LivePlayer, i32, f32)>,
    pub(super) round: Vec<(PlayerTask, LivePlayer, i32, f32)>,
    pub(super) finishes: Vec<PlayerTask>,
}

impl State {
    pub(super) fn queue_finish(&mut self, task: PlayerTask) {
        if !self.finishes.contains(&task) {
            self.finishes.push(task);
        }
    }

    pub(super) fn cancel_round_respawn(&mut self, slot: i32) {
        self.round.retain(|(task, ..)| task.player.slot != slot);
    }

    pub(super) fn cancel_player_respawns(&mut self, slot: i32) {
        self.dead.retain(|(task, ..)| task.player.slot != slot);
        self.cancel_round_respawn(slot);
    }

    pub(super) fn cancel_slots(&mut self, slots: &[i32]) {
        self.dead.retain(|(task, ..)| !slots.contains(&task.player.slot));
        self.round.retain(|(task, ..)| !slots.contains(&task.player.slot));
        self.finishes.retain(|task| !slots.contains(&task.player.slot));
    }

    pub(super) fn clear_respawns(&mut self) {
        self.dead.clear();
        self.round.clear();
    }

    pub(super) fn clear(&mut self) {
        self.clear_respawns();
        self.finishes.clear();
    }
}

impl Runtime {
    pub(super) fn queue_respawn(&mut self, slot: i32, delay: f32) {
        if self.controller.phase() == Phase::Idle || LivePlayer::resolve(slot).is_some() {
            return;
        }
        self.connect(slot);
        let Some(player) = LivePlayer::resolve_present(slot) else { return };
        let team = s2sdk::GetClientTeam(slot) as i32;
        if !matches!(team, 2 | 3) {
            return;
        }
        let Some(task) = self.players.respawn_task(slot, self.controller.generation()) else {
            return;
        };
        if !self.respawning.dead.iter().any(|(pending, ..)| *pending == task) {
            self.respawning.dead.push((task, player, team, s2sdk::GetGameTime() + delay));
        }
    }

    pub(super) fn queue_round_respawn(&mut self, slot: i32, tick: i32, delay: f32) {
        if self.controller.phase() == Phase::Idle || !self.controller.current_mode().is_some_and(|mode| mode.round_respawn == RoundRespawn::All) {
            return;
        }
        let team = s2sdk::GetClientTeam(slot) as i32;
        if !matches!(team, 2 | 3) {
            return;
        }
        self.connect(slot);
        let Some(player) = LivePlayer::resolve_present(slot) else { return };
        if LivePlayer::resolve(slot).is_some()
            && self
                .players
                .identity(slot)
                .is_none_or(|identity| identity.pawn_handle != player.pawn_handle as u32)
        {
            // Register an alive player's current life without applying effects; its real spawn follows ForceRespawnClient.
            self.spawn(slot, tick, false);
        }
        let Some(task) = round_respawn_task(&self.players, self.controller.generation(), player) else {
            return;
        };
        if self.controllers.get(&slot) != Some(&player.controller_handle) {
            return;
        }
        let now = s2sdk::GetGameTime();
        if !now.is_finite() || !delay.is_finite() || delay < 0.0 {
            return;
        }
        if !self.respawning.round.iter().any(|(pending, ..)| *pending == task) {
            self.respawning.round.push((task, player, team, now + delay));
        }
    }

    pub(super) fn respawn_current(&self, task: PlayerTask, player: LivePlayer, team: i32, now: f32) -> bool {
        respawn_window_open(self.controller.phase(), self.team_match.as_ref().map(|game| game.remaining(now)))
            && self.players.validates_respawn(task, self.controller.generation())
            && respawn_target_valid(
                player,
                LivePlayer::resolve_present(player.slot),
                team,
                s2sdk::GetClientTeam(player.slot) as i32,
                LivePlayer::resolve(player.slot).is_some(),
            )
    }

    pub(super) fn round_respawn_current(&self, task: PlayerTask, player: LivePlayer, team: i32, now: f32) -> bool {
        round_respawn_task_valid(&self.players, task, self.controller.generation(), player)
            && respawn_window_open(self.controller.phase(), self.team_match.as_ref().map(|game| game.remaining(now)))
            && round_respawn_target_valid(
                self.controller.current_mode().is_some_and(|mode| mode.round_respawn == RoundRespawn::All),
                self.controller.phase(),
                player,
                LivePlayer::resolve_present(task.player.slot),
                team,
                s2sdk::GetClientTeam(task.player.slot) as i32,
            )
    }

    pub(super) fn low_health_finish_current(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.controller.phase() != Phase::Idle
            && self.controller.current_mode().is_some_and(|mode| mode.self_damage_finish_enabled())
            && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
            && LivePlayer::resolve(player.slot) == Some(player)
            && funrounds_core::low_health_finish(s2sdk::GetClientHealth(player.slot))
    }

    pub(super) fn collect_respawn_actions(&mut self, now: f32, actions: &mut Vec<EngineAction>) {
        for (task, player, team, due) in std::mem::take(&mut self.respawning.dead) {
            if !self.respawn_current(task, player, team, now) {
                continue;
            }
            if now < due {
                self.respawning.dead.push((task, player, team, due));
            } else {
                log(&format!("respawn slot={} team={team}", task.player.slot));
                actions.push(EngineAction::Respawn(Action::Player(task, player, team)));
            }
        }
        for (task, player, team, due) in std::mem::take(&mut self.respawning.round) {
            if !self.round_respawn_current(task, player, team, now) {
                continue;
            }
            if now < due {
                self.respawning.round.push((task, player, team, due));
            } else {
                log(&format!("round respawn slot={} team={team}", task.player.slot));
                actions.push(EngineAction::Respawn(Action::RoundStart(task, player, team)));
            }
        }
        for task in std::mem::take(&mut self.respawning.finishes) {
            if let Some(player) = LivePlayer::resolve(task.player.slot)
                && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
                && self.controller.current_mode().is_some_and(|mode| mode.self_damage_finish_enabled())
                && funrounds_core::low_health_finish(s2sdk::GetClientHealth(player.slot))
            {
                actions.push(EngineAction::Respawn(Action::LowHealthFinish(
                    task,
                    player,
                    self.controller.current_mode().is_some_and(|mode| mode.native_self_damage_enabled()),
                )));
            }
        }
    }
}

fn respawn_target_valid(expected: LivePlayer, current: Option<LivePlayer>, team: i32, current_team: i32, alive: bool) -> bool {
    current == Some(expected) && matches!(team, 2 | 3) && current_team == team && !alive
}

fn round_respawn_task(players: &PlayerTracker, round: RoundGeneration, player: LivePlayer) -> Option<PlayerTask> {
    if let Some(identity) = players
        .identity(player.slot)
        .filter(|identity| identity.pawn_handle == player.pawn_handle as u32)
    {
        return Some(PlayerTask { player: identity, round });
    }
    players.respawn_task(player.slot, round)
}

fn round_respawn_task_valid(players: &PlayerTracker, task: PlayerTask, round: RoundGeneration, player: LivePlayer) -> bool {
    task.player.slot == player.slot
        && if task.player.pawn_handle == u32::MAX {
            players.validates_respawn(task, round)
        } else {
            players.validates(task, round, player.pawn_handle as u32)
        }
}

fn round_respawn_target_valid(
    all_respawn: bool,
    phase: Phase,
    expected: LivePlayer,
    current: Option<LivePlayer>,
    team: i32,
    current_team: i32,
) -> bool {
    all_respawn && phase != Phase::Idle && current == Some(expected) && matches!(team, 2 | 3) && current_team == team
}

fn respawn_window_open(phase: Phase, remaining: Option<f32>) -> bool {
    phase == Phase::Preparing || phase == Phase::Warmup || (phase == Phase::Running && remaining.is_none_or(|seconds| seconds > 0.0))
}

pub enum Action {
    Player(PlayerTask, LivePlayer, i32),
    RoundStart(PlayerTask, LivePlayer, i32),
    LowHealthFinish(PlayerTask, LivePlayer, bool),
}

impl Action {
    pub(super) fn involves(&self, slots: &[i32]) -> bool {
        match self {
            Self::Player(task, ..) | Self::RoundStart(task, ..) | Self::LowHealthFinish(task, ..) => slots.contains(&task.player.slot),
        }
    }
}

impl Runtime {
    pub(super) fn respawn_action_current(&self, action: &Action) -> bool {
        match action {
            Action::Player(task, player, team) => self.respawn_current(*task, *player, *team, s2sdk::GetGameTime()),
            Action::RoundStart(task, player, team) => self.round_respawn_current(*task, *player, *team, s2sdk::GetGameTime()),
            Action::LowHealthFinish(task, player, _) => self.low_health_finish_current(*task, *player),
        }
    }
}

pub(crate) fn execute(action: Action) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        Action::Player(task, player, team) => execute_valid_action(EngineAction::Respawn(Action::Player(task, player, team)), || {
            s2sdk::RespawnClient(task.player.slot);
            Ok(())
        })?,
        Action::RoundStart(task, player, team) => execute_valid_action(EngineAction::Respawn(Action::RoundStart(task, player, team)), || {
            s2sdk::ForceRespawnClient(task.player.slot);
            Ok(())
        })?,
        Action::LowHealthFinish(task, player, numeric) => {
            execute_valid_action(EngineAction::Respawn(Action::LowHealthFinish(task, player, numeric)), || {
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn team_respawn_works_during_freeze_without_starting_the_score_timer() {
        use funrounds_core::Phase;
        assert!(super::respawn_window_open(Phase::Preparing, None));
        assert!(super::respawn_window_open(Phase::Running, Some(3.0)));
        assert!(!super::respawn_window_open(Phase::Running, Some(0.0)));
        assert!(super::respawn_window_open(Phase::Running, None));
        assert!(!super::respawn_window_open(Phase::Idle, Some(30.0)));
    }

    #[test]
    fn respawn_rejects_changed_handles_team_and_external_resurrection() {
        let player = crate::game::LivePlayer {
            slot: 1,
            controller_handle: 0x4001,
            pawn_handle: 0x4002,
        };
        assert!(super::respawn_target_valid(player, Some(player), 2, 2, false));
        assert!(!super::respawn_target_valid(player, Some(player), 2, 3, false));
        assert!(!super::respawn_target_valid(player, Some(player), 2, 2, true));
        assert!(!super::respawn_target_valid(player, None, 2, 2, false));
        assert!(!super::respawn_target_valid(
            player,
            Some(crate::game::LivePlayer {
                pawn_handle: 0x8002,
                ..player
            }),
            2,
            2,
            false
        ));
        assert!(!super::respawn_target_valid(
            player,
            Some(crate::game::LivePlayer {
                controller_handle: 0x8001,
                ..player
            }),
            2,
            2,
            false
        ));
    }
}
