// Generated from funrounds_native.pplugin (group: bighead)
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

/// Reports whether the current server build has the verified Pawn reset hook, schema field and GameRules mutator-bit contract needed to own Big Head scaling.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasBigHeadScaleSupport() -> bool {
    unsafe { __funrounds_native_HasBigHeadScaleSupport.expect("HasBigHeadScaleSupport function was not found")() }
}
pub type _HasBigHeadScaleSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasBigHeadScaleSupport: Option<_HasBigHeadScaleSupport> = None;

/// On the game thread, targets a finite positive scale for one exact live player Pawn after its native reset. Returns false if the target or current build contract cannot be confirmed.
///
/// # Arguments
/// * `pawnHandle` - (int32)
/// * `scale` - (float)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetBigHeadScaleTarget(pawnHandle: i32, scale: f32) -> bool {
    unsafe { __funrounds_native_SetBigHeadScaleTarget.expect("SetBigHeadScaleTarget function was not found")(pawnHandle, scale) }
}
pub type _SetBigHeadScaleTarget = unsafe extern "C" fn(i32, f32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_SetBigHeadScaleTarget: Option<_SetBigHeadScaleTarget> = None;

/// On the game thread, removes the exact Pawn target and restores its captured original scale only while the current Pawn still holds the owned value; map changes discard old handles without writing through them.
///
/// # Arguments
/// * `pawnHandle` - (int32)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn ClearBigHeadScaleTarget(pawnHandle: i32) -> bool {
    unsafe { __funrounds_native_ClearBigHeadScaleTarget.expect("ClearBigHeadScaleTarget function was not found")(pawnHandle) }
}
pub type _ClearBigHeadScaleTarget = unsafe extern "C" fn(i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_ClearBigHeadScaleTarget: Option<_ClearBigHeadScaleTarget> = None;
