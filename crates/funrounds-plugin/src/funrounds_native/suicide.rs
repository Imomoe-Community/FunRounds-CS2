// Generated from funrounds_native.pplugin (group: suicide)

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

/// Reports whether the optional CanAcquire hook and inspect-button schema are available.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasSuicideSupport() -> bool {
    unsafe { __funrounds_native_HasSuicideSupport.expect("HasSuicideSupport function was not found")() }
}
pub type _HasSuicideSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasSuicideSupport: Option<_HasSuicideSupport> = None;

/// Returns live IN_LOOK_AT_WEAPON state for an exact pawn handle, or -1 when unavailable.
///
/// # Arguments
/// * `pawnHandle` - (int32)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetLookAtWeaponButtonState(pawnHandle: i32) -> i32 {
    unsafe { __funrounds_native_GetLookAtWeaponButtonState.expect("GetLookAtWeaponButtonState function was not found")(pawnHandle) }
}
pub type _GetLookAtWeaponButtonState = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_GetLookAtWeaponButtonState: Option<_GetLookAtWeaponButtonState> = None;

/// Hooks pickup acquisition decisions so only active suicide bombers are denied weapon pickups.
///
/// # Arguments
/// * `callback` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HookPlayerCanAcquire(callback: PlayerCanAcquireCallback) -> bool {
    unsafe { __funrounds_native_HookPlayerCanAcquire.expect("HookPlayerCanAcquire function was not found")(callback) }
}
pub type _HookPlayerCanAcquire = unsafe extern "C" fn(PlayerCanAcquireCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HookPlayerCanAcquire: Option<_HookPlayerCanAcquire> = None;

/// Removes the optional CanAcquire detour and callback.
///
/// # Arguments
/// * `callback` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UnhookPlayerCanAcquire(callback: PlayerCanAcquireCallback) -> bool {
    unsafe { __funrounds_native_UnhookPlayerCanAcquire.expect("UnhookPlayerCanAcquire function was not found")(callback) }
}
pub type _UnhookPlayerCanAcquire = unsafe extern "C" fn(PlayerCanAcquireCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_UnhookPlayerCanAcquire: Option<_UnhookPlayerCanAcquire> = None;
