// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::fault::{Fault, Mechanism};
use crate::{RUNTIME, callbacks_active, contain_failure, funrounds_native};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub fn available() -> bool {
    unsafe {
        match (
            funrounds_native::__funrounds_native_HasAirMoveSupport,
            funrounds_native::__funrounds_native_SetAirMoveAssist,
        ) {
            (Some(query), Some(_)) => query(),
            _ => false,
        }
    }
}

#[derive(Default, PartialEq, Eq)]
pub enum AirStrafe {
    #[default]
    Detached,
    Attached,
    CleanupPending,
}

impl AirStrafe {
    pub fn is_clean(&self) -> bool {
        *self == Self::Detached
    }

    pub fn retry_cleanup(&mut self) {
        if *self == Self::CleanupPending {
            *self = Self::Attached;
        }
    }

    pub fn apply(&mut self, active: bool) -> Result<(), Fault> {
        self.apply_with(active, |active| funrounds_native::SetAirMoveAssist(active, assist))
    }

    fn apply_with(&mut self, active: bool, mut set: impl FnMut(bool) -> bool) -> Result<(), Fault> {
        if *self == Self::CleanupPending || active == (*self == Self::Attached) {
            return Ok(());
        }
        if active && set(true) {
            *self = Self::Attached;
            return Ok(());
        }
        if set(false) {
            *self = Self::Detached;
            return if active {
                Err(Fault::mechanism(Mechanism::Autostrafe, "AirMove hook activation failed"))
            } else {
                Ok(())
            };
        }
        *self = Self::CleanupPending;
        Err(Fault::global("AirMove hook detach unconfirmed"))
    }
}

unsafe extern "C" fn assist(slot: i32, pawn: i32, controller: i32, yaw: f32, vx: f32, vy: f32, forward: &mut f32, left: &mut f32) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        if !callbacks_active()
            || !RUNTIME
                .try_lock()
                .is_ok_and(|guard| guard.as_ref().is_some_and(|runtime| runtime.air_strafe_allowed(slot, pawn, controller)))
        {
            return false;
        }
        if let Some((f, l)) = funrounds_core::air_strafe_input(yaw, vx, vy) {
            (*forward, *left) = (f, l);
            true
        } else {
            false
        }
    }))
    .unwrap_or_else(|_| {
        contain_failure("AirMove assist callback panic contained");
        false
    })
}
