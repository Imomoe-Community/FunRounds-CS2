// Generated from funrounds_native.pplugin (group: groundreverse)
// SPDX-License-Identifier: AGPL-3.0-or-later

#[allow(unused_imports)]
use super::aliases::*;
#[allow(unused_imports)]
use super::delegates::*;
#[allow(unused_imports)]
use super::enums::*;
#[allow(unused_imports)]
use plugify::{Arr, Mat4x4, Str, Var, Vec2, Vec3, Vec4, scope};
#[allow(unused_imports)]
use std::sync::RwLock;

/// Reports whether the platform ground-reverse callsite and current-controller identity schema are available.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasGroundReverseSupport() -> bool {
    unsafe { __funrounds_native_HasGroundReverseSupport.expect("HasGroundReverseSupport function was not found")() }
}
pub type _HasGroundReverseSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasGroundReverseSupport: Option<_HasGroundReverseSupport> = None;

/// Enables ground-reversed human movement and normal BOT movement with the configured local ground acceleration; false detaches the hook.
///
/// # Arguments
/// * `active` - (bool)
/// * `acceleration` - (float)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetGroundReverseActive(active: bool, acceleration: f32) -> bool {
    unsafe { __funrounds_native_SetGroundReverseActive.expect("SetGroundReverseActive function was not found")(active, acceleration) }
}
pub type _SetGroundReverseActive = unsafe extern "C" fn(bool, f32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_SetGroundReverseActive: Option<_SetGroundReverseActive> = None;
