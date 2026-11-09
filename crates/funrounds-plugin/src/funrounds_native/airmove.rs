// Generated from funrounds_native.pplugin (group: airmove)
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

/// Reports whether the verified AirMove entry and current pawn identity schema are available.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasAirMoveSupport() -> bool {
    unsafe { __funrounds_native_HasAirMoveSupport.expect("HasAirMoveSupport function was not found")() }
}
pub type _HasAirMoveSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasAirMoveSupport: Option<_HasAirMoveSupport> = None;

/// Attaches or detaches temporary AirMove input assistance for this callback.
///
/// # Arguments
/// * `active` - (bool)
/// * `assist` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetAirMoveAssist(active: bool, assist: AirMoveAssist) -> bool {
    unsafe { __funrounds_native_SetAirMoveAssist.expect("SetAirMoveAssist function was not found")(active, assist) }
}
pub type _SetAirMoveAssist = unsafe extern "C" fn(bool, AirMoveAssist) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_SetAirMoveAssist: Option<_SetAirMoveAssist> = None;
