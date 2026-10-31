// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fault::{Fault, Mechanism};

pub fn available() -> bool {
    unsafe {
        match (
            crate::funrounds_native::__funrounds_native_HasGroundReverseSupport,
            crate::funrounds_native::__funrounds_native_SetGroundReverseActive,
        ) {
            (Some(query), Some(_)) => query(),
            _ => false,
        }
    }
}

fn set_native(active: bool, acceleration: f32) -> bool {
    unsafe { crate::funrounds_native::__funrounds_native_SetGroundReverseActive }.is_some_and(|set| unsafe { set(active, acceleration) })
}

#[derive(Default)]
pub struct GroundReverse {
    acceleration: Option<f32>,
    cleanup_pending: bool,
    retry_suppressed: bool,
}

impl GroundReverse {
    pub fn is_clean(&self) -> bool {
        !self.cleanup_pending
    }

    pub fn retry_cleanup(&mut self) {
        self.retry_suppressed = false;
    }

    pub fn apply(&mut self, acceleration: Option<f32>) -> Result<(), Fault> {
        self.apply_with(acceleration, set_native)
    }

    fn apply_with(&mut self, acceleration: Option<f32>, mut set: impl FnMut(bool, f32) -> bool) -> Result<(), Fault> {
        if self.retry_suppressed {
            return Err(Fault::global("ground-reverse hook detach unconfirmed"));
        }
        if acceleration == self.acceleration && (acceleration.is_some() || !self.cleanup_pending) {
            return Ok(());
        }
        if let Some(value) = acceleration {
            // A failed registration can retain a partial native detour.
            self.cleanup_pending = true;
            if set(true, value) {
                self.acceleration = Some(value);
                return Ok(());
            }
            self.acceleration = None;
            if set(false, value) {
                self.cleanup_pending = false;
                return Err(Fault::mechanism(Mechanism::GroundReverse, "ground-reverse hook activation failed"));
            }
        } else {
            self.acceleration = None;
            if set(false, 5.5) {
                self.cleanup_pending = false;
                return Ok(());
            }
        }
        // Retry only at an explicit disable/unload boundary, not every game frame.
        self.retry_suppressed = true;
        Err(Fault::global("ground-reverse hook detach unconfirmed"))
    }
}
