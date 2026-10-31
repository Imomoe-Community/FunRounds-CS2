// Generated from funrounds_native.pplugin (group: thirdperson)
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

/// On the game thread, checks the optional pawn eye camera-branch Hook ABI and schema. Does not change any view.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasThirdPersonEyeSupport() -> bool {
    unsafe { __funrounds_native_HasThirdPersonEyeSupport.expect("HasThirdPersonEyeSupport function was not found")() }
}
pub type _HasThirdPersonEyeSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasThirdPersonEyeSupport: Option<_HasThirdPersonEyeSupport> = None;

/// On the game thread, registers one exact pawn/camera pair before assigning its view. Only a live own-camera path uses the original pawn eye branch.
///
/// # Arguments
/// * `pawn` - (int32)
/// * `camera` - (int32)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn RegisterThirdPersonEyeTarget(pawn: i32, camera: i32) -> bool {
    unsafe { __funrounds_native_RegisterThirdPersonEyeTarget.expect("RegisterThirdPersonEyeTarget function was not found")(pawn, camera) }
}
pub type _RegisterThirdPersonEyeTarget = unsafe extern "C" fn(i32, i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_RegisterThirdPersonEyeTarget: Option<_RegisterThirdPersonEyeTarget> = None;

/// On the game thread, removes an exact pair only after its pawn view no longer references the owned camera. A failed detach remains retryable.
///
/// # Arguments
/// * `pawn` - (int32)
/// * `camera` - (int32)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UnregisterThirdPersonEyeTarget(pawn: i32, camera: i32) -> bool {
    unsafe { __funrounds_native_UnregisterThirdPersonEyeTarget.expect("UnregisterThirdPersonEyeTarget function was not found")(pawn, camera) }
}
pub type _UnregisterThirdPersonEyeTarget = unsafe extern "C" fn(i32, i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_UnregisterThirdPersonEyeTarget: Option<_UnregisterThirdPersonEyeTarget> = None;

/// Map transition only: drops all old-map target identities and detaches the eye Hook without resolving or modifying reused entity handles.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn ResetThirdPersonEyeTargets() -> bool {
    unsafe { __funrounds_native_ResetThirdPersonEyeTargets.expect("ResetThirdPersonEyeTargets function was not found")() }
}
pub type _ResetThirdPersonEyeTargets = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_ResetThirdPersonEyeTargets: Option<_ResetThirdPersonEyeTargets> = None;
