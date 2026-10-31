// Generated from s2sdk.pplugin (group: gamerules)
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

/// Retrieves the pointer to the current game rules instance.
///
/// # Returns - (ptr64):
/// A pointer to the game rules object.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetGameRules() -> usize {
    unsafe { __s2sdk_GetGameRules.expect("GetGameRules function was not found")() }
}
pub type _GetGameRules = unsafe extern "C" fn() -> usize;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetGameRules: Option<_GetGameRules> = None;

/// Forces the round to end with a specified reason and delay.
///
/// # Arguments
/// * `delay` - (float): Time (in seconds) to delay before the next round starts.
/// * `reason` - (int32): The reason for ending the round, defined by the CSRoundEndReason enum.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn TerminateRound(delay: f32, reason: CSRoundEndReason) {
    unsafe { __s2sdk_TerminateRound.expect("TerminateRound function was not found")(delay, reason) }
}
pub type _TerminateRound = unsafe extern "C" fn(f32, CSRoundEndReason);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_TerminateRound: Option<_TerminateRound> = None;
