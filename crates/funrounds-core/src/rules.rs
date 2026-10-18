// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

pub const LIFESTEAL_MAX_HEALTH: i32 = 214_748_363;
pub const DODGEBALL_DAMAGE_MULTIPLIER: f32 = 2.0;
pub const THROWING_KNIFE_BODY_DAMAGE: f32 = 80.0;
pub const THROWING_KNIFE_HEAD_DAMAGE: f32 = 250.0;
pub const THROWING_KNIFE_SPEED: f32 = 3500.0;
pub const THROWING_KNIFE_GRAVITY: f32 = 0.85;
pub const THROWING_KNIFE_ELASTICITY: f32 = 0.2;
pub const THROWING_KNIFE_LIFETIME: f32 = 2.5;
pub const THROWING_KNIFE_HEADSHOT_DISTANCE: f32 = 18.0;

pub fn dodgeball_damage(current_health: i32) -> f32 {
    current_health.max(0) as f32 * DODGEBALL_DAMAGE_MULTIPLIER
}

pub fn lifesteal_recovery(health: i32, armor: i32, health_damage: i32, armor_damage: i32) -> (i32, i32) {
    (
        health.max(0).saturating_add(health_damage.max(0)).min(LIFESTEAL_MAX_HEALTH),
        armor.max(0).saturating_add(armor_damage.max(0)),
    )
}

pub fn low_health_finish(health: i32) -> bool {
    health <= 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamResult {
    pub winner: Option<i32>,
    pub scores: [u32; 2],
}

pub struct TeamDeathmatch {
    started_at: f32,
    duration: f32,
    scores: [u32; 2],
    settled: bool,
}

impl TeamDeathmatch {
    pub fn start(now: f32, duration: f32) -> Result<Self, String> {
        if !now.is_finite() || !duration.is_finite() || now < 0.0 || duration <= 0.0 || !(now + duration).is_finite() {
            return Err("team deathmatch needs a finite non-negative start and positive duration".into());
        }
        Ok(Self {
            started_at: now,
            duration,
            scores: [0; 2],
            settled: false,
        })
    }

    pub fn record_kill(&mut self, now: f32, attacker_team: i32, victim_team: i32, distinct: bool) -> bool {
        if self.settled
            || !now.is_finite()
            || now < self.started_at
            || !distinct
            || attacker_team == victim_team
            || now >= self.started_at + self.duration
        {
            return false;
        }
        let Some(attacker_index) = team_index(attacker_team) else {
            return false;
        };
        if team_index(victim_team).is_none() {
            return false;
        }
        self.scores[attacker_index] = self.scores[attacker_index].saturating_add(1);
        true
    }

    pub fn finish(&mut self, now: f32) -> Option<TeamResult> {
        if self.settled || !now.is_finite() || now < self.started_at + self.duration {
            return None;
        }
        self.settled = true;
        Some(TeamResult {
            winner: match self.scores[0].cmp(&self.scores[1]) {
                std::cmp::Ordering::Greater => Some(2),
                std::cmp::Ordering::Less => Some(3),
                std::cmp::Ordering::Equal => None,
            },
            scores: self.scores,
        })
    }

    pub fn scores(&self) -> [u32; 2] {
        self.scores
    }

    pub fn remaining(&self, now: f32) -> f32 {
        if !now.is_finite() {
            return 0.0;
        }
        self.duration - (now - self.started_at).clamp(0.0, self.duration)
    }
}

fn team_index(team: i32) -> Option<usize> {
    match team {
        2 => Some(0),
        3 => Some(1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifesteal_saturates_without_exceeding_the_legacy_health_cap() {
        assert_eq!(lifesteal_recovery(100, 20, 12, 7), (112, 27));
        assert_eq!(lifesteal_recovery(LIFESTEAL_MAX_HEALTH - 2, 254, 10, 1), (LIFESTEAL_MAX_HEALTH, 255));
        assert_eq!(
            lifesteal_recovery(i32::MAX, i32::MAX, i32::MAX, i32::MAX),
            (LIFESTEAL_MAX_HEALTH, i32::MAX)
        );
        assert_eq!(lifesteal_recovery(100, 20, -1, -1), (100, 20));
    }

    #[test]
    fn dodgeball_uses_twice_the_target_current_health() {
        assert_eq!(dodgeball_damage(25), 50.0);
        assert_eq!(dodgeball_damage(1), 2.0);
        assert_eq!(dodgeball_damage(0), 0.0);
    }

    #[test]
    fn throwing_knife_values_match_the_legacy_mode() {
        assert_eq!(THROWING_KNIFE_BODY_DAMAGE, 80.0);
        assert_eq!(THROWING_KNIFE_HEAD_DAMAGE, 250.0);
        assert_eq!(THROWING_KNIFE_SPEED, 3500.0);
        assert_eq!(THROWING_KNIFE_GRAVITY, 0.85);
        assert_eq!(THROWING_KNIFE_ELASTICITY, 0.2);
        assert_eq!(THROWING_KNIFE_LIFETIME, 2.5);
        assert_eq!(THROWING_KNIFE_HEADSHOT_DISTANCE, 18.0);
    }

    #[test]
    fn low_health_finish_matches_the_legacy_one_health_cleanup_threshold() {
        assert!(low_health_finish(1));
        assert!(low_health_finish(0));
        assert!(!low_health_finish(2));
    }

    #[test]
    fn team_deathmatch_counts_only_distinct_enemy_kills_before_the_deadline() {
        let mut game = TeamDeathmatch::start(10.0, 30.0).unwrap();
        assert_eq!(game.remaining(15.0), 25.0);
        assert!(!game.record_kill(9.99, 2, 3, true));
        assert!(game.record_kill(15.0, 2, 3, true));
        assert!(game.record_kill(20.0, 3, 2, true));
        assert!(!game.record_kill(20.0, 2, 2, true));
        assert!(!game.record_kill(20.0, 2, 3, false));
        assert!(!game.record_kill(20.0, 1, 3, true));
        assert!(!game.record_kill(40.0, 2, 3, true));
        assert_eq!(game.scores(), [1, 1]);
        assert_eq!(game.finish(39.99), None);
        assert_eq!(
            game.finish(40.0),
            Some(TeamResult {
                winner: None,
                scores: [1, 1]
            })
        );
        assert_eq!(game.finish(41.0), None);
        assert!(!game.record_kill(21.0, 2, 3, true));
    }

    #[test]
    fn team_deathmatch_reports_winner_and_rejects_invalid_times() {
        let mut game = TeamDeathmatch::start(0.0, 1.0).unwrap();
        assert!(game.record_kill(0.5, 2, 3, true));
        assert_eq!(
            game.finish(1.0),
            Some(TeamResult {
                winner: Some(2),
                scores: [1, 0]
            })
        );
        assert!(TeamDeathmatch::start(f32::NAN, 1.0).is_err());
        assert!(TeamDeathmatch::start(0.0, f32::INFINITY).is_err());
        assert!(TeamDeathmatch::start(0.0, -1.0).is_err());
        assert!(TeamDeathmatch::start(0.0, 0.0).is_err());
    }
}
