// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{EngineAction, Runtime, active_combat_phase};
use crate::process_afterburn_ignite;
use crate::{
    fault::{Fault, Mechanism},
    game::{self, LivePlayer},
    s2sdk,
};
use funrounds_core::PlayerTask;

impl Runtime {
    pub(super) fn managed_burn_enabled(&self) -> bool {
        self.classic_enabled()
            && active_combat_phase(self.controller.phase())
            && self.faults.capabilities().afterburn
            && self
                .controller
                .current_mode()
                .is_some_and(|mode| mode.afterburn.is_some() || mode.hot_floor.is_some())
    }

    pub(super) fn afterburn_settings(&self) -> Option<funrounds_core::AfterburnSettings> {
        (self.classic_enabled() && active_combat_phase(self.controller.phase()) && self.faults.capabilities().afterburn)
            .then(|| self.controller.current_mode().and_then(|mode| mode.afterburn))
            .flatten()
    }

    pub(crate) fn afterburn_action_valid(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.managed_burn_enabled()
            && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
            && LivePlayer::resolve(player.slot) == Some(player)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && self.afterburns.is_active(task, player, f64::from(s2sdk::GetGameTime()))
    }

    pub(crate) fn begin_afterburn_ignite(&mut self, task: PlayerTask, player: LivePlayer, now: f64) -> Option<f32> {
        self.afterburn_action_valid(task, player)
            .then(|| self.afterburns.begin_ignite(task, player, now))
            .flatten()
    }

    pub(crate) fn afterburn_attachment_seen(&mut self, task: PlayerTask, player: LivePlayer, now: f64, attached: Option<i32>) {
        let _ = self.afterburns.observe_attachment(task, player, now, attached);
    }

    pub(crate) fn record_afterburn_flame(&mut self, task: PlayerTask, player: LivePlayer, flame: i32, now: f64) -> bool {
        if !self.afterburn_action_valid(task, player) || !self.afterburns.record_flame(task, player, flame, now) {
            return false;
        }
        if self.afterburns.hot_floor_active(task, player, now) {
            self.record_regeneration_combat_for(task, now);
        }
        true
    }

    pub(crate) fn afterburn_ignition_failed(&mut self, message: impl Into<String>) {
        self.handle_fault(Fault::mechanism(Mechanism::Afterburn, message));
    }

    pub(crate) fn take_afterburn_cleanup(&mut self) -> Vec<i32> {
        self.afterburns.take_removals()
    }

    pub(crate) fn queue_afterburn_ground_tick(&mut self, damage: &crate::damage::Damage, players: &game::DamagePlayers) {
        if damage.inflictor == -1 || !s2sdk::IsValidEntHandle(damage.inflictor) {
            return;
        }
        let (Some(settings), Some(player)) = (self.afterburn_settings(), players.victim) else {
            return;
        };
        let Some(task) = self.combat_task(player) else { return };
        let now = f64::from(s2sdk::GetGameTime());
        if crate::afterburn::genuine_ground_burn(
            damage.skip_plugin_rules,
            damage.damage_type,
            s2sdk::GetEntityClassname(damage.inflictor).as_str(),
        ) && self.afterburns.refresh_source(task, player, now, settings, false)
            && self.afterburns.queue_ignite(task, player)
        {
            self.engine_actions.push(EngineAction::Afterburn(Action::Ignite { task, player }));
        }
    }

    pub(crate) fn afterburn_flame_damage(&self, damage: &crate::damage::Damage, players: &game::DamagePlayers) -> Option<f32> {
        if damage.amount <= 0.0
            || !damage.amount.is_finite()
            || damage.inflictor == -1
            || !s2sdk::IsValidEntHandle(damage.inflictor)
            || !crate::afterburn::flame_tick_damage(
                damage.skip_plugin_rules,
                damage.damage_type,
                s2sdk::GetEntityClassname(damage.inflictor).as_str(),
            )
        {
            return None;
        }
        let player = players.victim?;
        if !self.managed_burn_enabled() {
            return None;
        }
        let task = self.combat_task(player)?;
        self.afterburns
            .owns_flame(task, player, damage.inflictor)
            .then(|| crate::afterburn::flame_attached_to(player, damage.inflictor))
            .filter(|attached| *attached)
            .and_then(|_| {
                self.afterburns
                    .tick_damage(task, player, f64::from(s2sdk::GetGameTime()), s2sdk::GetClientHealth(player.slot))
            })
    }

    pub(super) fn update_afterburns(&mut self, now: f64) {
        self.afterburns.clear_round(self.controller.generation());
        let entries = self.afterburns.entries();
        for (task, player, _) in entries {
            if !self.managed_burn_enabled()
                || !self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
                || LivePlayer::resolve(player.slot) != Some(player)
                || self.controllers.get(&player.slot) != Some(&player.controller_handle)
            {
                self.afterburns.clear_life(task);
                continue;
            }
            let attached = crate::afterburn::attached_effect(player);
            if self.afterburns.observe_attachment(task, player, now, attached) {
                self.engine_actions.push(EngineAction::Afterburn(Action::Ignite { task, player }));
            }
        }
    }
}

pub enum Action {
    Ignite { task: PlayerTask, player: LivePlayer },
    Remove(i32),
}

impl Action {
    pub(super) fn involves(&self, slots: &[i32]) -> bool {
        match self {
            Self::Ignite { task, .. } => slots.contains(&task.player.slot),
            Self::Remove(_) => false,
        }
    }
}

impl Runtime {
    pub(super) fn afterburn_action_current(&self, action: &Action) -> bool {
        match action {
            Action::Ignite { task, player } => self.afterburn_action_valid(*task, *player),
            Action::Remove(_) => true,
        }
    }
}

pub(crate) fn execute(action: Action) {
    match action {
        Action::Remove(handle) => crate::afterburn::remove_flame(handle),
        Action::Ignite { task, player } => process_afterburn_ignite(task, player),
    }
}
