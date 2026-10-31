// Generated from funrounds_native.pplugin (group: healthshot)
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

/// Reports whether the healthshot eligibility and successful-effect hooks resolve on this platform.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasHealthshotSupport() -> bool {
    unsafe { __funrounds_native_HasHealthshotSupport.expect("HasHealthshotSupport function was not found")() }
}
pub type _HasHealthshotSupport = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasHealthshotSupport: Option<_HasHealthshotSupport> = None;

/// Blocks only the custom B-relative cap decision before consumption and reports successful native effects afterward.
///
/// # Arguments
/// * `canUse` - (function)
/// * `effect` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HookHealthshot(canUse: HealthshotCanUseCallback, effect: HealthshotEffectCallback) -> bool {
    unsafe { __funrounds_native_HookHealthshot.expect("HookHealthshot function was not found")(canUse, effect) }
}
pub type _HookHealthshot = unsafe extern "C" fn(HealthshotCanUseCallback, HealthshotEffectCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HookHealthshot: Option<_HookHealthshot> = None;

/// Removes only the two healthshot callbacks owned by this provider consumer.
///
/// # Arguments
/// * `canUse` - (function)
/// * `effect` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UnhookHealthshot(canUse: HealthshotCanUseCallback, effect: HealthshotEffectCallback) -> bool {
    unsafe { __funrounds_native_UnhookHealthshot.expect("UnhookHealthshot function was not found")(canUse, effect) }
}
pub type _UnhookHealthshot = unsafe extern "C" fn(HealthshotCanUseCallback, HealthshotEffectCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_UnhookHealthshot: Option<_UnhookHealthshot> = None;
