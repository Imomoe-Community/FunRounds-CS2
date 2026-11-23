// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::f32::consts::{PI, TAU};

/// The native AirMove step consumes forward and left input relative to aim.
pub fn air_strafe_input(yaw: f32, vx: f32, vy: f32) -> Option<(f32, f32)> {
    let aim = yaw.to_radians();
    let speed = vx.hypot(vy);
    if !aim.is_finite() || !speed.is_finite() {
        return None;
    }
    let heading = vy.atan2(vx);
    let turn = (aim - heading + PI).rem_euclid(TAU) - PI;
    if speed >= 30.0 && turn.abs() < 2.0_f32.to_radians() {
        return None;
    }
    let wish = if speed < 30.0 {
        aim
    } else {
        heading + (20.0 / speed).acos().copysign(turn)
    };
    let (left, forward) = (wish - aim).sin_cos();
    Some((200.0 * forward, 200.0 * left))
}
