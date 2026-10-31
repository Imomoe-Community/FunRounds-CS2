// Generated from funrounds_native.pplugin (group: chickeneating)
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

/// Reports whether the exact committed player-use target callback is available on this platform.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasUseTargetSupport() -> bool {
    unsafe { __funrounds_native_HasUseTargetSupport.expect("HasUseTargetSupport function was not found")() }
}
pub type _HasUseTargetSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasUseTargetSupport: Option<_HasUseTargetSupport> = None;

/// Observes the exact outer use-selector result and committed target for a live player pawn.
///
/// # Arguments
/// * `callback` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HookUseTarget(callback: UseTargetCallback) -> bool {
    unsafe { __funrounds_native_HookUseTarget.expect("HookUseTarget function was not found")(callback) }
}
pub type _HookUseTarget = unsafe extern "C" fn(UseTargetCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HookUseTarget: Option<_HookUseTarget> = None;

/// Removes only this callback from the committed use-target observation hook.
///
/// # Arguments
/// * `callback` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UnhookUseTarget(callback: UseTargetCallback) -> bool {
    unsafe { __funrounds_native_UnhookUseTarget.expect("UnhookUseTarget function was not found")(callback) }
}
pub type _UnhookUseTarget = unsafe extern "C" fn(UseTargetCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_UnhookUseTarget: Option<_UnhookUseTarget> = None;
