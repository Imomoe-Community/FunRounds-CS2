// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    Mode,
    equipment::{WeaponSlot, weapon_slot},
};

impl Mode {
    pub fn classic_chat(&self) -> [String; 5] {
        [
            " \u{4}----------------------------------------".into(),
            format!(" \u{4}{}", self.name),
            format!(" \u{4}回合编号 : {}", self.id),
            format!(" \u{4}{}", self.description),
            " \u{4}----------------------------------------".into(),
        ]
    }

    pub fn classic_console(&self) -> String {
        format!("[FunRounds]回合名称 : {}\n[FunRounds]回合编号 : {}", self.name, self.id)
    }
}

pub fn classic_damage_scale(attacker_health: Option<(i32, u16)>, self_damage: bool, fall: bool, mode65_exception: bool) -> Option<f32> {
    if fall {
        return Some(1.0);
    }
    if self_damage {
        return mode65_exception.then_some(1.0);
    }
    Some(attacker_health.map_or(1.0, |(health, base)| health_scaling(health, base).0))
}

pub fn health_scaling(health: i32, spawn_health: u16) -> (f32, f32) {
    assert!(spawn_health > 0, "spawn health must be validated before computing scaling");
    let percent_numerator = i64::from(health.max(0)) * 100;
    let base = i64::from(spawn_health);
    if percent_numerator < base * 10 {
        (0.6925, 0.385)
    } else if percent_numerator < base * 55 {
        (0.86, 0.68)
    } else if percent_numerator < base * 133 {
        (1.0, 1.0)
    } else {
        let k = (percent_numerator / (base * 33) - 4) as f32;
        ((1.0 + k * 0.02).min(6.0), (1.0 + k * 0.01).min(5.0))
    }
}

pub fn decay_health(health: i32, spawn_health: u16) -> i32 {
    if health <= i32::from(spawn_health) {
        return health;
    }
    ((i64::from(health) * 95 + 99) / 100).max(i64::from(spawn_health)) as i32
}

pub fn self_push_force(weapon: &str) -> Option<f32> {
    match weapon {
        "weapon_nova" | "weapon_mag7" | "weapon_sawedoff" | "weapon_xm1014" | "weapon_ssg08" => Some(700.0),
        "weapon_awp" | "weapon_scar20" | "weapon_g3sg1" => Some(1300.0),
        "weapon_deagle" | "weapon_revolver" => Some(450.0),
        "weapon_taser" => Some(135.0),
        _ if matches!(weapon_slot(weapon), Some(WeaponSlot::Primary | WeaponSlot::Secondary)) => Some(135.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_damage_excludes_self_fall_world_and_low_health_finish_from_scaling() {
        assert_eq!(classic_damage_scale(Some((165, 100)), false, false, false), Some(1.02));
        assert_eq!(classic_damage_scale(Some((9, 100)), false, false, false), Some(0.6925));
        assert_eq!(classic_damage_scale(Some((165, 100)), false, true, false), Some(1.0));
        // Native out-of-world damage can attribute DMG_FALL to the victim itself.
        assert_eq!(classic_damage_scale(Some((165, 100)), true, true, false), Some(1.0));
        assert_eq!(classic_damage_scale(None, false, false, false), Some(1.0));
        assert_eq!(classic_damage_scale(Some((165, 100)), true, false, false), None);
        assert_eq!(classic_damage_scale(Some((165, 100)), true, false, true), Some(1.0));
        // Apply the special-headshot multiplier once, after the common decision allows damage.
        assert_eq!(
            classic_damage_scale(Some((165, 100)), false, false, false).map(|scale| scale * 1.5),
            Some(1.53)
        );
    }

    #[test]
    fn health_scaling_uses_exact_percentage_boundaries_and_caps() {
        for (health, expected) in [
            (0, (0.6925, 0.385)),
            (9, (0.6925, 0.385)),
            (10, (0.86, 0.68)),
            (54, (0.86, 0.68)),
            (55, (1.0, 1.0)),
            (132, (1.0, 1.0)),
            (133, (1.0, 1.0)),
            (164, (1.0, 1.0)),
            (165, (1.02, 1.01)),
            (8382, (6.0, 3.5)),
            (13332, (6.0, 5.0)),
            (i32::MAX, (6.0, 5.0)),
        ] {
            assert_eq!(health_scaling(health, 100), expected, "health={health}");
        }
        // Use the mode's spawn health, including non-100 values.
        assert_eq!(health_scaling(19, 200), (0.6925, 0.385));
        assert_eq!(health_scaling(20, 200), (0.86, 0.68));
        assert_eq!(health_scaling(110, 200), (1.0, 1.0));
        assert_eq!(health_scaling(330, 200), (1.02, 1.01));
    }

    #[test]
    fn decay_rounds_up_without_reviving_or_overflowing() {
        for (health, expected) in [(0, 0), (99, 99), (100, 100), (101, 100), (102, 100), (105, 100), (106, 101), (200, 190)] {
            assert_eq!(decay_health(health, 100), expected);
        }
        assert_eq!(decay_health(201, 200), 200);
        assert_eq!(decay_health(i32::MAX, 100), 2_040_109_465);
    }

    #[test]
    fn self_push_uses_weapon_tiers_and_excludes_non_guns() {
        for weapon in ["weapon_nova", "weapon_mag7", "weapon_sawedoff", "weapon_xm1014", "weapon_ssg08"] {
            assert_eq!(self_push_force(weapon), Some(700.0));
        }
        for weapon in ["weapon_awp", "weapon_scar20", "weapon_g3sg1"] {
            assert_eq!(self_push_force(weapon), Some(1300.0));
        }
        for weapon in ["weapon_deagle", "weapon_revolver"] {
            assert_eq!(self_push_force(weapon), Some(450.0));
        }
        for weapon in ["weapon_ak47", "weapon_m4a1", "weapon_glock", "weapon_taser"] {
            assert_eq!(self_push_force(weapon), Some(135.0));
        }
        for weapon in ["weapon_knife", "weapon_c4", "weapon_hegrenade", "weapon_molotov", "weapon_unknown"] {
            assert_eq!(self_push_force(weapon), None);
        }
    }
}
