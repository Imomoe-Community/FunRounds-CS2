// Generated from s2sdk.pplugin (group: events)
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

/// Creates a hook for when a game event is fired.
///
/// # Arguments
/// * `name` - (string): The name of the event to hook.
/// * `callback` - (function): The callback function to call when the event is fired.
/// * `type_` - (uint8): Whether the hook was in post mode (after processing) or pre mode (before processing).
///
/// # Returns - (int32):
/// An integer indicating the result of the hook operation.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HookEvent(name: &Str, callback: EventCallback, type_: HookMode) -> EventHookError {
    unsafe { __s2sdk_HookEvent.expect("HookEvent function was not found")(name, callback, type_) }
}
pub type _HookEvent = unsafe extern "C" fn(&Str, EventCallback, HookMode) -> EventHookError;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_HookEvent: Option<_HookEvent> = None;

/// Removes a hook for when a game event is fired.
///
/// # Arguments
/// * `name` - (string): The name of the event to unhook.
/// * `callback` - (function): The callback function to remove.
/// * `type_` - (uint8): Whether the hook was in post mode (after processing) or pre mode (before processing).
///
/// # Returns - (int32):
/// An integer indicating the result of the unhook operation.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UnhookEvent(name: &Str, callback: EventCallback, type_: HookMode) -> EventHookError {
    unsafe { __s2sdk_UnhookEvent.expect("UnhookEvent function was not found")(name, callback, type_) }
}
pub type _UnhookEvent = unsafe extern "C" fn(&Str, EventCallback, HookMode) -> EventHookError;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UnhookEvent: Option<_UnhookEvent> = None;

/// Retrieves the boolean value of a game event's key.
///
/// # Arguments
/// * `event` - (ptr64): A pointer to the IGameEvent object containing event data.
/// * `key` - (string): The key for which to retrieve the boolean value.
///
/// # Returns - (bool):
/// The boolean value associated with the key.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEventBool(event: usize, key: &Str) -> bool {
    unsafe { __s2sdk_GetEventBool.expect("GetEventBool function was not found")(event, key) }
}
pub type _GetEventBool = unsafe extern "C" fn(usize, &Str) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEventBool: Option<_GetEventBool> = None;

/// Retrieves the integer value of a game event's key.
///
/// # Arguments
/// * `event` - (ptr64): A pointer to the IGameEvent object containing event data.
/// * `key` - (string): The key for which to retrieve the integer value.
///
/// # Returns - (int32):
/// The integer value associated with the key.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEventInt(event: usize, key: &Str) -> i32 {
    unsafe { __s2sdk_GetEventInt.expect("GetEventInt function was not found")(event, key) }
}
pub type _GetEventInt = unsafe extern "C" fn(usize, &Str) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEventInt: Option<_GetEventInt> = None;

/// Retrieves the player slot of a game event's key.
///
/// # Arguments
/// * `event` - (ptr64): A pointer to the IGameEvent object containing event data.
/// * `key` - (string): The key for which to retrieve the player index.
///
/// # Returns - (int32):
/// The player slot associated with the key.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEventPlayerSlot(event: usize, key: &Str) -> i32 {
    unsafe { __s2sdk_GetEventPlayerSlot.expect("GetEventPlayerSlot function was not found")(event, key) }
}
pub type _GetEventPlayerSlot = unsafe extern "C" fn(usize, &Str) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEventPlayerSlot: Option<_GetEventPlayerSlot> = None;

/// Retrieves the string value of a game event's key.
///
/// # Arguments
/// * `event` - (ptr64): A pointer to the IGameEvent object containing event data.
/// * `key` - (string): The key for which to retrieve the string value.
///
/// # Returns - (string):
/// A string where the result will be stored.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetEventString(event: usize, key: &Str) -> Str {
    unsafe { __s2sdk_GetEventString.expect("GetEventString function was not found")(event, key) }
}
pub type _GetEventString = unsafe extern "C" fn(usize, &Str) -> Str;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetEventString: Option<_GetEventString> = None;

/// Sets the string value of a game event's key.
///
/// # Arguments
/// * `event` - (ptr64): A pointer to the IGameEvent object containing event data.
/// * `key` - (string): The key for which to set the string value.
/// * `value` - (string): The string value to set.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetEventString(event: usize, key: &Str, value: &Str) {
    unsafe { __s2sdk_SetEventString.expect("SetEventString function was not found")(event, key, value) }
}
pub type _SetEventString = unsafe extern "C" fn(usize, &Str, &Str);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetEventString: Option<_SetEventString> = None;
