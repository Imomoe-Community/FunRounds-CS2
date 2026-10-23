// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use rand::{Rng, RngExt};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RandomDamage {
    pub amount: f32,
    pub attacker_heal: i32,
}

pub fn random_damage(damage: f32, target_health: i32, rng: &mut impl Rng) -> Option<RandomDamage> {
    if !damage.is_finite() || damage <= 0.0 || target_health <= 0 {
        return None;
    }
    let roll = rng.random_range(0..100);
    let value = match roll {
        0..=5 => rng.random_range(1..=10) as f32,
        6..=15 => rng.random_range(0.0..=0.8),
        16..=75 => rng.random_range(0.8..=2.4),
        76..=85 => rng.random_range(2.4..=5.0),
        86..=90 => rng.random_range(5.0..=20.0),
        91..=97 => rng.random_range(0.33..=0.99),
        _ => 0.0,
    };
    random_damage_for_roll(damage, target_health, roll, value)
}

fn random_damage_for_roll(damage: f32, target_health: i32, roll: u8, value: f32) -> Option<RandomDamage> {
    let health = target_health as f32;
    let (amount, attacker_heal) = match roll {
        0..=5 => {
            let heal = (damage * value).floor().min(i32::MAX as f32) as i32;
            (damage, heal)
        }
        6..=15 => (damage * value, 0),
        16..=75 => (damage * value, 0),
        76..=85 => (damage * value, 0),
        86..=90 => (damage * value, 0),
        91..=97 => (damage * health * value, 0),
        98..=99 => (health * 10.0, 0),
        _ => return None,
    };
    amount.is_finite().then_some(RandomDamage { amount, attacker_heal })
}
