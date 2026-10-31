// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThirdPersonSettings {
    pub distance: f32,
    pub follow_eyes: bool,
    pub clip_camera_offset: bool,
    pub return_strength: f32,
}

impl Default for ThirdPersonSettings {
    fn default() -> Self {
        Self {
            distance: 64.0,
            follow_eyes: true,
            clip_camera_offset: true,
            return_strength: 1.0,
        }
    }
}

impl ThirdPersonSettings {
    pub(crate) fn validate(self) -> Result<(), String> {
        if !self.distance.is_finite() || !(15.0..=150.0).contains(&self.distance) {
            return Err("third_person distance must be finite and between 15 and 150".into());
        }
        if !self.return_strength.is_finite() || self.return_strength <= 0.0 {
            return Err("third_person return_strength must be finite and positive".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ThirdPersonInput {
    distance: Option<f32>,
    follow_eyes: Option<bool>,
    clip_camera_offset: Option<bool>,
    return_strength: Option<f32>,
}

impl ThirdPersonInput {
    pub(crate) fn resolve(self, base: ThirdPersonSettings) -> ThirdPersonSettings {
        ThirdPersonSettings {
            distance: self.distance.unwrap_or(base.distance),
            follow_eyes: self.follow_eyes.unwrap_or(base.follow_eyes),
            clip_camera_offset: self.clip_camera_offset.unwrap_or(base.clip_camera_offset),
            return_strength: self.return_strength.unwrap_or(base.return_strength),
        }
    }
}
