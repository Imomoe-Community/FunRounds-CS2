// Generated from funrounds_native.pplugin (group: chickenfollow)
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

/// Reports whether the exact CChickenChickenUse callback detour is available on this platform.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasChickenFollowBlockSupport() -> bool {
    unsafe { __funrounds_native_HasChickenFollowBlockSupport.expect("HasChickenFollowBlockSupport function was not found")() }
}
pub type _HasChickenFollowBlockSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasChickenFollowBlockSupport: Option<_HasChickenFollowBlockSupport> = None;

/// On the game thread, enables the native chicken-follow veto or disables it and removes only this provider's detour. Returns false while setup or owned-hook cleanup remains incomplete.
///
/// # Arguments
/// * `active` - (bool)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetChickenFollowBlocking(active: bool) -> bool {
    unsafe { __funrounds_native_SetChickenFollowBlocking.expect("SetChickenFollowBlocking function was not found")(active) }
}
pub type _SetChickenFollowBlocking = unsafe extern "C" fn(bool) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_SetChickenFollowBlocking: Option<_SetChickenFollowBlocking> = None;
