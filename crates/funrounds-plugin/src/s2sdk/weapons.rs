// Generated from s2sdk.pplugin (group: weapons)

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

/// Retrieves the weapon VData for a given weapon.
///
/// # Arguments
/// * `entityHandle` - (int32): The handle of the entity from which to retrieve the weapon VData.
///
/// # Returns - (ptr64):
/// A pointer to the `CCSWeaponBaseVData` if the entity handle is valid and represents a player weapon; otherwise, nullptr.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetWeaponVData(entityHandle: i32) -> usize {
    unsafe { __s2sdk_GetWeaponVData.expect("GetWeaponVData function was not found")(entityHandle) }
}
pub type _GetWeaponVData = unsafe extern "C" fn(i32) -> usize;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetWeaponVData: Option<_GetWeaponVData> = None;
