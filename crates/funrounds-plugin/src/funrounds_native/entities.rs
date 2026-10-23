// Generated from funrounds_native.pplugin (group: entities)

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

/// Reports whether the Touch vtable offset is available in the native gamedata.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn IsEntityTouchPreHookAvailable() -> bool {
    unsafe { __funrounds_native_IsEntityTouchPreHookAvailable.expect("IsEntityTouchPreHookAvailable function was not found")() }
}
pub type _IsEntityTouchPreHookAvailable = unsafe extern "C" fn() -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_IsEntityTouchPreHookAvailable: Option<_IsEntityTouchPreHookAvailable> = None;

/// Hooks the entity's current Touch implementation address; the callback sees all entities sharing it.
///
/// # Arguments
/// * `entityHandle` - (int32)
/// * `callback` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HookEntityTouchPre(entityHandle: i32, callback: EntityTouchPreCallback) -> bool {
    unsafe { __funrounds_native_HookEntityTouchPre.expect("HookEntityTouchPre function was not found")(entityHandle, callback) }
}
pub type _HookEntityTouchPre = unsafe extern "C" fn(i32, EntityTouchPreCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_HookEntityTouchPre: Option<_HookEntityTouchPre> = None;

/// Revokes the Touch callback and removes all Touch implementation detours it owns.
///
/// # Arguments
/// * `callback` - (function)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UnhookEntityTouchPre(callback: EntityTouchPreCallback) -> bool {
    unsafe { __funrounds_native_UnhookEntityTouchPre.expect("UnhookEntityTouchPre function was not found")(callback) }
}
pub type _UnhookEntityTouchPre = unsafe extern "C" fn(EntityTouchPreCallback) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_UnhookEntityTouchPre: Option<_UnhookEntityTouchPre> = None;

/// Sets both collision-group fields to the standard default and notifies collision rules.
///
/// # Arguments
/// * `entityHandle` - (int32)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEntityDefaultCollisionGroup(entityHandle: i32) {
    unsafe { __funrounds_native_SetEntityDefaultCollisionGroup.expect("SetEntityDefaultCollisionGroup function was not found")(entityHandle) }
}
pub type _SetEntityDefaultCollisionGroup = unsafe extern "C" fn(i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __funrounds_native_SetEntityDefaultCollisionGroup: Option<_SetEntityDefaultCollisionGroup> = None;
