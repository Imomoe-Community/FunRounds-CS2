// Generated from s2sdk.pplugin (group: transmit)
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

/// Gets all hidden entity handles for a player.
///
/// # Arguments
/// * `playerSlot` - (int32): The player slot to query.
///
/// # Returns - (int32[]):
/// Array of hidden entity handles.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetHiddenTransmitEntities(playerSlot: i32) -> Arr<i32> {
    unsafe { __s2sdk_GetHiddenTransmitEntities.expect("GetHiddenTransmitEntities function was not found")(playerSlot) }
}
pub type _GetHiddenTransmitEntities = unsafe extern "C" fn(i32) -> Arr<i32>;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetHiddenTransmitEntities: Option<_GetHiddenTransmitEntities> = None;

/// Hides a single entity from a player's transmit list.
///
/// # Arguments
/// * `playerSlot` - (int32): The player slot to hide the entity from.
/// * `entityHandle` - (int32): Entity handle to hide.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HideTransmitEntity(playerSlot: i32, entityHandle: i32) {
    unsafe { __s2sdk_HideTransmitEntity.expect("HideTransmitEntity function was not found")(playerSlot, entityHandle) }
}
pub type _HideTransmitEntity = unsafe extern "C" fn(i32, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_HideTransmitEntity: Option<_HideTransmitEntity> = None;

/// Shows a previously hidden entity to a player.
///
/// # Arguments
/// * `playerSlot` - (int32): The player slot to show the entity to.
/// * `entityHandle` - (int32): Entity handle to show.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn ShowTransmitEntity(playerSlot: i32, entityHandle: i32) {
    unsafe { __s2sdk_ShowTransmitEntity.expect("ShowTransmitEntity function was not found")(playerSlot, entityHandle) }
}
pub type _ShowTransmitEntity = unsafe extern "C" fn(i32, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_ShowTransmitEntity: Option<_ShowTransmitEntity> = None;
