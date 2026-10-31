// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use funrounds_core::{ChickenConfig, ChickenEatError, ChickenEatOutcome, ChickenEatingState, PlayerTask, RoundGeneration};
use rand::Rng;

#[derive(Clone, Debug)]
struct EatingLife {
    task: PlayerTask,
    base_health: i32,
    stomach: ChickenEatingState,
}

/// Owns the adapter-side, life-bound chicken digestion state.
///
/// Callers must validate `task` against the live engine pawn before passing it here. Spawn
/// events replace stale lives idempotently, and a confirmed BOT takeover moves only the same
/// full pawn's state to its new task identity.
#[derive(Default)]
pub(crate) struct ChickenAdapter {
    lives: HashMap<i32, EatingLife>,
}

impl ChickenAdapter {
    /// Start a life, preserving state if the exact same identity is observed again.
    /// Returns true when a new life replaced the previous entry for this slot.
    pub(crate) fn observe_life(&mut self, task: PlayerTask, base_health: i32) -> bool {
        if self.lives.get(&task.player.slot).is_some_and(|life| life.task == task) {
            return false;
        }
        if base_health <= 0 {
            return false;
        }
        self.lives.insert(
            task.player.slot,
            EatingLife {
                task,
                base_health,
                stomach: ChickenEatingState::new(task),
            },
        );
        true
    }

    /// Move an existing stomach to a confirmed controller identity for the same pawn.
    /// The caller is responsible for validating the adopted identity against PlayerTracker and
    /// the live engine pawn before this method is called.
    pub(crate) fn takeover(&mut self, previous: PlayerTask, adopted: PlayerTask) -> bool {
        let Some(life) = self.lives.get(&previous.player.slot) else {
            return false;
        };
        if life.task != previous {
            return false;
        }
        let base_health = life.base_health;
        let mut stomach = life.stomach.clone();
        if !stomach.rebind_takeover(adopted) {
            return false;
        }

        if previous.player.slot != adopted.player.slot {
            self.lives.remove(&previous.player.slot);
        }
        self.lives.insert(
            adopted.player.slot,
            EatingLife {
                task: adopted,
                base_health,
                stomach,
            },
        );
        true
    }

    /// Adopt a current controller identity when the same full pawn was previously tracked by
    /// another controller. Used only after the player tracker confirms the engine takeover.
    pub(crate) fn takeover_pawn(&mut self, adopted: PlayerTask) -> bool {
        let previous = self
            .lives
            .values()
            .find(|life| {
                life.task.round == adopted.round
                    && life.task.player.slot != adopted.player.slot
                    && life.task.player.pawn_handle == adopted.player.pawn_handle
            })
            .map(|life| life.task);
        previous.is_some_and(|previous| self.takeover(previous, adopted))
    }

    /// Apply one accepted chicken interaction. `task` must be the current validated life.
    pub(crate) fn consume(
        &mut self,
        task: PlayerTask,
        current_health: i32,
        base_health: i32,
        now: f64,
        config: ChickenConfig,
        rng: &mut impl Rng,
    ) -> Result<ChickenEatOutcome, ChickenAdapterError> {
        let life = self.current_life_mut(task)?;
        if !config.eating.enabled {
            life.stomach.reset();
        }
        life.stomach
            .consume(task, current_health, base_health, now, config, rng)
            .map_err(ChickenAdapterError::Eating)
    }

    /// Advance digestion for a current life. Disabling eating clears its stomach immediately.
    pub(crate) fn digest_due(&mut self, task: PlayerTask, now: f64, config: ChickenConfig) -> Result<u32, ChickenAdapterError> {
        let life = self.current_life_mut(task)?;
        if !config.eating.enabled {
            life.stomach.reset();
            return Ok(0);
        }
        life.stomach
            .digest_due(task, now, config.eating.digestion_seconds)
            .map_err(ChickenAdapterError::Eating)
    }

    /// Clear a life only when the caller's identity still owns that slot.
    pub(crate) fn clear_life(&mut self, task: PlayerTask) -> bool {
        if self.lives.get(&task.player.slot).is_some_and(|life| life.task == task) {
            self.lives.remove(&task.player.slot);
            true
        } else {
            false
        }
    }

    pub(crate) fn clear_slot(&mut self, slot: i32) {
        self.lives.remove(&slot);
    }

    /// Reset digestion without discarding the spawn-health baseline shared with healthshots.
    pub(crate) fn clear_eating(&mut self) {
        for life in self.lives.values_mut() {
            life.stomach.reset();
        }
    }

    /// Keep only state from the active round; all older round tasks become invalid together.
    pub(crate) fn retain_round(&mut self, round: RoundGeneration) {
        self.lives.retain(|_, life| life.task.round == round);
    }

    pub(crate) fn undigested(&self, task: PlayerTask) -> Result<u32, ChickenAdapterError> {
        let life = self.current_life(task)?;
        Ok(life.stomach.undigested())
    }

    pub(crate) fn base_health(&self, task: PlayerTask) -> Result<i32, ChickenAdapterError> {
        Ok(self.current_life(task)?.base_health)
    }

    fn current_life(&self, task: PlayerTask) -> Result<&EatingLife, ChickenAdapterError> {
        match self.lives.get(&task.player.slot) {
            Some(life) if life.task == task => Ok(life),
            Some(_) => Err(ChickenAdapterError::StaleLife),
            None => Err(ChickenAdapterError::MissingLife),
        }
    }

    fn current_life_mut(&mut self, task: PlayerTask) -> Result<&mut EatingLife, ChickenAdapterError> {
        match self.lives.get_mut(&task.player.slot) {
            Some(life) if life.task == task => Ok(life),
            Some(_) => Err(ChickenAdapterError::StaleLife),
            None => Err(ChickenAdapterError::MissingLife),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChickenAdapterError {
    MissingLife,
    StaleLife,
    Eating(ChickenEatError),
}
