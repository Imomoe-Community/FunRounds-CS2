// Generated from funrounds_native.pplugin (group: ghostradar)
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

/// On the game thread, reports whether both the per-recipient radar update filter and spotted-state filter are ready.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasGhostRadarSupport() -> bool {
    unsafe { __funrounds_native_HasGhostRadarSupport.expect("HasGhostRadarSupport function was not found")() }
}
pub type _HasGhostRadarSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasGhostRadarSupport: Option<_HasGhostRadarSupport> = None;

/// On the game thread, publishes full pawn handles and parallel 64-bit masks of living enemy recipients whose radar updates must be hidden. Empty parallel arrays clear the snapshot.
///
/// # Arguments
/// * `fullPawnHandles` - (int32[])
/// * `enemyRecipientMasks` - (uint64[])
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetGhostRadarTargets(fullPawnHandles: &Arr<i32>, enemyRecipientMasks: &Arr<u64>) -> bool {
    unsafe { __funrounds_native_SetGhostRadarTargets.expect("SetGhostRadarTargets function was not found")(fullPawnHandles, enemyRecipientMasks) }
}
pub type _SetGhostRadarTargets = unsafe extern "C" fn(&Arr<i32>, &Arr<u64>) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_SetGhostRadarTargets: Option<_SetGhostRadarTargets> = None;
