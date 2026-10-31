// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::TeamFilter;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostSettings {
    pub team: TeamFilter,
    pub crouch_alpha: u8,
    pub slow_walk_alpha: u8,
    pub normal_alpha: u8,
    pub speed_reveal_start: f32,
    pub speed_reveal_full: f32,
    pub reveal_delay: f32,
    pub breath_interval: f32,
    pub breath_volume: f32,
    pub breath_distance: f32,
}

impl Default for GhostSettings {
    fn default() -> Self {
        Self {
            team: TeamFilter::Terrorists,
            crouch_alpha: 0,
            slow_walk_alpha: 13,
            normal_alpha: 127,
            speed_reveal_start: 100.0,
            speed_reveal_full: 200.0,
            reveal_delay: 0.25,
            breath_interval: 10.0,
            breath_volume: 0.9,
            breath_distance: 1000.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhostPose {
    Still,
    Crouching,
    SlowWalking,
    Normal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostFrame {
    pub pose: GhostPose,
    pub horizontal_speed: f32,
    pub bunny_hop: bool,
    pub attacking: bool,
    pub reloading: bool,
    pub using: bool,
}

impl Default for GhostFrame {
    fn default() -> Self {
        Self {
            pose: GhostPose::Normal,
            horizontal_speed: 0.0,
            bunny_hop: false,
            attacking: false,
            reloading: false,
            using: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GhostState {
    reloading: bool,
    reveal_until: f32,
}

impl GhostState {
    pub fn alpha(&mut self, settings: GhostSettings, frame: GhostFrame, now: f32) -> u8 {
        let reloading = frame.reloading;
        if reloading || self.reloading {
            self.reveal_until = now + settings.reveal_delay;
        }
        self.reloading = reloading;
        ghost_alpha(settings, frame, reloading, self.reveal_until, now)
    }
}

pub fn ghost_alpha(settings: GhostSettings, frame: GhostFrame, reloading: bool, reveal_until: f32, now: f32) -> u8 {
    if reloading || now < reveal_until {
        return settings.normal_alpha;
    }
    if frame.bunny_hop || !frame.horizontal_speed.is_finite() {
        return pose_alpha(settings, frame.pose);
    }
    if frame.horizontal_speed <= settings.speed_reveal_start {
        return settings.crouch_alpha;
    }
    let progress =
        ((frame.horizontal_speed - settings.speed_reveal_start) / (settings.speed_reveal_full - settings.speed_reveal_start)).clamp(0.0, 1.0);
    let start_alpha = settings.crouch_alpha;
    (f32::from(start_alpha) + (f32::from(settings.normal_alpha) - f32::from(start_alpha)) * progress).round() as u8
}

const fn pose_alpha(settings: GhostSettings, pose: GhostPose) -> u8 {
    match pose {
        GhostPose::Still | GhostPose::Crouching => settings.crouch_alpha,
        GhostPose::SlowWalking => settings.slow_walk_alpha,
        GhostPose::Normal => settings.normal_alpha,
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct GhostInput {
    team: Option<TeamFilter>,
    crouch_alpha: Option<u8>,
    slow_walk_alpha: Option<u8>,
    normal_alpha: Option<u8>,
    speed_reveal_start: Option<f32>,
    speed_reveal_full: Option<f32>,
    reveal_delay: Option<f32>,
    breath_interval: Option<f32>,
    breath_volume: Option<f32>,
    breath_distance: Option<f32>,
}

impl GhostInput {
    pub(crate) fn resolve(self, base: GhostSettings) -> GhostSettings {
        GhostSettings {
            team: self.team.unwrap_or(base.team),
            crouch_alpha: self.crouch_alpha.unwrap_or(base.crouch_alpha),
            slow_walk_alpha: self.slow_walk_alpha.unwrap_or(base.slow_walk_alpha),
            normal_alpha: self.normal_alpha.unwrap_or(base.normal_alpha),
            speed_reveal_start: self.speed_reveal_start.unwrap_or(base.speed_reveal_start),
            speed_reveal_full: self.speed_reveal_full.unwrap_or(base.speed_reveal_full),
            reveal_delay: self.reveal_delay.unwrap_or(base.reveal_delay),
            breath_interval: self.breath_interval.unwrap_or(base.breath_interval),
            breath_volume: self.breath_volume.unwrap_or(base.breath_volume),
            breath_distance: self.breath_distance.unwrap_or(base.breath_distance),
        }
    }
}

impl GhostSettings {
    pub(crate) fn validate(self) -> Result<(), String> {
        if !self.reveal_delay.is_finite() || self.reveal_delay < 0.0 {
            return Err("ghost reveal_delay must be finite and non-negative".into());
        }
        if !self.speed_reveal_start.is_finite()
            || self.speed_reveal_start < 0.0
            || !self.speed_reveal_full.is_finite()
            || self.speed_reveal_full <= self.speed_reveal_start
        {
            return Err("ghost speed_reveal_full must be finite and greater than non-negative speed_reveal_start".into());
        }
        if !self.breath_interval.is_finite() || self.breath_interval < 0.0 {
            return Err("ghost breath_interval must be finite and non-negative".into());
        }
        if !self.breath_volume.is_finite() || !(0.0..=1.0).contains(&self.breath_volume) {
            return Err("ghost breath_volume must be finite and between 0 and 1".into());
        }
        if !self.breath_distance.is_finite() || self.breath_distance < 0.0 {
            return Err("ghost breath_distance must be finite and non-negative".into());
        }
        Ok(())
    }
}
