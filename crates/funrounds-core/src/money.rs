// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

/// Return the balance after raising it to a round floor, capped at the live cash limit.
/// Existing balances above the limit are preserved.
pub fn money_floor(current: i64, floor: u32, limit: i64) -> i64 {
    if limit <= 0 || current >= limit || current >= i64::from(floor) {
        return current;
    }
    i64::from(floor).min(limit)
}

/// Return the balance after income, capped at the live cash limit.
/// Existing balances above the limit are preserved.
pub fn money_income_credit(current: i64, amount: u32, limit: i64) -> i64 {
    if limit <= 0 || current >= limit {
        return current;
    }
    current.saturating_add(i64::from(amount)).min(limit)
}

/// Schedule the next income tick after `now`; missed ticks are skipped, never paid retroactively.
pub fn next_money_income_due(interval: f32, scheduled: f32, now: f32) -> Option<f32> {
    if !interval.is_finite() || interval <= 0.0 || !scheduled.is_finite() || !now.is_finite() {
        return None;
    }
    if now < scheduled {
        return Some(scheduled);
    }
    let interval = f64::from(interval);
    let scheduled = f64::from(scheduled);
    let now = f64::from(now);
    let next = scheduled + (((now - scheduled) / interval).floor() + 1.0) * interval;
    let next = next as f32;
    next.is_finite().then_some(next)
}
