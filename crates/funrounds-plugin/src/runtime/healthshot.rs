// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{EngineAction, Runtime, active_combat_phase, log};
use crate::{execute_valid_action, fault, process_healthshot_purchase};
use crate::{
    fault::{Fault, Mechanism},
    game::LivePlayer,
    parameters::Parameters,
    s2sdk,
};
use funrounds_core::PlayerTask;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    parameters: Parameters,
    hooks_requested: bool,
    hooks_ready: bool,
    override_active: bool,
    faulted: bool,
    purchases: HashMap<i32, PlayerTask>,
}

impl State {
    pub(super) fn cancel_purchase(&mut self, slot: i32, actions: &mut Vec<EngineAction>) {
        self.purchases.remove(&slot);
        actions.retain(|action| !matches!(action, EngineAction::Healthshot(Action::Purchase { task, .. }) if task.player.slot == slot));
    }

    pub(super) fn cancel_purchases_for_slots(&mut self, slots: &[i32]) {
        self.purchases.retain(|slot, _| !slots.contains(slot));
    }

    fn finish_purchase(&mut self, task: PlayerTask) {
        if self.purchases.get(&task.player.slot) == Some(&task) {
            self.purchases.remove(&task.player.slot);
        }
    }

    pub(super) fn clear_round(&mut self) {
        self.purchases.clear();
    }

    pub(super) fn stop_hooks(&mut self) {
        self.hooks_ready = false;
    }

    pub(super) fn restore(&mut self) -> Result<(), Fault> {
        let result = self.parameters.set_healthshot_override(false);
        self.override_active = false;
        result
    }

    pub(super) fn settled(&self, wanted: bool) -> bool {
        // The ConVar lease lasts across rounds; only an unwanted lease must be fully restored.
        override_settled(wanted, self.override_active, self.parameters.is_restored())
    }
}

fn override_settled(wanted: bool, active: bool, parameters_restored: bool) -> bool {
    wanted == active && (wanted || parameters_restored)
}

impl Runtime {
    pub(super) fn take_healthshot_hook_request(&mut self) -> bool {
        let requested = self.healthshot_configured() && !self.healthshot.hooks_requested;
        self.healthshot.hooks_requested |= requested;
        requested
    }

    fn healthshot_configured(&self) -> bool {
        self.controller.config().is_some_and(|config| config.healthshot.is_some())
    }

    pub(crate) fn healthshot_hook_ready(&mut self) {
        self.healthshot.hooks_ready = true;
    }

    pub(crate) fn healthshot_hook_failure(&mut self, reason: impl Into<String>) {
        self.healthshot.hooks_ready = false;
        self.healthshot.faulted = true;
        let reason = reason.into();
        self.faults.exclude(Mechanism::Healthshot, reason.clone());
        // Healthshot is an optional global service, not a round-selection requirement.
        // Do not clear an unrelated active round when lazy registration fails.
        self.capability_reasons.push(format!("healthshot:[{reason}]"));
        log(&format!("healthshot capability disabled: {reason}"));
    }

    pub(super) fn healthshot_active(&self) -> bool {
        self.healthshot.hooks_ready
            && !self.healthshot.faulted
            && self.controller.enabled()
            && self.healthshot_configured()
            && self.faults.capabilities().healthshot
    }

    pub(super) fn sync_healthshot_override(&mut self) -> Result<(), Fault> {
        let wanted = self.healthshot_active();
        if self.healthshot.settled(wanted) {
            return Ok(());
        }
        self.healthshot.parameters.set_healthshot_override(wanted)?;
        self.healthshot.override_active = wanted;
        if !wanted {
            self.healthshot.purchases.clear();
            self.engine_actions
                .retain(|action| !matches!(action, EngineAction::Healthshot(Action::Purchase { .. })));
        }
        Ok(())
    }

    pub(crate) fn reject_healthshot_use(&self, pawn_handle: i32, weapon_handle: i32) -> bool {
        if !self.healthshot_active() || !self.healthshot.override_active {
            return false;
        }
        let Some(player) = LivePlayer::resolve_pawn_owner(pawn_handle) else {
            return true;
        };
        if !s2sdk::IsValidEntHandle(weapon_handle)
            || s2sdk::GetEntityClassname(weapon_handle).as_str() != "weapon_healthshot"
            || !s2sdk::GetClientWeapons(player.slot).iter().any(|handle| *handle == weapon_handle)
        {
            return true;
        }
        let Some(task) = self.combat_task(player) else { return true };
        let Some(base_health) = self.chickens.base_health(task).ok() else {
            return true;
        };
        let Some(config) = self.controller.config().and_then(|config| config.healthshot) else {
            return false;
        };
        matches!(
            funrounds_core::use_healthshot(config, s2sdk::GetClientHealth(player.slot), base_health),
            funrounds_core::HealthshotOutcome::NotConsumed
        )
    }

    pub(crate) fn queue_healthshot_use(&mut self, pawn_handle: i32, weapon_handle: i32) {
        if !self.healthshot_active() || !self.healthshot.override_active {
            return;
        }
        let Some(player) = LivePlayer::resolve_pawn_owner(pawn_handle) else {
            return;
        };
        if !s2sdk::IsValidEntHandle(weapon_handle)
            || s2sdk::GetEntityClassname(weapon_handle).as_str() != "weapon_healthshot"
            || !s2sdk::GetClientWeapons(player.slot).iter().any(|handle| *handle == weapon_handle)
        {
            return;
        }
        let Some(task) = self.combat_task(player) else { return };
        let Some(base_health) = self.chickens.base_health(task).ok() else {
            return;
        };
        let Some(config) = self.controller.config().and_then(|config| config.healthshot) else {
            return;
        };
        if matches!(
            funrounds_core::use_healthshot(config, s2sdk::GetClientHealth(player.slot), base_health),
            funrounds_core::HealthshotOutcome::Applied { .. }
        ) {
            self.engine_actions.push(EngineAction::Healthshot(Action::Use {
                task,
                player,
                base_health,
                config,
            }));
        }
    }

    pub(super) fn request_healthshot(&mut self, slot: i32) -> Result<String, String> {
        if !self.healthshot_active() || !self.healthshot.override_active || !active_combat_phase(self.controller.phase()) {
            return Err(self.text(slot, "healthshot.unavailable", &[]));
        }
        let player = LivePlayer::resolve(slot).ok_or(self.text(slot, "player.alive_required", &[]))?;
        if !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
            return Err(self.text(slot, "healthshot.team_required", &[]));
        }
        if Self::owns_healthshot(player) {
            return Err(self.text(slot, "healthshot.already_owned", &[]));
        }
        let task = self.combat_task(player).ok_or(self.text(slot, "player.life_pending", &[]))?;
        if self.healthshot.purchases.contains_key(&slot) {
            return Err(self.text(slot, "healthshot.pending", &[]));
        }
        let price = self
            .controller
            .config()
            .ok_or(self.text(slot, "config.unavailable", &[]))?
            .prices
            .healthshot_cash;
        self.healthshot.purchases.insert(slot, task);
        self.engine_actions
            .push(EngineAction::Healthshot(Action::Purchase { task, player, price }));
        Ok(self.text(slot, "healthshot.request", &[("price", &price.to_string())]))
    }

    pub(super) fn owns_healthshot(player: LivePlayer) -> bool {
        s2sdk::GetClientWeapons(player.slot)
            .iter()
            .copied()
            .any(|handle| s2sdk::IsValidEntHandle(handle) && s2sdk::GetEntityClassname(handle).as_str() == "weapon_healthshot")
    }

    pub(crate) fn healthshot_buyer_current(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.healthshot_active()
            && self.healthshot.override_active
            && active_combat_phase(self.controller.phase())
            && self.healthshot.purchases.get(&player.slot) == Some(&task)
            && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && LivePlayer::resolve(player.slot) == Some(player)
    }

    pub(crate) fn finish_healthshot_purchase(&mut self, task: PlayerTask) {
        self.healthshot.finish_purchase(task);
    }

    pub(super) fn update_healthshot(&mut self) {
        if let Err(error) = self.sync_healthshot_override() {
            self.healthshot.faulted = true;
            let message = error.to_string();
            // Healthshot is not a round-selection dependency; leave the active round intact.
            self.faults.exclude(Mechanism::Healthshot, message.clone());
            log(&format!("healthshot capability disabled: {message}"));
        }
    }
}

pub enum Action {
    Use {
        task: PlayerTask,
        player: LivePlayer,
        base_health: i32,
        config: funrounds_core::HealthshotConfig,
    },
    Purchase {
        task: PlayerTask,
        player: LivePlayer,
        price: u32,
    },
}

impl Action {
    pub(super) fn involves(&self, slots: &[i32]) -> bool {
        match self {
            Self::Use { task, .. } | Self::Purchase { task, .. } => slots.contains(&task.player.slot),
        }
    }
}

impl Runtime {
    pub(super) fn healthshot_action_current(&self, action: &Action) -> bool {
        match action {
            Action::Use { task, player, .. } => {
                self.healthshot_active()
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            Action::Purchase { task, player, .. } => self.healthshot_buyer_current(*task, *player) && !Self::owns_healthshot(*player),
        }
    }
}

pub(crate) fn execute(action: Action) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        Action::Use {
            task,
            player,
            base_health,
            config,
        } => {
            let action = EngineAction::Healthshot(Action::Use {
                task,
                player,
                base_health,
                config,
            });
            execute_valid_action(action, || {
                let current = s2sdk::GetClientHealth(task.player.slot);
                if let funrounds_core::HealthshotOutcome::Applied { health, .. } = funrounds_core::use_healthshot(config, current, base_health) {
                    s2sdk::SetClientHealth(task.player.slot, health);
                    if s2sdk::GetClientHealth(task.player.slot) != health {
                        return Err(fault::Fault::mechanism(
                            fault::Mechanism::Healthshot,
                            format!("healthshot health write failed for slot {}", task.player.slot),
                        ));
                    }
                }
                Ok(())
            })?;
        }
        Action::Purchase { task, player, price } => process_healthshot_purchase(task, player, price),
    }
    Ok(())
}
