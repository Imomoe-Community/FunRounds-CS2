// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{EngineAction, Runtime, active_combat_phase, log};
use crate::{
    fault::{Fault, Recovery},
    game::{self, LivePlayer},
    s2sdk,
};
use crate::{process_chicken_blast, process_chicken_eat, process_chicken_purchase, process_chicken_spawn};
use funrounds_core::{Phase, PlayerIdentity, PlayerTask, PlayerTracker, RoundGeneration};
use plugify::Str;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingChickenBlastKill {
    round: RoundGeneration,
    victim: PlayerIdentity,
    tick: i32,
    damage_frame_token: Option<u64>,
    source: crate::chicken_entities::KillWeaponSource,
}

impl PendingChickenBlastKill {
    fn matches(&self, round: RoundGeneration, victim: PlayerIdentity, attacker: PlayerIdentity, tick: i32, frame_token: Option<u64>) -> bool {
        self.round == round
            && self.victim == victim
            && self.source.attacker == attacker
            && self.tick == tick
            && self.damage_frame_token.is_none_or(|expected| frame_token == Some(expected))
    }
}

fn chicken_weapon_source_readback_matches(
    source: &crate::chicken_entities::KillWeaponSource,
    owner: i32,
    inventory: &[i32],
    classname: &str,
    item_definition: u16,
) -> bool {
    owner == source.attacker.pawn_handle as i32
        && inventory.contains(&source.handle)
        && source.classname == classname
        && source.item_definition == item_definition
}

fn capture_chicken_damage_weapon_source(
    damage: crate::chicken_entities::EntityDamageSource,
    players: &PlayerTracker,
    controllers: &HashMap<i32, i32>,
) -> Option<crate::chicken_entities::KillWeaponSource> {
    let item_definition_reader = unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
    for slot in 0..s2sdk::GetMaxClients() {
        let Some(player) = LivePlayer::resolve(slot) else { continue };
        if (s2sdk::GetClientTeam(slot) as i32) < 2
            || player.pawn_handle != damage.attacker
            || controllers.get(&slot) != Some(&player.controller_handle)
        {
            continue;
        }
        let Some(attacker) = players
            .identity(slot)
            .filter(|identity| identity.pawn_handle == player.pawn_handle as u32)
        else {
            continue;
        };
        let inventory = s2sdk::GetClientWeapons(slot);
        for handle in [damage.ability, damage.inflictor] {
            if handle == -1
                || !s2sdk::IsValidEntHandle(handle)
                || s2sdk::GetEntityOwner(handle) != player.pawn_handle
                || !inventory.iter().any(|owned| *owned == handle)
            {
                continue;
            }
            let classname = s2sdk::GetEntityClassname(handle).to_string();
            if !classname.starts_with("weapon_") {
                continue;
            }
            let item_definition = unsafe { item_definition_reader(handle) };
            return Some(crate::chicken_entities::KillWeaponSource {
                attacker,
                handle,
                classname,
                item_definition,
            });
        }
    }
    None
}

pub(super) fn chicken_weapon_source_is_current(
    source: &crate::chicken_entities::KillWeaponSource,
    players: &PlayerTracker,
    controllers: &HashMap<i32, i32>,
) -> bool {
    let Some(player) = LivePlayer::resolve(source.attacker.slot) else {
        return false;
    };
    if players.identity(player.slot) != Some(source.attacker)
        || controllers.get(&player.slot) != Some(&player.controller_handle)
        || player.pawn_handle != source.attacker.pawn_handle as i32
    {
        return false;
    }
    if !s2sdk::IsValidEntHandle(source.handle) {
        return false;
    }
    let owner = s2sdk::GetEntityOwner(source.handle);
    let inventory = s2sdk::GetClientWeapons(player.slot).iter().copied().collect::<Vec<_>>();
    let classname = s2sdk::GetEntityClassname(source.handle).to_string();
    let item_definition_reader = unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
    let item_definition = unsafe { item_definition_reader(source.handle) };
    chicken_weapon_source_readback_matches(source, owner, &inventory, &classname, item_definition)
}

fn use_button_rising_edge(states: &mut HashMap<i32, (PlayerIdentity, bool)>, identity: PlayerIdentity, down: bool) -> bool {
    let rising = down && !states.get(&identity.slot).is_some_and(|(old, was_down)| *old == identity && *was_down);
    if down {
        states.insert(identity.slot, (identity, true));
    } else if states.get(&identity.slot).is_some_and(|(old, _)| *old == identity) {
        states.remove(&identity.slot);
    }
    rising
}

#[derive(Default)]
pub(super) struct State {
    pub(super) entities: crate::chicken_entities::Chickens,
    pub(super) ready: bool,
    follow_block_support: bool,
    pub(super) use_buttons: HashMap<i32, (PlayerIdentity, bool)>,
    pub(super) spawn_round: Option<RoundGeneration>,
    blast_kills: HashMap<i32, PendingChickenBlastKill>,
    pub(super) cleanup: Vec<i32>,
    pub(super) purchases: HashMap<i32, PlayerTask>,
}

impl State {
    pub(super) fn new(ready: bool, follow_block_support: bool) -> Self {
        Self {
            ready,
            follow_block_support,
            ..Self::default()
        }
    }

    pub(super) fn cancel_purchase(&mut self, slot: i32, actions: &mut Vec<EngineAction>) {
        self.purchases.remove(&slot);
        actions.retain(|action| !matches!(action, EngineAction::Chicken(Action::Purchase { task, .. }) if task.player.slot == slot));
    }

    pub(super) fn cancel_purchases_for_slots(&mut self, slots: &[i32]) {
        self.purchases.retain(|slot, _| !slots.contains(slot));
    }

    pub(super) fn forget_input(&mut self, slot: i32) {
        self.use_buttons.remove(&slot);
    }

    pub(super) fn discard_map_state(&mut self) {
        self.entities.discard_map_state();
        self.cleanup.clear();
    }

    fn finish_purchase(&mut self, task: PlayerTask) {
        if self.purchases.get(&task.player.slot) == Some(&task) {
            self.purchases.remove(&task.player.slot);
        }
    }

    fn clear_round_work(&mut self, actions: &mut Vec<EngineAction>) {
        self.use_buttons.clear();
        self.purchases.clear();
        self.blast_kills.clear();
        actions.retain(|action| {
            !matches!(
                action,
                EngineAction::Chicken(Action::Spawn { .. } | Action::Blast { .. } | Action::Purchase { .. } | Action::Eat { .. })
            )
        });
        self.spawn_round = None;
    }

    pub(super) fn clear_pending_chicken_blast_for_life(&mut self, identity: PlayerIdentity) {
        self.blast_kills
            .retain(|slot, pending| *slot != identity.slot && pending.victim != identity && pending.source.attacker != identity);
    }

    pub(super) fn clear_pending_chicken_blast_for_slot(&mut self, slot: i32) {
        self.blast_kills
            .retain(|victim, pending| *victim != slot && pending.source.attacker.slot != slot);
    }

    pub(super) fn expire_chicken_blast_kills(&mut self, tick: i32, round: RoundGeneration, players: &funrounds_core::PlayerTracker) {
        self.blast_kills.retain(|slot, pending| {
            pending.tick == tick
                && pending.round == round
                && players.identity(*slot) == Some(pending.victim)
                && players.identity(pending.source.attacker.slot) == Some(pending.source.attacker)
        });
    }

    pub(super) fn cleanup_complete(&self, round_transition: bool) -> bool {
        (self.entities.is_empty() || (round_transition && self.entities.has_only_active_chickens())) && self.cleanup.is_empty()
    }
}

impl Runtime {
    pub(crate) fn chicken_use_configured(&self) -> bool {
        self.controller
            .config()
            .is_some_and(|config| config.chicken.is_some_and(|chicken| chicken.eating.enabled))
    }

    pub(crate) fn chicken_follow_block_supported(&self) -> bool {
        self.ordinary_chickens.follow_block_support
    }

    /// Following belongs to ordinary chicken ecology, independently from E eating.
    pub(crate) fn chicken_follow_block_desired(&self) -> bool {
        self.controller.enabled() && self.faults.recovery != Recovery::Stopped && self.ordinary_chickens_enabled()
    }

    pub(crate) fn observe_chicken_use_target(&mut self, pawn_handle: i32, target_handle: i32, in_use: bool) {
        let Some(player) = LivePlayer::resolve_pawn_owner(pawn_handle) else {
            return;
        };
        if self.controllers.get(&player.slot) != Some(&player.controller_handle) {
            return;
        }
        let Some(identity) = self
            .players
            .identity(player.slot)
            .filter(|identity| identity.pawn_handle == pawn_handle as u32)
        else {
            return;
        };
        if !in_use {
            use_button_rising_edge(&mut self.ordinary_chickens.use_buttons, identity, false);
            return;
        }
        self.discover_ordinary_chicken(target_handle);
        if !self.use_target_hook_ready
            || !self.ordinary_chickens.ready
            || !self.ordinary_chickens_enabled()
            || !self.controller.enabled()
            || !matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
            || !s2sdk::IsValidEntHandle(target_handle)
            || s2sdk::GetEntityClassname(target_handle).as_str() != "chicken"
            || !self.ordinary_chickens.entities.contains(target_handle, self.controller.generation())
        {
            return;
        }
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return;
        };
        let Some(mode) = self.controller.current_mode() else { return };
        if !config.eating.enabled_for_mode(mode.chicken_eating) {
            return;
        }
        let Some(task) = self.combat_task(player) else { return };
        let current_health = s2sdk::GetClientHealth(player.slot);
        let Some(base_health) = self.chickens.base_health(task).ok() else {
            return;
        };
        // A selector may report no target before finding a chicken during the same press.
        // Only a valid consumable target spends this press; releases are polled independently.
        if self
            .ordinary_chickens
            .use_buttons
            .get(&identity.slot)
            .is_some_and(|(old, down)| *old == identity && *down)
            || !self.ordinary_chickens.entities.mark_consumed(target_handle, task.round)
        {
            return;
        }
        let now = f64::from(s2sdk::GetGameTime());
        let outcome = match self.chickens.consume(task, current_health, base_health, now, config, &mut self.rng) {
            Ok(outcome) => outcome,
            Err(_) => {
                self.ordinary_chickens.entities.release_consumed(target_handle, task.round);
                return;
            }
        };
        use_button_rising_edge(&mut self.ordinary_chickens.use_buttons, identity, true);
        self.mark_chicken_target_claimed(identity, s2sdk::GetGameTickCount(), target_handle);
        self.engine_actions.push(EngineAction::Chicken(Action::Eat {
            task,
            player,
            chicken: target_handle,
            outcome,
        }));
    }

    pub(crate) fn finish_chicken_use(&mut self, round: RoundGeneration, chicken: i32) {
        self.ordinary_chickens.entities.remove_chicken(chicken, round);
        self.public_properties.restore_model_scale(chicken);
    }

    pub(super) fn release_chicken_use_buttons(&mut self) {
        // The native target selector is not called on every released-input frame.
        // Button state 0 is current down state; transition masks must not keep the latch held.
        self.ordinary_chickens.use_buttons.retain(|slot, (identity, _)| {
            self.players.identity(*slot) == Some(*identity)
                && LivePlayer::resolve(*slot).is_some_and(|player| player.pawn_handle as u32 == identity.pawn_handle)
                && s2sdk::GetClientButtons(*slot, 0) & (1 << 5) != 0
        });
    }

    pub(super) fn update_chicken_digestion(&mut self, now: f64) {
        self.chickens.retain_round(self.controller.generation());
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return;
        };
        if !now.is_finite() || now < 0.0 {
            return;
        }
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let Some(task) = self.combat_task(player) else { continue };
            let digestion_needed = !config.eating.enabled || matches!(self.chickens.undigested(task), Ok(count) if count > 0);
            if digestion_needed {
                let _ = self.chickens.digest_due(task, now, config);
            }
        }
    }

    pub fn take_chicken_cleanup(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.ordinary_chickens.cleanup)
    }

    pub fn finish_chicken_purchase(&mut self, task: PlayerTask) {
        self.ordinary_chickens.finish_purchase(task);
    }

    pub(crate) fn chicken_risk_finish_current(&self, task: PlayerTask, player: LivePlayer) -> bool {
        // round_end advances the round generation and removes the chicken. The
        // synchronous risk-blast continuation retains only this exact live eater.
        self.classic_enabled()
            && self.players.identity(player.slot) == Some(task.player)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && LivePlayer::resolve(player.slot) == Some(player)
    }

    pub fn chicken_buyer_identity_current(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.on_game_thread()
            && self.players.validates_connection(task.player)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && LivePlayer::resolve_present(player.slot) == Some(player)
    }

    pub(super) fn ordinary_chickens_enabled(&self) -> bool {
        self.controller.config().is_some_and(|config| config.chicken.is_some())
            && self.controller.current_mode().is_some_and(|mode| mode.ordinary_chickens)
    }

    pub(super) fn cleanup_ordinary_chickens(&mut self) {
        self.clear_chicken_round_work();
        let registered = self.ordinary_chickens.entities.registered_handles();
        let chickens = self.ordinary_chickens.entities.clear();
        for handle in &registered {
            self.public_properties.restore_model_scale(*handle);
        }
        self.ordinary_chickens.cleanup.extend(chickens);
        if self.public_properties.has_pending_restores() {
            self.effects_ready = false;
        }
        crate::CHICKEN_DAMAGE_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
    }

    pub(super) fn clear_chicken_round_work(&mut self) {
        self.chickens.clear_eating();
        self.ordinary_chickens.clear_round_work(&mut self.engine_actions);
    }

    pub(super) fn advance_chicken_round(&mut self, round: RoundGeneration) {
        self.advance_chicken_round_if_alive(round, |handle| {
            if !s2sdk::IsValidEntHandle(handle) || s2sdk::GetEntityClassname(handle).as_str() != "chicken" {
                return false;
            }
            crate::chicken_entities::health(handle).is_some_and(|(health, life_state)| health > 0 && life_state == 0)
        });
    }

    pub(super) fn advance_chicken_round_if_alive(&mut self, round: RoundGeneration, alive: impl FnMut(i32) -> bool) {
        self.ordinary_chickens.blast_kills.retain(|_, pending| pending.round == round);
        let registered = self.ordinary_chickens.entities.registered_handles();
        let stale = self.ordinary_chickens.entities.advance_round(round, alive);
        for handle in registered {
            if !self.ordinary_chickens.entities.contains(handle, round) {
                self.public_properties.restore_model_scale(handle);
            }
        }
        self.ordinary_chickens.cleanup.extend(stale);
        crate::CHICKEN_DAMAGE_ACTIVE.store(
            self.ordinary_chickens.ready && self.controller.enabled() && self.ordinary_chickens_enabled(),
            std::sync::atomic::Ordering::Release,
        );
    }

    pub fn track_chicken(&mut self, round: RoundGeneration, handle: i32) -> bool {
        if !self.ordinary_chickens.ready
            || !self.ordinary_chickens_enabled()
            || self.controller.generation() != round
            || !matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
            || !self.ordinary_chickens.entities.track(handle, round)
        {
            return false;
        }
        self.public_properties.randomize_model_scale(handle, &mut self.rng);
        crate::CHICKEN_DAMAGE_ACTIVE.store(true, std::sync::atomic::Ordering::Release);
        true
    }

    fn discover_ordinary_chickens(&mut self) {
        if !self.ordinary_chickens.ready || !self.controller.enabled() || !self.ordinary_chickens_enabled() {
            return;
        }
        crate::CHICKEN_DAMAGE_ACTIVE.store(true, std::sync::atomic::Ordering::Release);
        let mut handle = -1;
        loop {
            let previous = handle;
            handle = s2sdk::FindEntityByClassname(handle, &Str::from_str("chicken"));
            if handle == -1 {
                break;
            }
            if handle == previous || !s2sdk::IsValidEntHandle(handle) {
                self.handle_fault(Fault::global("chicken enumeration returned a repeated or invalid handle"));
                break;
            }
            self.discover_ordinary_chicken(handle);
        }
    }

    fn discover_ordinary_chicken(&mut self, handle: i32) {
        if !self.ordinary_chickens.ready
            || !self.controller.enabled()
            || !self.ordinary_chickens_enabled()
            || !matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
            || self.bombs.is_carrier_chicken(handle)
            || !s2sdk::IsValidEntHandle(handle)
            || s2sdk::GetEntityClassname(handle).as_str() != "chicken"
        {
            return;
        }
        if self.ordinary_chickens.entities.discover(handle, self.controller.generation()) {
            self.public_properties.randomize_model_scale(handle, &mut self.rng);
            crate::CHICKEN_DAMAGE_ACTIVE.store(true, std::sync::atomic::Ordering::Release);
        }
    }

    pub fn track_chicken_explosion(&mut self, round: RoundGeneration, handle: i32, source: i32) -> bool {
        self.ordinary_chickens_enabled()
            && self.controller.generation() == round
            && matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
            && self.ordinary_chickens.entities.track_explosion(handle, round, source)
    }

    pub fn finish_chicken_explosion(&mut self, round: RoundGeneration, handle: i32) {
        self.ordinary_chickens.entities.remove_explosion(handle, round);
    }

    pub fn chicken_blast_damage(&self, damage: &crate::damage::Damage) -> bool {
        let round = self.controller.generation();
        [damage.inflictor, damage.attacker]
            .into_iter()
            .any(|handle| self.ordinary_chickens.entities.is_chicken_explosion(handle, round))
    }

    pub(crate) fn observe_chicken_blast_player_damage(&mut self, damage: &crate::damage::Damage, players: &game::DamagePlayers) {
        let Some(victim) = players.victim else { return };
        let Some(victim_identity) = self
            .players
            .identity(victim.slot)
            .filter(|identity| identity.pawn_handle == victim.pawn_handle as u32)
        else {
            self.ordinary_chickens.blast_kills.remove(&victim.slot);
            return;
        };
        let round = self.controller.generation();
        if !self.classic_round()
            || !active_combat_phase(self.controller.phase())
            || !self.controller.current_mode().is_some_and(|mode| mode.kill_loadout)
            || !self.chicken_blast_damage(damage)
        {
            self.ordinary_chickens.blast_kills.remove(&victim.slot);
            return;
        }
        let Some(source) = self
            .ordinary_chickens
            .entities
            .blast_kill_weapon_source(&[damage.inflictor, damage.attacker, damage.weapon], round)
        else {
            self.ordinary_chickens.blast_kills.remove(&victim.slot);
            return;
        };
        if !chicken_weapon_source_is_current(&source, &self.players, &self.controllers) {
            self.ordinary_chickens.blast_kills.remove(&victim.slot);
            return;
        }
        self.ordinary_chickens.blast_kills.insert(
            victim.slot,
            PendingChickenBlastKill {
                round,
                victim: victim_identity,
                tick: s2sdk::GetGameTickCount(),
                damage_frame_token: crate::damage::current_damage_frame_token().filter(|token| *token != 0),
                source,
            },
        );
    }

    pub(super) fn take_chicken_blast_kill_weapon(
        &mut self,
        victim_slot: i32,
        attacker_slot: i32,
    ) -> Option<crate::chicken_entities::KillWeaponSource> {
        let pending = self.ordinary_chickens.blast_kills.remove(&victim_slot)?;
        let victim = self.players.identity(victim_slot)?;
        let attacker = self.players.identity(attacker_slot)?;
        let frame = crate::damage::current_damage_frame_token().filter(|token| *token != 0);
        if pending.matches(self.controller.generation(), victim, attacker, s2sdk::GetGameTickCount(), frame)
            && chicken_weapon_source_is_current(&pending.source, &self.players, &self.controllers)
        {
            Some(pending.source)
        } else {
            None
        }
    }

    pub fn chicken_damage_blocked(&mut self, damage: crate::chicken_entities::EntityDamageSource) -> bool {
        self.discover_ordinary_chicken(damage.victim);
        let round = self.controller.generation();
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return false;
        };
        let blocked = self.ordinary_chickens.entities.damage_blocked(damage, round, config.damage_chain);
        if !blocked {
            let players = &self.players;
            let controllers = &self.controllers;
            self.ordinary_chickens
                .entities
                .record_lethal_attacker(damage, round, || capture_chicken_damage_weapon_source(damage, players, controllers));
        }
        blocked
    }

    pub(super) fn queue_chicken_population(&mut self) {
        if !self.ordinary_chickens.ready
            || !self.classic_enabled()
            || !matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
        {
            return;
        }
        if !self.ordinary_chickens_enabled() {
            self.cleanup_ordinary_chickens();
            return;
        }
        if self.ordinary_chickens.spawn_round == Some(self.controller.generation()) {
            return;
        }
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return;
        };
        let mut effective_players = 0_u32;
        let mut positions = Vec::new();
        for slot in 0..s2sdk::GetMaxClients() {
            if !s2sdk::IsClientInGame(slot) || !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
                continue;
            }
            effective_players = effective_players.saturating_add(1);
            if let Some(player) = LivePlayer::resolve(slot) {
                let origin = s2sdk::GetEntityAbsOrigin(player.pawn_handle);
                if origin.x.is_finite() && origin.y.is_finite() && origin.z.is_finite() {
                    positions.push(origin);
                }
            }
        }
        if positions.is_empty() {
            return;
        }
        let round = self.controller.generation();
        self.discover_ordinary_chickens();
        let count = config.spawn.target_count(effective_players);
        let population = self.ordinary_chickens.entities.active_handles(round).len();
        let owned = self.ordinary_chickens.entities.owned_handles(round);
        let maximum = config.spawn.maximum_population(count);
        if population as u64 > u64::from(maximum) {
            let excess = ((population as u64 - u64::from(maximum)) as usize).min(owned.len());
            for handle in owned.into_iter().take(excess) {
                if self.ordinary_chickens.entities.remove_chicken(handle, round) {
                    self.public_properties.restore_model_scale(handle);
                    self.ordinary_chickens.cleanup.push(handle);
                }
            }
            self.ordinary_chickens.spawn_round = Some(round);
            crate::CHICKEN_DAMAGE_ACTIVE.store(
                self.ordinary_chickens.ready && self.controller.enabled() && self.ordinary_chickens_enabled(),
                std::sync::atomic::Ordering::Release,
            );
            log(&format!("chicken population trim round={round:?} maximum={maximum} removed={excess}"));
            return;
        }
        let refill_threshold = config.spawn.refill_threshold(count);
        if population >= refill_threshold as usize {
            self.ordinary_chickens.spawn_round = Some(round);
            return;
        }
        for index in 0..count {
            let player_origin = positions[index as usize % positions.len()];
            let turn = index as f32 * 2.399_963_1;
            let ring = (index as usize / positions.len()) as f32;
            let distance = 64.0 + ring * 24.0;
            self.engine_actions.push(EngineAction::Chicken(Action::Spawn {
                round,
                origin: plugify::Vec3 {
                    x: player_origin.x + turn.cos() * distance,
                    y: player_origin.y + turn.sin() * distance,
                    z: player_origin.z + 4.0,
                },
            }));
        }
        self.ordinary_chickens.spawn_round = Some(round);
        log(&format!(
            "chicken spawn plan round={round:?} count={count} effective_players={effective_players}"
        ));
    }

    pub(super) fn poll_chickens(&mut self) -> Vec<EngineAction> {
        if !self.ordinary_chickens.ready || !matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup) {
            return Vec::new();
        }
        if !self.ordinary_chickens_enabled() {
            self.cleanup_ordinary_chickens();
            return Vec::new();
        }
        self.discover_ordinary_chickens();
        let round = self.controller.generation();
        let Some(config) = self.controller.config().and_then(|config| config.chicken) else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        for handle in self.ordinary_chickens.entities.active_handles(round) {
            let Some((health, life_state)) = crate::chicken_entities::health(handle) else {
                // Removal without a readable dead state is not a kill and must not blast.
                self.ordinary_chickens.entities.remove_chicken(handle, round);
                self.public_properties.restore_model_scale(handle);
                continue;
            };
            let attacker = self.ordinary_chickens.entities.killer(handle, round);
            if (health <= 0 || life_state == 2)
                && let Some(origin) = crate::chicken_entities::position(handle)
                && self.ordinary_chickens.entities.mark_dead(handle, round)
                && let Some(config) = self.ordinary_chickens.entities.take_blast_config(handle, round, config.kill_blast)
            {
                actions.push(EngineAction::Chicken(Action::Blast {
                    round,
                    chicken: handle,
                    attacker,
                    origin,
                    config,
                }));
            }
        }

        actions
    }

    pub(super) fn request_chicken(&mut self, slot: i32) -> Result<String, String> {
        if !self.ordinary_chickens.ready
            || !self.classic_enabled()
            || !active_combat_phase(self.controller.phase())
            || !self.ordinary_chickens_enabled()
        {
            return Err(self.text(slot, "chicken.unavailable", &[]));
        }
        let player = LivePlayer::resolve(slot).ok_or(self.text(slot, "player.alive_required", &[]))?;
        if !matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3) {
            return Err(self.text(slot, "chicken.team_required", &[]));
        }
        let task = self.combat_task(player).ok_or(self.text(slot, "player.life_pending", &[]))?;
        if self.ordinary_chickens.purchases.contains_key(&slot) {
            return Err(self.text(slot, "chicken.pending", &[]));
        }
        let config = self.controller.config().ok_or(self.text(slot, "config.unavailable", &[]))?;
        let price = config.prices.chicken_cash;
        let player_origin = s2sdk::GetEntityAbsOrigin(player.pawn_handle);
        if !player_origin.x.is_finite() || !player_origin.y.is_finite() || !player_origin.z.is_finite() {
            return Err(self.text(slot, "chicken.position_invalid", &[]));
        }
        self.ordinary_chickens.purchases.insert(slot, task);
        self.engine_actions.push(EngineAction::Chicken(Action::Purchase {
            task,
            player,
            price,
            origin: plugify::Vec3 {
                x: player_origin.x + 64.0,
                y: player_origin.y,
                z: player_origin.z + 4.0,
            },
        }));
        Ok(self.text(slot, "chicken.request", &[("price", &price.to_string())]))
    }

    pub(super) fn chicken_spawn_current(&self, round: RoundGeneration) -> bool {
        self.ordinary_chickens.ready
            && self.ordinary_chickens_enabled()
            && self.controller.generation() == round
            && matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
    }

    pub(super) fn chicken_blast_current(&self, round: RoundGeneration, chicken: i32) -> bool {
        self.chicken_spawn_current(round) && self.ordinary_chickens.entities.contains(chicken, round)
    }

    pub(super) fn chicken_purchase_current(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.ordinary_chickens.ready
            && self.ordinary_chickens_enabled()
            && self.classic_enabled()
            && active_combat_phase(self.controller.phase())
            && self.ordinary_chickens.purchases.get(&task.player.slot) == Some(&task)
            && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
            && LivePlayer::resolve(player.slot) == Some(player)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
    }

    pub(super) fn chicken_eat_current(&self, task: PlayerTask, player: LivePlayer, chicken: i32) -> bool {
        self.use_target_hook_ready
            && self.ordinary_chickens.ready
            && self.ordinary_chickens_enabled()
            && self.controller.enabled()
            && matches!(self.controller.phase(), Phase::Preparing | Phase::Running | Phase::Warmup)
            && self.controller.current_mode().is_some_and(|mode| {
                self.controller
                    .config()
                    .and_then(|config| config.chicken)
                    .is_some_and(|config| config.eating.enabled_for_mode(mode.chicken_eating))
            })
            && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
            && LivePlayer::resolve(player.slot) == Some(player)
            && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            && self.ordinary_chickens.entities.contains(chicken, task.round)
    }
}

pub enum Action {
    Spawn {
        round: RoundGeneration,
        origin: plugify::Vec3,
    },
    Blast {
        round: RoundGeneration,
        chicken: i32,
        attacker: Option<i32>,
        origin: plugify::Vec3,
        config: funrounds_core::ChickenBlastConfig,
    },
    Remove(i32),
    Purchase {
        task: PlayerTask,
        player: LivePlayer,
        price: u32,
        origin: plugify::Vec3,
    },
    Eat {
        task: PlayerTask,
        player: LivePlayer,
        chicken: i32,
        outcome: funrounds_core::ChickenEatOutcome,
    },
}

impl Action {
    pub(super) fn involves(&self, slots: &[i32]) -> bool {
        match self {
            Self::Purchase { task, .. } | Self::Eat { task, .. } => slots.contains(&task.player.slot),
            Self::Spawn { .. } | Self::Blast { .. } | Self::Remove(_) => false,
        }
    }
}

impl Runtime {
    pub(super) fn chickens_action_current(&self, action: &Action) -> bool {
        match action {
            Action::Spawn { round, .. } => self.chicken_spawn_current(*round),
            Action::Blast { round, chicken, .. } => self.chicken_blast_current(*round, *chicken),
            Action::Remove(_) => true,
            Action::Purchase { task, player, .. } => self.chicken_purchase_current(*task, *player),
            Action::Eat { task, player, chicken, .. } => self.chicken_eat_current(*task, *player, *chicken),
        }
    }
}

pub(crate) fn execute(action: Action) {
    match action {
        Action::Spawn { round, origin } => process_chicken_spawn(round, origin),
        Action::Blast {
            round,
            chicken,
            attacker,
            origin,
            config,
        } => process_chicken_blast(round, chicken, attacker, origin, config),
        Action::Remove(handle) => crate::chicken_entities::remove(handle),
        Action::Eat {
            task,
            player,
            chicken,
            outcome,
        } => process_chicken_eat(task, player, chicken, outcome),
        Action::Purchase { task, player, price, origin } => process_chicken_purchase(task, player, price, origin),
    }
}
