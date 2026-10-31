// Generated from funrounds_native.pplugin
// SPDX-License-Identifier: AGPL-3.0-or-later

#[allow(unused_imports)]
use super::aliases::*;
#[allow(unused_imports)]
use super::enums::*;
#[allow(unused_imports)]
use plugify::{Arr, Mat4x4, Str, Var, Vec2, Vec3, Vec4};

/// Synchronous pickup decision for exact player pawn and controller handles.
#[allow(unused)]
pub type PlayerCanAcquireCallback = unsafe extern "C" fn(i32, i32) -> i32;

/// Synchronous player damage decision with native damage type, exact weapon handle, and per-call rule scope.
#[allow(unused)]
pub type DamagePreCallback = unsafe extern "C" fn(i32, i32, i32, i32, i32, bool, f32, u32, &mut f32, &mut bool, i32, bool) -> i32;

/// Synchronous raw non-player entity damage decision with exact victim/source handles, damage type, and per-call rule scope; return zero to allow or nonzero to suppress.
#[allow(unused)]
pub type EntityDamagePreCallback = unsafe extern "C" fn(i32, i32, i32, i32, f32, u32, bool) -> i32;

/// Synchronous decision for every entity sharing the selected Touch implementation.
#[allow(unused)]
pub type EntityTouchPreCallback = unsafe extern "C" fn(i32, i32) -> i32;

/// Synchronous target observation for an exact live player pawn; target is -1 if no committed match and inUse reports the current E state.
#[allow(unused)]
pub type UseTargetCallback = unsafe extern "C" fn(i32, i32, bool);

/// Returns nonzero only to veto custom healthshot use for the exact player life and healthshot inventory entity.
#[allow(unused)]
pub type HealthshotCanUseCallback = unsafe extern "C" fn(i32, i32) -> i32;

/// Reports the completed native healthshot effect for exact pawn and inventory-weapon handles.
#[allow(unused)]
pub type HealthshotEffectCallback = unsafe extern "C" fn(i32, i32);
