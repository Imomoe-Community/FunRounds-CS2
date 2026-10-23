// Generated from funrounds_native.pplugin (group: damage)

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

/// Reports whether the current native damage-hook prerequisites are available.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HasPlayerTakeDamagePre() -> bool {
    unsafe { __funrounds_native_HasPlayerTakeDamagePre.expect("HasPlayerTakeDamagePre function was not found")() }
}
pub type _HasPlayerTakeDamagePre = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HasPlayerTakeDamagePre: Option<_HasPlayerTakeDamagePre> = None;

/// Installs the exclusive synchronous player damage policy callback.
///
/// # Arguments
/// * `callback` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HookPlayerTakeDamagePre(callback: DamagePreCallback) -> bool {
    unsafe { __funrounds_native_HookPlayerTakeDamagePre.expect("HookPlayerTakeDamagePre function was not found")(callback) }
}
pub type _HookPlayerTakeDamagePre = unsafe extern "C" fn(DamagePreCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HookPlayerTakeDamagePre: Option<_HookPlayerTakeDamagePre> = None;

/// Revokes the player damage callback and removes its detour.
///
/// # Arguments
/// * `callback` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UnhookPlayerTakeDamagePre(callback: DamagePreCallback) -> bool {
    unsafe { __funrounds_native_UnhookPlayerTakeDamagePre.expect("UnhookPlayerTakeDamagePre function was not found")(callback) }
}
pub type _UnhookPlayerTakeDamagePre = unsafe extern "C" fn(DamagePreCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_UnhookPlayerTakeDamagePre: Option<_UnhookPlayerTakeDamagePre> = None;

/// Returns the current nested damage-call token, or zero outside a damage call.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetCurrentDamageFrameToken() -> u64 {
    unsafe { __funrounds_native_GetCurrentDamageFrameToken.expect("GetCurrentDamageFrameToken function was not found")() }
}
pub type _GetCurrentDamageFrameToken = unsafe extern "C" fn() -> u64;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_GetCurrentDamageFrameToken: Option<_GetCurrentDamageFrameToken> = None;

/// Applies native damage with an exact ability weapon handle and optional callback-specific rule bypass.
///
/// # Arguments
/// * `victimHandle` - (int32)
/// * `inflictorHandle` - (int32)
/// * `attackerHandle` - (int32)
/// * `force` - (vec3)
/// * `position` - (vec3)
/// * `amount` - (float)
/// * `damageType` - (int32)
/// * `weaponHandle` - (int32)
/// * `skipPluginRules` - (bool)
#[track_caller]
#[allow(dead_code, non_snake_case, clippy::too_many_arguments)]
pub fn TakeEntityDamageWithWeapon(
    victimHandle: i32,
    inflictorHandle: i32,
    attackerHandle: i32,
    force: &Vec3,
    position: &Vec3,
    amount: f32,
    damageType: i32,
    weaponHandle: i32,
    skipPluginRules: bool,
) -> i32 {
    unsafe {
        __funrounds_native_TakeEntityDamageWithWeapon.expect("TakeEntityDamageWithWeapon function was not found")(
            victimHandle,
            inflictorHandle,
            attackerHandle,
            force,
            position,
            amount,
            damageType,
            weaponHandle,
            skipPluginRules,
        )
    }
}
pub type _TakeEntityDamageWithWeapon = unsafe extern "C" fn(i32, i32, i32, &Vec3, &Vec3, f32, i32, i32, bool) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_TakeEntityDamageWithWeapon: Option<_TakeEntityDamageWithWeapon> = None;
