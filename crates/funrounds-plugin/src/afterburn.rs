// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use funrounds_core::{PlayerTask, RoundGeneration};
use std::collections::HashMap;

use crate::game::LivePlayer;
use crate::s2sdk;
use plugify::Str;

fn attached_effect_handle(pawn: usize) -> i32 {
    s2sdk::GetEntSchemaEnt2(pawn, &Str::from_str("CBaseEntity"), &Str::from_str("m_hEffectEntity"), 0)
}

fn flame_owner_handle(flame: usize) -> i32 {
    s2sdk::GetEntSchemaEnt2(flame, &Str::from_str("CEntityFlame"), &Str::from_str("m_hEntAttached"), 0)
}

pub(crate) fn attached_effect(player: LivePlayer) -> Option<i32> {
    if LivePlayer::resolve(player.slot) != Some(player) {
        return None;
    }
    let handle = player.pawn_handle;
    if !s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    let pawn = s2sdk::EntHandleToEntPointer(handle);
    if pawn == 0 || s2sdk::EntPointerToEntHandle(pawn) != handle {
        return None;
    }
    let effect = attached_effect_handle(pawn);
    (effect != -1 && s2sdk::IsValidEntHandle(effect)).then_some(effect)
}

pub(crate) fn flame_attached_to(player: LivePlayer, flame: i32) -> bool {
    if attached_effect(player) != Some(flame) || !s2sdk::IsValidEntHandle(flame) || s2sdk::GetEntityClassname(flame).as_str() != "entityflame" {
        return false;
    }
    let flame_pointer = s2sdk::EntHandleToEntPointer(flame);
    flame_pointer != 0 && s2sdk::EntPointerToEntHandle(flame_pointer) == flame && flame_owner_handle(flame_pointer) == player.pawn_handle
}

pub(crate) fn remove_flame(handle: i32) {
    if handle != -1 && s2sdk::IsValidEntHandle(handle) && s2sdk::GetEntityClassname(handle).as_str() == "entityflame" {
        s2sdk::RemoveEntity(handle);
    }
}

pub(crate) fn genuine_ground_burn(skip_plugin_rules: bool, damage_type: u32, inflictor_class: &str) -> bool {
    !skip_plugin_rules && damage_type & 8 != 0 && inflictor_class == "inferno"
}

pub(crate) fn flame_tick_damage(skip_plugin_rules: bool, damage_type: u32, inflictor_class: &str) -> bool {
    !skip_plugin_rules && damage_type & 8 != 0 && inflictor_class == "entityflame"
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Life {
    task: PlayerTask,
    player: LivePlayer,
    deadline: f64,
    flame: Option<i32>,
    ignite_pending: bool,
    waiting_external_effect: bool,
}

#[derive(Default)]
pub(crate) struct Afterburns {
    lives: HashMap<i32, Life>,
    removals: Vec<i32>,
}

impl Afterburns {
    pub fn refresh(&mut self, task: PlayerTask, player: LivePlayer, now: f64, duration: f32) -> bool {
        if task.player.slot != player.slot
            || task.player.pawn_handle != player.pawn_handle as u32
            || !now.is_finite()
            || now < 0.0
            || !duration.is_finite()
            || duration <= 0.0
        {
            return false;
        }
        let deadline = now + f64::from(duration);
        if !deadline.is_finite() {
            return false;
        }
        match self.lives.get_mut(&player.slot) {
            Some(life) if life.task == task && life.player == player => life.deadline = deadline,
            _ => {
                self.clear_slot(player.slot);
                self.lives.insert(
                    player.slot,
                    Life {
                        task,
                        player,
                        deadline,
                        flame: None,
                        ignite_pending: false,
                        waiting_external_effect: false,
                    },
                );
            }
        }
        true
    }

    pub fn entries(&self) -> Vec<(PlayerTask, LivePlayer, f64)> {
        self.lives.values().map(|life| (life.task, life.player, life.deadline)).collect()
    }

    pub fn queue_ignite(&mut self, task: PlayerTask, player: LivePlayer) -> bool {
        let Some(life) = self.matching_life_mut(task, player) else {
            return false;
        };
        if life.flame.is_some() || life.ignite_pending || life.waiting_external_effect {
            return false;
        }
        life.ignite_pending = true;
        true
    }

    pub fn begin_ignite(&mut self, task: PlayerTask, player: LivePlayer, now: f64) -> Option<f32> {
        let life = self.matching_life_mut(task, player)?;
        if !life.ignite_pending {
            return None;
        }
        life.ignite_pending = false;
        if !now.is_finite() || now < 0.0 || !life.deadline.is_finite() || life.deadline <= now {
            return None;
        }
        let remaining = life.deadline - now;
        (remaining > 0.0 && remaining <= f64::from(f32::MAX)).then_some(remaining as f32)
    }

    /// Returns true when the caller should queue a new ignition attempt.
    pub fn observe_attachment(&mut self, task: PlayerTask, player: LivePlayer, now: f64, attached: Option<i32>) -> bool {
        let Some(mut life) = self
            .lives
            .get(&player.slot)
            .copied()
            .filter(|life| life.task == task && life.player == player)
        else {
            return false;
        };
        if !now.is_finite() || now < 0.0 || !life.deadline.is_finite() || life.deadline <= now {
            self.clear_life(task);
            return false;
        }
        if attached.is_some() && attached == life.flame {
            life.waiting_external_effect = false;
            self.lives.insert(player.slot, life);
            return false;
        }
        let old_flame = life.flame.take();
        if attached.is_some() {
            life.waiting_external_effect = true;
            self.lives.insert(player.slot, life);
            if let Some(flame) = old_flame {
                self.queue_remove(flame);
            }
            return false;
        }
        life.waiting_external_effect = false;
        let queue = !life.ignite_pending;
        life.ignite_pending = true;
        self.lives.insert(player.slot, life);
        if let Some(flame) = old_flame {
            self.queue_remove(flame);
        }
        queue
    }

    pub fn record_flame(&mut self, task: PlayerTask, player: LivePlayer, flame: i32, now: f64) -> bool {
        let Some(mut life) = self
            .lives
            .get(&player.slot)
            .copied()
            .filter(|life| life.task == task && life.player == player)
        else {
            return false;
        };
        if flame == -1 || !now.is_finite() || life.deadline <= now {
            return false;
        }
        let previous = life.flame.replace(flame).filter(|previous| *previous != flame);
        life.ignite_pending = false;
        life.waiting_external_effect = false;
        self.lives.insert(player.slot, life);
        if let Some(previous) = previous {
            self.queue_remove(previous);
        }
        true
    }

    pub fn owns_flame(&self, task: PlayerTask, player: LivePlayer, flame: i32) -> bool {
        self.lives
            .get(&player.slot)
            .is_some_and(|life| life.task == task && life.player == player && life.flame == Some(flame))
    }

    /// Rebinds a same-pawn, same-round effect across a BOT takeover without extending its deadline.
    pub fn takeover_pawn(&mut self, task: PlayerTask, player: LivePlayer) {
        if task.player.slot != player.slot || task.player.pawn_handle != player.pawn_handle as u32 {
            return;
        }
        let Some(source_slot) = self
            .lives
            .iter()
            .find_map(|(slot, life)| (life.task.round == task.round && life.player.pawn_handle == player.pawn_handle).then_some(*slot))
        else {
            return;
        };
        let Some(life) = self.lives.remove(&source_slot) else { return };
        self.clear_slot(player.slot);
        self.lives.insert(
            player.slot,
            Life {
                task,
                player,
                ignite_pending: false,
                ..life
            },
        );
    }

    pub fn clear_life(&mut self, task: PlayerTask) {
        if self.lives.get(&task.player.slot).is_some_and(|life| life.task == task) {
            self.clear_slot(task.player.slot);
        }
    }

    pub fn clear_round(&mut self, round: RoundGeneration) {
        let slots = self
            .lives
            .iter()
            .filter_map(|(slot, life)| (life.task.round != round).then_some(*slot))
            .collect::<Vec<_>>();
        for slot in slots {
            self.clear_slot(slot);
        }
    }

    pub fn clear_all(&mut self) {
        for slot in self.lives.keys().copied().collect::<Vec<_>>() {
            self.clear_slot(slot);
        }
    }

    pub fn discard_map_state(&mut self) {
        self.lives.clear();
        self.removals.clear();
    }

    pub fn take_removals(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.removals)
    }

    pub fn is_clean(&self) -> bool {
        self.lives.is_empty() && self.removals.is_empty()
    }

    fn matching_life_mut(&mut self, task: PlayerTask, player: LivePlayer) -> Option<&mut Life> {
        self.lives.get_mut(&player.slot).filter(|life| life.task == task && life.player == player)
    }

    fn clear_slot(&mut self, slot: i32) {
        if let Some(life) = self.lives.remove(&slot)
            && let Some(flame) = life.flame
        {
            self.queue_remove(flame);
        }
    }

    fn queue_remove(&mut self, flame: i32) {
        if flame != -1 && !self.removals.contains(&flame) {
            self.removals.push(flame);
        }
    }
}
