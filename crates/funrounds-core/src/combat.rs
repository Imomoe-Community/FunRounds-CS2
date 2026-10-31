// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::Mode;

impl Mode {
    pub fn native_self_damage_enabled(&self) -> bool {
        matches!(self.self_damage, Some(crate::SelfDamage::Native { amount }) if amount > 0.0)
    }

    pub fn self_damage_finish_enabled(&self) -> bool {
        matches!(self.self_damage, Some(crate::SelfDamage::MissPenalty)) || self.native_self_damage_enabled()
    }
}

pub fn weapon_damage_scale(mode: &Mode, weapon: Option<&str>, player_attack: bool) -> f32 {
    if !player_attack {
        return 1.0;
    }
    match weapon {
        Some("weapon_taser") => mode.weapon_damage.taser,
        Some("weapon_knife" | "weapon_knife_t") => mode.weapon_damage.knife,
        _ => 1.0,
    }
}

#[derive(Debug)]
pub struct CombatLife {
    attributes: crate::LifeAttributes,
    shield: Option<u64>,
    protected_until: Option<f32>,
    next_recharge: f32,
}

impl CombatLife {
    pub fn new(mode: &Mode, now: f32, rng: &mut impl rand::Rng) -> Self {
        let attributes = mode.sample_attributes(rng);
        Self {
            attributes,
            shield: mode.energy_shield.then_some(u64::from(attributes.armor)),
            protected_until: mode.spawn_protection.then_some(now + 0.5),
            next_recharge: now + 0.5,
        }
    }

    pub fn attributes(&self) -> crate::LifeAttributes {
        self.attributes
    }

    pub fn protect(&mut self, now: f32) {
        self.protected_until = Some(now + 0.5);
    }

    pub fn protected(&self, now: f32) -> bool {
        self.protected_until.is_some_and(|until| now < until)
    }

    pub fn shield(&self) -> Option<u64> {
        self.shield
    }

    pub fn armor_hud(&self) -> Option<i32> {
        self.shield.map(|value| value.min(255) as i32)
    }

    pub fn lose_native_armor(&mut self, amount: i32) {
        if amount <= 0 {
            return;
        }
        if let Some(shield) = &mut self.shield {
            *shield = shield.saturating_sub(amount as u64);
        }
    }

    pub fn absorb(&mut self, damage: f32) -> bool {
        let Some(shield) = self.shield.as_mut().filter(|shield| **shield > 0) else {
            return false;
        };
        *shield = shield.saturating_sub((f64::from(damage) / 1.5).ceil() as u64);
        true
    }

    pub fn recharge(&mut self, now: f32, health: i32) -> Option<i32> {
        let base = self.attributes.health;
        let shield = self.shield.as_mut().filter(|shield| **shield > 0)?;
        if now < self.next_recharge {
            return None;
        }
        self.next_recharge = now + 0.5;
        let missing = (i64::from(base) - i64::from(health)).max(0) as u64;
        // Legacy negative writes leave the shield unchanged; an exact zero stops charging.
        if let Some(next) = shield.checked_add(1).and_then(|value| value.checked_sub(missing)) {
            *shield = next;
        }
        Some(health.max(i32::from(base)))
    }
}

// Self damage can consume a shield without entering PvP reflection or knife rules.
pub fn combat_hit(mode: &Mode, life: &mut CombatLife, eligible: bool, allowed_self: bool, knife: bool, incoming: f32) -> (bool, Option<f32>) {
    if !(eligible || allowed_self) {
        return (false, None);
    }
    if eligible && knife && mode.weapon_damage.knife == 0.0 {
        return (true, None);
    }
    if life.absorb(incoming) {
        return (true, None);
    }
    if !eligible {
        return (false, None);
    }
    let reflected = mode.reflect_damage.map(|ratio| incoming * ratio).filter(|damage| *damage > 0.0);
    (false, reflected)
}
