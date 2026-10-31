// Generated from funrounds_native.pplugin (group: randomspawn)
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

/// Whether the native engine spawn-point selector is available. Call on the game thread.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasPlayerRandomSpawnPointSupport() -> bool {
    unsafe { __funrounds_native_HasPlayerRandomSpawnPointSupport.expect("HasPlayerRandomSpawnPointSupport function was not found")() }
}
pub type _HasPlayerRandomSpawnPointSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasPlayerRandomSpawnPointSupport: Option<_HasPlayerRandomSpawnPointSupport> = None;

/// Validate and select a map-defined deathmatch spawn point for an exact pawn, starting at a complete point handle. Returns a complete handle, or -1 including engine fallback to normal team starts. Does not move the player. Game thread only.
///
/// # Arguments
/// * `pawnHandle` - (int32)
/// * `startPointHandle` - (int32)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SelectPlayerRandomSpawnPoint(pawnHandle: i32, startPointHandle: i32) -> i32 {
    unsafe {
        __funrounds_native_SelectPlayerRandomSpawnPoint.expect("SelectPlayerRandomSpawnPoint function was not found")(pawnHandle, startPointHandle)
    }
}
pub type _SelectPlayerRandomSpawnPoint = unsafe extern "C" fn(i32, i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_SelectPlayerRandomSpawnPoint: Option<_SelectPlayerRandomSpawnPoint> = None;
