// Generated from funrounds_native.pplugin (group: grenades)
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

/// On the game thread, checks kind 1 smoke, 2 Molotov, or 3 incendiary independently and installs an owned pre-hook that claims natural Detonate entry for complete projectile handles.
///
/// # Arguments
/// * `kind` - (int32)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasGrenadeDetonateSupport(kind: i32) -> bool {
    unsafe { __funrounds_native_HasGrenadeDetonateSupport.expect("HasGrenadeDetonateSupport function was not found")(kind) }
}
pub type _HasGrenadeDetonateSupport = unsafe extern "C" fn(i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasGrenadeDetonateSupport: Option<_HasGrenadeDetonateSupport> = None;

/// On the game thread, checks kind 1 smoke, 2 Molotov, or 3 incendiary independently. Returns 1 when any Detonate entry claimed this exact full handle or its non-sentinel projectile has disappeared, 0 for a valid matching projectile not yet claimed, and -1 when support, kind, identity, or state resolution fails. It never changes projectile physics or calls Detonate.
///
/// # Arguments
/// * `projectileHandle` - (int32)
/// * `kind` - (int32)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetGrenadeDetonationState(projectileHandle: i32, kind: i32) -> i32 {
    unsafe { __funrounds_native_GetGrenadeDetonationState.expect("GetGrenadeDetonationState function was not found")(projectileHandle, kind) }
}
pub type _GetGrenadeDetonationState = unsafe extern "C" fn(i32, i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_GetGrenadeDetonationState: Option<_GetGrenadeDetonationState> = None;

/// On the game thread, requests native Detonate for an exact full handle after caller-side provenance, managed-projectile exclusion, and proximity checks. Returns 1 when submitted, 0 when natural or earlier Detonate was already claimed or the non-sentinel handle normally disappeared, and -1 on support, identity, or submission failure.
///
/// # Arguments
/// * `projectileHandle` - (int32)
/// * `kind` - (int32)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn DetonateGrenade(projectileHandle: i32, kind: i32) -> i32 {
    unsafe { __funrounds_native_DetonateGrenade.expect("DetonateGrenade function was not found")(projectileHandle, kind) }
}
pub type _DetonateGrenade = unsafe extern "C" fn(i32, i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_DetonateGrenade: Option<_DetonateGrenade> = None;
