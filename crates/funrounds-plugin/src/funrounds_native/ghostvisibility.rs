// Generated from funrounds_native.pplugin (group: ghostvisibility)
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

/// Installs the resident CCSBot visibility hook and reports whether its current-platform signature is available.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasGhostVisibilitySupport() -> bool {
    unsafe { __funrounds_native_HasGhostVisibilitySupport.expect("HasGhostVisibilitySupport function was not found")() }
}
pub type _HasGhostVisibilitySupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasGhostVisibilitySupport: Option<_HasGhostVisibilitySupport> = None;

/// Atomically replaces the exact full pawn-handle set hidden from BOT visibility; an empty set clears the filter.
///
/// # Arguments
/// * `pawnHandles` - (int32[])
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetGhostVisibilityTargets(pawnHandles: &Arr<i32>) -> bool {
    unsafe { __funrounds_native_SetGhostVisibilityTargets.expect("SetGhostVisibilityTargets function was not found")(pawnHandles) }
}
pub type _SetGhostVisibilityTargets = unsafe extern "C" fn(&Arr<i32>) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_SetGhostVisibilityTargets: Option<_SetGhostVisibilityTargets> = None;

/// On the game thread, reports whether the per-recipient filter for the complete paired passive movement sound-event family is ready for fully hidden Ghost pawn handles. It filters client sound messages only; Hostage.Breath and active combat sounds remain visible.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasGhostPassiveSoundMessageSupport() -> bool {
    unsafe { __funrounds_native_HasGhostPassiveSoundMessageSupport.expect("HasGhostPassiveSoundMessageSupport function was not found")() }
}
pub type _HasGhostPassiveSoundMessageSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasGhostPassiveSoundMessageSupport: Option<_HasGhostPassiveSoundMessageSupport> = None;

/// On the game thread, reports whether the scoped CCSBotManager::OnPlayerFootstep/per-BOT noise filter is ready. It suppresses only footsteps from exact fully hidden Ghost pawns to the exact connected enemy BOT controller; other noise, fall damage, teammates, and combat stay native.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasGhostPassiveNoiseSupport() -> bool {
    unsafe { __funrounds_native_HasGhostPassiveNoiseSupport.expect("HasGhostPassiveNoiseSupport function was not found")() }
}
pub type _HasGhostPassiveNoiseSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasGhostPassiveNoiseSupport: Option<_HasGhostPassiveNoiseSupport> = None;
