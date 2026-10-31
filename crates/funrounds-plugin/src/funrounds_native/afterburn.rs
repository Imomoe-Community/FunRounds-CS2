// Generated from funrounds_native.pplugin (group: afterburn)
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

/// On the game thread, attempts native mode-14 ignition for one exact current live player pawn without replacing a valid attached effect. Returns the new attached entityflame full handle for exact caller-owned lifecycle cleanup, or -1 when validation or readback fails.
///
/// # Arguments
/// * `pawnHandle` - (int32)
/// * `duration` - (float)
///
/// # Returns - (int32):
/// -1 on validation/effect verification failure; otherwise the newly attached entityflame full handle.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn TryIgnitePlayer(pawnHandle: i32, duration: f32) -> i32 {
    unsafe { __funrounds_native_TryIgnitePlayer.expect("TryIgnitePlayer function was not found")(pawnHandle, duration) }
}
pub type _TryIgnitePlayer = unsafe extern "C" fn(i32, f32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_TryIgnitePlayer: Option<_TryIgnitePlayer> = None;
