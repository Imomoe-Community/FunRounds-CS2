// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::PlayerTask;

const FULL_DAMAGE_RADIUS: f32 = 400.0;
const FALLOFF_BASE: f32 = 1000.0;
const PI: f32 = std::f32::consts::PI;

pub fn suicide_blast_damage(victim_health: i32, distance: f32) -> f32 {
    if !distance.is_finite() || distance < 0.0 || victim_health <= 0 {
        return 0.0;
    }
    if distance <= FULL_DAMAGE_RADIUS {
        return victim_health as f32 * 10.0;
    }
    (FALLOFF_BASE - (distance - FULL_DAMAGE_RADIUS) * PI).max(0.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuicideSequence {
    task: PlayerTask,
    due_at: f32,
    due_tick: i32,
    triggered: bool,
    exploded: bool,
}

impl SuicideSequence {
    pub fn begin(task: PlayerTask, due_at: f32, due_tick: i32) -> Option<Self> {
        due_at.is_finite().then_some(Self {
            task,
            due_at,
            due_tick,
            triggered: false,
            exploded: false,
        })
    }

    pub fn task(self) -> PlayerTask {
        self.task
    }

    pub fn retarget(&mut self, task: PlayerTask) -> bool {
        if self.task.round != task.round || self.task.player.pawn_handle != task.player.pawn_handle {
            return false;
        }
        self.task = task;
        true
    }

    pub fn triggered(self) -> bool {
        self.triggered
    }

    pub fn exploded(self) -> bool {
        self.exploded
    }

    pub fn chain_next_tick(&mut self, tick: i32) -> bool {
        if self.exploded || self.triggered || tick < 0 {
            return false;
        }
        let Some(next_tick) = tick.checked_add(1) else {
            return false;
        };
        self.triggered = true;
        self.due_tick = next_tick;
        self.due_at = f32::NEG_INFINITY;
        true
    }

    pub fn take_due(&mut self, tick: i32, now: f32) -> bool {
        if self.exploded || tick < self.due_tick || !now.is_finite() || now < self.due_at {
            return false;
        }
        self.exploded = true;
        true
    }
}
