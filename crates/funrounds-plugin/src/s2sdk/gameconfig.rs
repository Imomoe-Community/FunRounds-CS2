// Generated from s2sdk.pplugin (group: gameconfig)
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

/// Loads a game configuration file.
///
/// # Arguments
/// * `paths` - (string[]): The paths to the game configuration file to be loaded.
///
/// # Returns - (uint32):
/// A id to the loaded game configuration object, or 0 if loading fails.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn LoadGameConfigFile(paths: &Arr<Str>) -> ConfigId {
    unsafe { __s2sdk_LoadGameConfigFile.expect("LoadGameConfigFile function was not found")(paths) }
}
pub type _LoadGameConfigFile = unsafe extern "C" fn(&Arr<Str>) -> ConfigId;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_LoadGameConfigFile: Option<_LoadGameConfigFile> = None;

/// Closes a game configuration file.
///
/// # Arguments
/// * `id` - (uint32): An id to the game configuration to be closed.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn CloseGameConfigFile(id: ConfigId) {
    unsafe { __s2sdk_CloseGameConfigFile.expect("CloseGameConfigFile function was not found")(id) }
}
pub type _CloseGameConfigFile = unsafe extern "C" fn(ConfigId);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_CloseGameConfigFile: Option<_CloseGameConfigFile> = None;

/// Retrieves the offset associated with a name from the game configuration.
///
/// # Arguments
/// * `id` - (uint32): An id to the game configuration from which to retrieve the offset.
/// * `name` - (string): The name whose offset is to be retrieved.
///
/// # Returns - (int32):
/// The offset associated with the specified name.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetGameConfigOffset(id: ConfigId, name: &Str) -> i32 {
    unsafe { __s2sdk_GetGameConfigOffset.expect("GetGameConfigOffset function was not found")(id, name) }
}
pub type _GetGameConfigOffset = unsafe extern "C" fn(ConfigId, &Str) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetGameConfigOffset: Option<_GetGameConfigOffset> = None;

/// Retrieves the signature associated with a name from the game configuration.
///
/// # Arguments
/// * `id` - (uint32): An id to the game configuration from which to retrieve the signature.
/// * `name` - (string): The name whose signature is to be resolved and retrieved.
///
/// # Returns - (ptr64):
/// A pointer to the signature associated with the specified name.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetGameConfigSignature(id: ConfigId, name: &Str) -> usize {
    unsafe { __s2sdk_GetGameConfigSignature.expect("GetGameConfigSignature function was not found")(id, name) }
}
pub type _GetGameConfigSignature = unsafe extern "C" fn(ConfigId, &Str) -> usize;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetGameConfigSignature: Option<_GetGameConfigSignature> = None;

/// Retrieves a vtable associated with the game configuration.
///
/// # Arguments
/// * `id` - (uint32): An id to the game configuration from which to retrieve the vtable.
/// * `name` - (string): The name of the vtable to be retrieved.
///
/// # Returns - (ptr64):
/// A pointer to the vtable associated with the specified name
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetGameConfigVTable(id: ConfigId, name: &Str) -> usize {
    unsafe { __s2sdk_GetGameConfigVTable.expect("GetGameConfigVTable function was not found")(id, name) }
}
pub type _GetGameConfigVTable = unsafe extern "C" fn(ConfigId, &Str) -> usize;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetGameConfigVTable: Option<_GetGameConfigVTable> = None;
