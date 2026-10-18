// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::RoundGeneration;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerIdentity {
    pub slot: i32,
    connection: u64,
    life: u64,
    last_spawn: Option<u64>,
    pending_pawn: bool,
    pub pawn_handle: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerTask {
    pub round: RoundGeneration,
    pub player: PlayerIdentity,
}

#[derive(Default)]
pub struct PlayerTracker {
    players: HashMap<i32, PlayerIdentity>,
    next_connection: u64,
}

impl PlayerTracker {
    pub fn connect(&mut self, slot: i32) -> bool {
        if slot < 0 || self.players.contains_key(&slot) {
            return false;
        }
        self.next_connection = self.next_connection.checked_add(1).expect("connection generation exhausted");
        self.players.insert(
            slot,
            PlayerIdentity {
                slot,
                connection: self.next_connection,
                life: 0,
                last_spawn: None,
                pending_pawn: false,
                pawn_handle: u32::MAX,
            },
        );
        true
    }

    // Spawn keys are stable, increasing engine event identities within this connection.
    pub fn spawn(&mut self, slot: i32, spawn_key: u64, pawn_handle: u32) -> Option<PlayerIdentity> {
        let player = self.players.get_mut(&slot)?;
        if player.last_spawn.is_some_and(|previous| spawn_key <= previous) {
            return None;
        }
        player.life = player.life.checked_add(1).expect("life generation exhausted");
        player.last_spawn = Some(spawn_key);
        player.pawn_handle = pawn_handle;
        player.pending_pawn = pawn_handle == u32::MAX;
        (pawn_handle != u32::MAX).then_some(*player)
    }

    pub fn pending_identity(&self, slot: i32) -> Option<PlayerIdentity> {
        self.players.get(&slot).copied().filter(|player| player.pending_pawn)
    }

    pub fn bind_spawn(&mut self, expected: PlayerIdentity, pawn_handle: u32) -> Option<PlayerIdentity> {
        let player = self.players.get_mut(&expected.slot)?;
        if *player != expected || !player.pending_pawn || pawn_handle == u32::MAX {
            return None;
        }
        player.pawn_handle = pawn_handle;
        player.pending_pawn = false;
        Some(*player)
    }

    pub fn death(&mut self, slot: i32) -> bool {
        let Some(player) = self.players.get_mut(&slot) else {
            return false;
        };
        if player.pawn_handle == u32::MAX && !player.pending_pawn {
            return false;
        }
        player.life = player.life.checked_add(1).expect("life generation exhausted");
        player.pawn_handle = u32::MAX;
        player.pending_pawn = false;
        true
    }

    pub fn disconnect(&mut self, slot: i32) -> bool {
        self.players.remove(&slot).is_some()
    }

    pub fn identity(&self, slot: i32) -> Option<PlayerIdentity> {
        self.players.get(&slot).copied().filter(|player| player.pawn_handle != u32::MAX)
    }

    pub fn connection_identity(&self, slot: i32) -> Option<PlayerIdentity> {
        self.players.get(&slot).copied()
    }

    pub fn validates_connection(&self, expected: PlayerIdentity) -> bool {
        self.players
            .get(&expected.slot)
            .is_some_and(|current| current.connection == expected.connection)
    }

    pub fn respawn_task(&self, slot: i32, round: RoundGeneration) -> Option<PlayerTask> {
        let player = self.players.get(&slot).copied()?;
        (player.last_spawn.is_some() && !player.pending_pawn && player.pawn_handle == u32::MAX).then_some(PlayerTask { round, player })
    }

    pub fn validates_respawn(&self, task: PlayerTask, round: RoundGeneration) -> bool {
        task.round == round
            && task.player.pawn_handle == u32::MAX
            && self
                .players
                .get(&task.player.slot)
                .is_some_and(|player| *player == task.player && player.last_spawn.is_some() && !player.pending_pawn)
    }

    // The adapter must also resolve this handle and validate it against the live engine pawn.
    pub fn validates(&self, task: PlayerTask, round: RoundGeneration, live_pawn_handle: u32) -> bool {
        task.round == round && task.player.pawn_handle == live_pawn_handle && self.identity(task.player.slot) == Some(task.player)
    }

    pub fn clear(&mut self) {
        self.players.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Capabilities, Config, RoundController};
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn connection_tasks_survive_lives_but_not_disconnect_or_slot_reuse() {
        let mut players = PlayerTracker::default();
        players.connect(0);
        let welcome = players.connection_identity(0).unwrap();
        players.spawn(0, 1, 100);
        assert!(players.validates_connection(welcome));
        players.death(0);
        assert!(players.validates_connection(welcome));
        players.spawn(0, 2, 101);
        assert!(players.validates_connection(welcome));
        players.disconnect(0);
        assert!(!players.validates_connection(welcome));
        players.connect(0);
        players.spawn(0, 3, 100);
        assert!(!players.validates_connection(welcome));
    }

    #[test]
    fn reconnect_respawn_death_and_entity_reuse_reject_old_tasks() {
        let mut players = PlayerTracker::default();
        let round = RoundController::new(None, Capabilities::default()).unwrap().generation();
        assert!(players.connect(0));
        assert!(!players.connect(0));
        assert!(players.identity(0).is_none());
        assert!(players.spawn(0, 0, u32::MAX).is_none());
        let first = PlayerTask {
            round,
            player: players.spawn(0, 1, 0x4001).unwrap(),
        };
        assert!(players.validates(first, round, 0x4001));
        assert!(!players.validates(first, round, 0x8001));
        let second = PlayerTask {
            round,
            player: players.spawn(0, 2, 0x4001).unwrap(),
        };
        assert!(!players.validates(first, round, 0x4001));
        assert!(players.validates(second, round, 0x4001));
        assert!(players.spawn(0, 2, 0x4001).is_none());
        assert!(players.spawn(0, 1, 0x4001).is_none());
        assert!(players.validates(second, round, 0x4001));
        assert!(players.death(0));
        assert!(!players.death(0));
        assert!(!players.validates(second, round, 0x4001));
        assert!(players.disconnect(0));
        assert!(!players.disconnect(0));
        players.connect(0);
        let third = PlayerTask {
            round,
            player: players.spawn(0, 1, 0x4001).unwrap(),
        };
        assert_ne!(third.player, first.player);
        assert!(!players.validates(first, round, 0x4001));
        players.clear();
        players.connect(0);
        assert_ne!(players.spawn(0, 1, 0x4001).unwrap(), third.player);
    }

    #[test]
    fn round_end_disable_and_map_change_invalidate_player_tasks() {
        let config = Config::parse(include_str!("../../../config/funrounds.example.toml")).unwrap();
        let capabilities = Capabilities {
            classic: true,
            equipment: true,
            health: true,
            armor: true,
            gravity: true,
            ..Capabilities::default()
        };
        let mut controller = RoundController::new(Some(config), capabilities).unwrap();
        let mut rng = StdRng::seed_from_u64(0);
        let mut players = PlayerTracker::default();
        players.connect(0);
        let player = players.spawn(0, 1, 0x4001).unwrap();
        for key in 0..3 {
            controller.start_round(controller.round_key(key), false, &mut rng);
            let task = PlayerTask {
                round: controller.generation(),
                player,
            };
            assert!(players.validates(task, controller.generation(), player.pawn_handle));
            match key {
                0 => {
                    controller.end_round(controller.round_key(key));
                }
                1 => {
                    controller.set_enabled(false);
                }
                _ => controller.reset(),
            }
            assert!(!players.validates(task, controller.generation(), player.pawn_handle));
            controller.set_enabled(true);
        }
        assert!(!players.connect(-1));
        assert!(players.spawn(99, 1, 0x4001).is_none());
    }

    #[test]
    fn invalid_pawn_at_a_new_spawn_still_invalidates_the_old_life() {
        let mut players = PlayerTracker::default();
        let round = RoundController::new(None, Capabilities::default()).unwrap().generation();
        players.connect(0);
        let old = PlayerTask {
            round,
            player: players.spawn(0, 1, 0x4001).unwrap(),
        };
        assert!(players.spawn(0, 2, u32::MAX).is_none());
        assert!(players.identity(0).is_none());
        assert!(!players.validates(old, round, 0x4001));
        assert!(players.spawn(0, 1, 0x4001).is_none());
    }

    #[test]
    fn binding_a_pending_pawn_preserves_the_spawn_and_rejects_stale_lives() {
        let mut players = PlayerTracker::default();
        players.connect(0);
        players.spawn(0, 1, u32::MAX);
        let pending = players.pending_identity(0).unwrap();
        assert!(players.bind_spawn(pending, u32::MAX).is_none());
        let bound = players.bind_spawn(pending, 0x4001).unwrap();
        assert_eq!(bound.life, pending.life);
        assert_eq!(bound.last_spawn, pending.last_spawn);
        assert!(players.pending_identity(0).is_none());
        assert!(players.bind_spawn(pending, 0x4001).is_none());
        players.spawn(0, 2, u32::MAX);
        let old_life = players.pending_identity(0).unwrap();
        players.spawn(0, 3, u32::MAX);
        assert!(players.bind_spawn(old_life, 0x4001).is_none());
        let old_connection = players.pending_identity(0).unwrap();
        players.disconnect(0);
        players.connect(0);
        players.spawn(0, 3, u32::MAX);
        assert!(players.bind_spawn(old_connection, 0x4001).is_none());
        let dead = players.pending_identity(0).unwrap();
        assert!(players.death(0));
        assert!(players.bind_spawn(dead, 0x4001).is_none());
        assert!(players.pending_identity(0).is_none());
    }

    #[test]
    fn delayed_respawn_tasks_expire_after_respawn_disconnect_or_round_change() {
        let mut players = PlayerTracker::default();
        let mut rounds = RoundController::new(None, Capabilities::default()).unwrap();
        let round = rounds.generation();
        assert!(players.connect(0));
        assert!(players.respawn_task(0, round).is_none());
        let connected = PlayerTask {
            round,
            player: players.players[&0],
        };
        assert!(!players.validates_respawn(connected, round));
        assert!(players.spawn(0, 1, 0x4001).is_some());
        assert!(players.death(0));
        let task = players.respawn_task(0, round).unwrap();
        assert!(players.validates_respawn(task, round));
        assert!(!players.validates(task, round, u32::MAX));

        players.spawn(0, 2, 0x4001);
        assert!(!players.validates_respawn(task, round));
        assert!(players.death(0));
        let disconnected = players.respawn_task(0, round).unwrap();
        assert!(players.disconnect(0));
        assert!(!players.validates_respawn(disconnected, round));

        players.connect(0);
        players.spawn(0, 1, 0x4001);
        players.death(0);
        let old_round_task = players.respawn_task(0, round).unwrap();
        rounds.reset();
        assert_ne!(rounds.generation(), round);
        assert!(!players.validates_respawn(old_round_task, rounds.generation()));
    }

    #[test]
    fn pending_initial_spawn_cannot_be_mistaken_for_a_dead_players_respawn() {
        let mut players = PlayerTracker::default();
        let round = RoundController::new(None, Capabilities::default()).unwrap().generation();
        players.connect(0);
        assert!(players.spawn(0, 1, u32::MAX).is_none());
        assert!(players.respawn_task(0, round).is_none());
        let pending = PlayerTask {
            round,
            player: players.pending_identity(0).unwrap(),
        };
        assert!(!players.validates_respawn(pending, round));
    }
}
