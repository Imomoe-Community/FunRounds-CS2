// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::log;
use crate::{
    fault::{Fault, Mechanism},
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::{PlayerIdentity, PlayerTask, PlayerTracker, RoundGeneration};
use plugify::Str;
use rand::RngExt;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq)]
struct OwnedFloat {
    previous: f32,
    installed: f32,
}

#[derive(Debug)]
struct WeaponInventory {
    player: LivePlayer,
    identity: PlayerIdentity,
    weapons: HashMap<i32, String>,
}

#[derive(Default)]
pub(super) struct State {
    duck_speed: HashMap<i32, OwnedFloat>,
    weapon_inventory: HashMap<i32, WeaponInventory>,
    model_scales: HashMap<i32, OwnedFloat>,
    model_scale_restore_pending: HashSet<i32>,
}

impl State {
    pub(super) fn update(&mut self, enabled: bool, players: &PlayerTracker, round: RoundGeneration, rng: &mut impl rand::Rng) {
        if enabled {
            self.update_duck_speed();
        }
        self.refresh_inventory(enabled, players, round, rng);
        self.restore_picked_up_weapon_scales();
        self.retry_model_scale_restores();
    }

    pub(super) fn clear_round_inventory(&mut self) {
        self.weapon_inventory.clear();
    }

    pub(super) fn discard_map_state(&mut self) {
        // A new map can reuse complete handles; old snapshots must never restore its entities.
        self.duck_speed.clear();
        self.weapon_inventory.clear();
        self.model_scales.clear();
        self.model_scale_restore_pending.clear();
    }

    pub(super) fn restore_all(&mut self) {
        self.restore_duck_speed();
        self.restore_model_scales();
    }

    pub(super) fn is_restored(&self) -> bool {
        self.duck_speed.is_empty() && self.model_scales.is_empty()
    }

    pub(super) fn has_pending_restores(&self) -> bool {
        !self.model_scale_restore_pending.is_empty()
    }

    pub(super) fn cleanup_complete(&self, enabled: bool) -> bool {
        (enabled || self.is_restored()) && !self.has_pending_restores()
    }

    fn update_duck_speed(&mut self) {
        let players = (0..s2sdk::GetMaxClients())
            .filter_map(|slot| {
                let player = LivePlayer::resolve_present(slot)?;
                duck_speed_update_candidate(player, LivePlayer::resolve(slot), s2sdk::GetClientTeam(slot) as i32).map(|speed| (player, speed))
            })
            .collect::<Vec<_>>();
        let current_handles = players
            .iter()
            .map(|(player, _)| player.pawn_handle)
            .collect::<std::collections::HashSet<_>>();
        for (player, current) in players {
            if let Some(value) = update_owned_minimum(&mut self.duck_speed, player.pawn_handle, current, 8.0)
                && !set_duck_speed_for_handle(player.pawn_handle, value)
            {
                log(&format!("failed to set crouch speed for pawn {}", player.pawn_handle));
            }
        }
        let stale = self
            .duck_speed
            .keys()
            .copied()
            .filter(|handle| !current_handles.contains(handle))
            .collect::<Vec<_>>();
        for handle in stale {
            let Some(owner) = self.duck_speed.get(&handle).copied() else { continue };
            if release_duck_speed(handle, owner) {
                self.duck_speed.remove(&handle);
            }
        }
    }

    fn restore_duck_speed(&mut self) {
        let entries = self.duck_speed.iter().map(|(handle, owner)| (*handle, *owner)).collect::<Vec<_>>();
        for (handle, owner) in entries {
            if release_duck_speed(handle, owner) {
                self.duck_speed.remove(&handle);
            }
        }
    }

    fn weapon_inventory_player_is_current(&self, player: LivePlayer, players: &PlayerTracker, round: RoundGeneration) -> bool {
        let live_identity_matches = players
            .identity(player.slot)
            .is_some_and(|identity| identity.pawn_handle == player.pawn_handle as u32)
            || self.weapon_inventory.get(&player.slot).is_some_and(|previous| {
                previous.player == player
                    && players.validates_damage_source(
                        PlayerTask {
                            round,
                            player: previous.identity,
                        },
                        round,
                        player.pawn_handle as u32,
                    )
            });
        live_identity_matches && player.pawn_controller() == Some(player.controller_handle)
    }

    // Both callers validate the complete player identity before these read-only
    // game-thread calls. An unavailable service is not an empty inventory.
    fn read_client_weapon_inventory(player: LivePlayer) -> Option<Vec<i32>> {
        let class = Str::from_str("CBasePlayerPawn");
        let field = Str::from_str("m_pWeaponServices");
        if s2sdk::GetSchemaOffset(&class, &field) < 0 {
            return None;
        }
        let pawn = s2sdk::EntHandleToEntPointer(player.pawn_handle);
        let services = s2sdk::GetEntSchema2(pawn, &class, &field, 0) as usize;
        (services != 0).then(|| s2sdk::GetClientWeapons(player.slot).iter().copied().collect())
    }

    pub(super) fn refresh_inventory(&mut self, enabled: bool, players: &PlayerTracker, round: RoundGeneration, rng: &mut impl rand::Rng) {
        let scale_drops = enabled && Self::model_scale_imports_available();
        if !scale_drops && self.model_scales.is_empty() && self.model_scale_restore_pending.is_empty() {
            self.weapon_inventory.clear();
            return;
        }
        let max_clients = s2sdk::GetMaxClients();
        self.weapon_inventory.retain(|slot, _| (0..max_clients).contains(slot));
        let mut dropped = Vec::new();
        for slot in 0..max_clients {
            let Some(player) = LivePlayer::resolve_present(slot).filter(|player| self.weapon_inventory_player_is_current(*player, players, round))
            else {
                self.weapon_inventory.remove(&slot);
                continue;
            };
            let Some(mut weapons) = Self::read_client_weapon_inventory(player) else {
                // An unreadable service is not an empty inventory. Keep only the
                // matching life's observation until a ready scan can confirm a drop.
                if self.weapon_inventory.get(&slot).is_some_and(|previous| previous.player != player) {
                    self.weapon_inventory.remove(&slot);
                }
                continue;
            };
            weapons.retain(|handle| s2sdk::IsValidEntHandle(*handle) && s2sdk::GetEntityOwner(*handle) == player.pawn_handle);
            let identity = players
                .identity(slot)
                .or_else(|| self.weapon_inventory.get(&slot).map(|previous| previous.identity));
            let Some(identity) = identity else { continue };
            let inventory = self.weapon_inventory.entry(slot).or_insert_with(|| WeaponInventory {
                player,
                identity,
                weapons: HashMap::new(),
            });
            if inventory.player != player {
                inventory.weapons.clear();
            }
            inventory.player = player;
            inventory.identity = identity;
            // CS2 has no weapon_drop event. Keep classnames for the lifetime of
            // each observed full handle, and recheck identity and owner on removal.
            inventory.weapons.retain(|handle, classname| {
                if weapons.contains(handle) {
                    return true;
                }
                if !scale_drops || !s2sdk::IsValidEntHandle(*handle) || s2sdk::GetEntityClassname(*handle).as_str() != classname {
                    return false;
                }
                match s2sdk::GetEntityOwner(*handle) {
                    -1 => {
                        dropped.push(*handle);
                        false
                    }
                    // Inventory removal can precede ownership release in a callback.
                    owner if owner == player.pawn_handle => true,
                    _ => false,
                }
            });
            for handle in weapons {
                inventory
                    .weapons
                    .entry(handle)
                    .or_insert_with(|| s2sdk::GetEntityClassname(handle).to_string());
            }
        }
        self.model_scales.retain(|handle, _| s2sdk::IsValidEntHandle(*handle));
        self.model_scale_restore_pending.retain(|handle| self.model_scales.contains_key(handle));
        for handle in dropped {
            self.randomize_model_scale(handle, rng);
        }
    }

    pub(super) fn randomize_model_scale(&mut self, handle: i32, rng: &mut impl rand::Rng) {
        if self.model_scales.contains_key(&handle) {
            return;
        }
        let Some(previous) = Self::entity_model_scale(handle) else { return };
        let installed = Self::random_model_scale(rng);
        if Self::set_entity_model_scale(handle, installed) {
            self.model_scales.insert(handle, OwnedFloat { previous, installed });
        }
    }

    pub(super) fn scale_chicken_bomb(&mut self, bomb: i32, chicken: i32, rng: &mut impl rand::Rng) -> Result<(), Fault> {
        if self.model_scales.contains_key(&chicken) || !s2sdk::IsValidEntHandle(bomb) || s2sdk::GetEntityParent(bomb) != chicken {
            return Ok(());
        }
        let (Some(previous), Some(child_local)) = (Self::entity_model_scale(chicken), Self::entity_model_scale(bomb)) else {
            return Ok(());
        };
        // SetParent preserves the C4's world scale via its local scale. The engine
        // propagates the parent's multiplier to both the child and its local offset.
        // Only the parent is written: two writes would square the group multiplier.
        let installed = previous * Self::random_model_scale(rng);
        self.model_scales.insert(chicken, OwnedFloat { previous, installed });
        if !Self::set_entity_model_scale(chicken, installed)
            || !s2sdk::IsValidEntHandle(bomb)
            || s2sdk::GetEntityParent(bomb) != chicken
            || Self::entity_model_scale(bomb).is_none_or(|actual| (actual - child_local).abs() > 0.0001)
        {
            self.restore_model_scale(chicken);
            return Err(Fault::mechanism(Mechanism::ChickenBomb, "chicken/C4 group model scale readback failed"));
        }
        Ok(())
    }

    fn entity_model_scale(handle: i32) -> Option<f32> {
        if !Self::model_scale_imports_available() || !s2sdk::IsValidEntHandle(handle) {
            return None;
        }
        let pointer = s2sdk::EntHandleToEntPointer(handle);
        if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
            return None;
        }
        let scale = s2sdk::GetEntityModelScale(handle);
        scale.is_finite().then_some(scale)
    }

    fn set_entity_model_scale(handle: i32, value: f32) -> bool {
        if !Self::model_scale_imports_available() || !s2sdk::IsValidEntHandle(handle) || !value.is_finite() {
            return false;
        }
        let pointer = s2sdk::EntHandleToEntPointer(handle);
        if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
            return false;
        }
        s2sdk::SetEntityModelScale(handle, value);
        Self::entity_model_scale(handle).is_some_and(|actual| (actual - value).abs() <= 0.0001)
    }

    fn model_scale_imports_available() -> bool {
        let mut probe = crate::capability::Probe::default();
        crate::capability::imports!(probe; __s2sdk_GetEntityModelScale, __s2sdk_SetEntityModelScale);
        probe.available()
    }

    fn random_model_scale(rng: &mut impl rand::Rng) -> f32 {
        match rng.random_range(1..=5) {
            5 => rng.random_range(3.0..=15.0),
            4 => rng.random_range(0.5..=0.75),
            _ => rng.random_range(0.75..=3.0),
        }
    }

    pub(super) fn item_pickup(&mut self, slot: i32, event: usize, players: &PlayerTracker, round: RoundGeneration) {
        let Some(player) = LivePlayer::resolve_present(slot).filter(|player| self.weapon_inventory_player_is_current(*player, players, round)) else {
            return;
        };
        let Some(weapons) = Self::read_client_weapon_inventory(player) else {
            return;
        };
        let item = s2sdk::GetEventString(event, &Str::from_str("item")).to_string();
        for handle in weapons {
            if !self.model_scales.contains_key(&handle) {
                continue;
            }
            if !s2sdk::IsValidEntHandle(handle)
                || s2sdk::GetEntityOwner(handle) != player.pawn_handle
                || !matches_weapon_item(s2sdk::GetEntityClassname(handle).as_str(), &item)
            {
                continue;
            }
            self.restore_model_scale(handle);
        }
    }

    fn restore_picked_up_weapon_scales(&mut self) {
        let picked_up = self
            .model_scales
            .keys()
            .filter_map(|handle| {
                let player_owns_it = self
                    .weapon_inventory
                    .values()
                    .any(|inventory| inventory.weapons.contains_key(handle) && s2sdk::GetEntityOwner(*handle) == inventory.player.pawn_handle);
                player_owns_it.then_some(*handle)
            })
            .collect::<Vec<_>>();
        for handle in picked_up {
            self.restore_model_scale(handle);
        }
    }

    pub(super) fn restore_model_scale(&mut self, handle: i32) -> bool {
        let Some(owner) = self.model_scales.get(&handle).copied() else {
            self.model_scale_restore_pending.remove(&handle);
            return true;
        };
        if release_model_scale(handle, owner) {
            self.model_scales.remove(&handle);
            self.model_scale_restore_pending.remove(&handle);
            true
        } else {
            self.model_scale_restore_pending.insert(handle);
            false
        }
    }

    fn restore_model_scales(&mut self) {
        let entries = self.model_scales.iter().map(|(handle, owner)| (*handle, *owner)).collect::<Vec<_>>();
        for (handle, owner) in entries {
            if release_model_scale(handle, owner) {
                self.model_scales.remove(&handle);
                self.model_scale_restore_pending.remove(&handle);
            } else {
                self.model_scale_restore_pending.insert(handle);
            }
        }
    }

    pub(super) fn retry_model_scale_restores(&mut self) {
        let handles = self.model_scale_restore_pending.iter().copied().collect::<Vec<_>>();
        for handle in handles {
            self.restore_model_scale(handle);
        }
    }
}

fn update_owned_minimum(properties: &mut HashMap<i32, OwnedFloat>, handle: i32, current: f32, installed: f32) -> Option<f32> {
    if !current.is_finite() || !installed.is_finite() {
        return None;
    }
    match properties.get(&handle).copied() {
        Some(_) if (current - installed).abs() <= 0.0001 => None,
        Some(owner) if current < installed => {
            properties.insert(handle, owner);
            Some(installed)
        }
        Some(_) => {
            properties.remove(&handle);
            None
        }
        None if current < installed => {
            properties.insert(
                handle,
                OwnedFloat {
                    previous: current,
                    installed,
                },
            );
            Some(installed)
        }
        None => None,
    }
}

fn owned_float_restore(owner: OwnedFloat, current: f32) -> Option<f32> {
    (current.is_finite() && (current - owner.installed).abs() <= 0.0001).then_some(owner.previous)
}

fn matches_weapon_item(classname: &str, item: &str) -> bool {
    let item = item.trim().to_ascii_lowercase();
    let classname = classname.trim().to_ascii_lowercase();
    !item.is_empty() && (item == classname || format!("weapon_{item}") == classname || format!("weapon_{classname}") == item)
}

fn player_movement_services_for_handle(handle: i32) -> Option<usize> {
    if !s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    let pawn = s2sdk::EntHandleToEntPointer(handle);
    if pawn == 0 || s2sdk::EntPointerToEntHandle(pawn) != handle || s2sdk::GetEntityClassname(handle).as_str() != "player" {
        return None;
    }
    let pawn_class = Str::from_str("CBasePlayerPawn");
    let services_field = Str::from_str("m_pMovementServices");
    let movement_class = Str::from_str("CCSPlayer_MovementServices");
    let duck_field = Str::from_str("m_flDuckSpeed");
    if s2sdk::GetSchemaOffset(&pawn_class, &services_field) < 0 || s2sdk::GetSchemaOffset(&movement_class, &duck_field) < 0 {
        return None;
    }
    let services = s2sdk::GetEntSchema2(pawn, &pawn_class, &services_field, 0) as usize;
    (services != 0).then_some(services)
}

fn duck_speed_for_handle(handle: i32) -> Option<f32> {
    let services = player_movement_services_for_handle(handle)?;
    let speed = s2sdk::GetEntSchemaFloat2(services, &Str::from_str("CCSPlayer_MovementServices"), &Str::from_str("m_flDuckSpeed"), 0) as f32;
    speed.is_finite().then_some(speed)
}

fn set_duck_speed_for_handle(handle: i32, value: f32) -> bool {
    if !value.is_finite() {
        return false;
    }
    let Some(services) = player_movement_services_for_handle(handle) else {
        return false;
    };
    s2sdk::SetEntSchemaFloat2(
        services,
        &Str::from_str("CCSPlayer_MovementServices"),
        &Str::from_str("m_flDuckSpeed"),
        f64::from(value),
        true,
        0,
    );
    duck_speed_for_handle(handle).is_some_and(|actual| (actual - value).abs() <= 0.0001)
}

fn release_duck_speed(handle: i32, owner: OwnedFloat) -> bool {
    if !s2sdk::IsValidEntHandle(handle) {
        return true;
    }
    let Some(current) = duck_speed_for_handle(handle) else { return false };
    let Some(previous) = owned_float_restore(owner, current) else {
        return true;
    };
    set_duck_speed_for_handle(handle, previous)
}

fn duck_speed_update_candidate(player: LivePlayer, active_player: Option<LivePlayer>, team: i32) -> Option<f32> {
    if active_player != Some(player) || !matches!(team, 2 | 3) {
        return None;
    }
    duck_speed_for_handle(player.pawn_handle)
}

fn release_model_scale(handle: i32, owner: OwnedFloat) -> bool {
    if !s2sdk::IsValidEntHandle(handle) {
        return true;
    }
    let Some(current) = State::entity_model_scale(handle) else {
        return false;
    };
    let Some(previous) = owned_float_restore(owner, current) else {
        return true;
    };
    State::set_entity_model_scale(handle, previous)
}
