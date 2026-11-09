// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::PlayerIdentity;

pub const MEDICAL_SMOKE_RADIUS: f32 = 160.0;
pub const MEDICAL_SMOKE_HEAL: i32 = 5;
pub const MEDICAL_SMOKE_INTERVAL_SECONDS: f64 = 1.0;

pub const TRAMPOLINE_RADIUS: f32 = 130.0;
pub const TRAMPOLINE_HALF_HEIGHT: f32 = 32.0;
pub const TRAMPOLINE_REARM_MARGIN: f32 = 16.0;
pub const TRAMPOLINE_LAUNCH_Z: f32 = 700.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Position {
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn within_sphere(self, center: Self, radius: f32) -> bool {
        if !self.is_finite() || !center.is_finite() || !radius.is_finite() || radius < 0.0 {
            return false;
        }
        let dx = f64::from(self.x) - f64::from(center.x);
        let dy = f64::from(self.y) - f64::from(center.y);
        let dz = f64::from(self.z) - f64::from(center.z);
        let radius = f64::from(radius);
        dx * dx + dy * dy + dz * dz <= radius * radius
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrampolineRegion {
    pub center: Position,
}

impl TrampolineRegion {
    pub fn contains(self, position: Position) -> bool {
        if !position.is_finite() || !self.center.is_finite() {
            return false;
        }
        let dx = f64::from(position.x) - f64::from(self.center.x);
        let dy = f64::from(position.y) - f64::from(self.center.y);
        dx * dx + dy * dy <= f64::from(TRAMPOLINE_RADIUS).powi(2) && (position.z - self.center.z).abs() <= TRAMPOLINE_HALF_HEIGHT
    }

    pub fn outside_rearm(self, position: Position) -> bool {
        if !position.is_finite() || !self.center.is_finite() {
            return false;
        }
        let dx = f64::from(position.x) - f64::from(self.center.x);
        let dy = f64::from(position.y) - f64::from(self.center.y);
        let radius = f64::from(TRAMPOLINE_RADIUS + TRAMPOLINE_REARM_MARGIN);
        dx * dx + dy * dy > radius * radius || (position.z - self.center.z).abs() > TRAMPOLINE_HALF_HEIGHT + TRAMPOLINE_REARM_MARGIN
    }
}

/// Returns the only regions supported by the first trampoline capability.
pub fn fixed_trampoline_region(map: &str) -> Option<TrampolineRegion> {
    let center = if map.eq_ignore_ascii_case("de_dust2") {
        Position {
            x: -1141.0,
            y: -808.0,
            z: 117.0,
        }
    } else if map.eq_ignore_ascii_case("de_nuke") {
        Position {
            x: 1105.0,
            y: -950.0,
            z: -767.97,
        }
    } else {
        return None;
    };
    Some(TrampolineRegion { center })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrampolineStep {
    pub rearmed: bool,
    pub triggered: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrampolineLatch {
    identity: Option<PlayerIdentity>,
    armed: bool,
}

impl TrampolineLatch {
    /// Preserve a launch across control transfer of the same live pawn.
    pub fn rebind(&mut self, identity: PlayerIdentity) -> bool {
        if self.identity.is_none_or(|previous| previous.pawn_handle != identity.pawn_handle) {
            return false;
        }
        self.identity = Some(identity);
        true
    }

    /// Observe one live player's sampled state. A new life starts armed only outside the region.
    pub fn observe(&mut self, identity: PlayerIdentity, position: Position, grounded: bool, region: TrampolineRegion) -> TrampolineStep {
        if self.identity != Some(identity) {
            self.identity = Some(identity);
            self.armed = !region.contains(position);
            return TrampolineStep::default();
        }

        let rearmed = !self.armed && grounded && region.outside_rearm(position);
        if rearmed {
            self.armed = true;
        }
        let triggered = self.armed && grounded && region.contains(position);
        if triggered {
            self.armed = false;
        }
        TrampolineStep { rearmed, triggered }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Teammate healing needs a real CS team and is intentionally unavailable in FFA modes.
pub const fn medical_smoke_team(team: i32, teammates_are_enemies: bool) -> Option<i32> {
    if !teammates_are_enemies && matches!(team, 2 | 3) {
        Some(team)
    } else {
        None
    }
}
