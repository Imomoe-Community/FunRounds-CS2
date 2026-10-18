// Generated from s2sdk.pplugin (group: cvars)

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

/// Searches for a console variable.
///
/// # Arguments
/// * `name` - (string): The name of the console variable to search for.
///
/// # Returns - (uint64):
/// A handle to the console variable data if found; otherwise, nullptr.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn FindConVar(name: &Str) -> u64 {
    unsafe { __s2sdk_FindConVar.expect("FindConVar function was not found")(name) }
}
pub type _FindConVar = unsafe extern "C" fn(&Str) -> u64;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_FindConVar: Option<_FindConVar> = None;

/// Retrieves the current value of a console variable and stores it in the output string.
///
/// # Arguments
/// * `conVarHandle` - (uint64): The handle to the console variable data.
///
/// # Returns - (string):
/// The output value in string format.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetConVarValue(conVarHandle: u64) -> Str {
    unsafe { __s2sdk_GetConVarValue.expect("GetConVarValue function was not found")(conVarHandle) }
}
pub type _GetConVarValue = unsafe extern "C" fn(u64) -> Str;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetConVarValue: Option<_GetConVarValue> = None;

/// Sets the value of a console variable.
///
/// # Arguments
/// * `conVarHandle` - (uint64): The handle to the console variable data.
/// * `value` - (string): The string value to set for the console variable.
/// * `replicate` - (bool): If set to true, the new convar value will be set on all clients. This will only work if the convar has the FCVAR_REPLICATED flag and actually exists on clients.
/// * `notify` - (bool): If set to true, clients will be notified that the convar has changed. This will only work if the convar has the FCVAR_NOTIFY flag.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetConVarValue(conVarHandle: u64, value: &Str, replicate: bool, notify: bool) {
    unsafe { __s2sdk_SetConVarValue.expect("SetConVarValue function was not found")(conVarHandle, value, replicate, notify) }
}
pub type _SetConVarValue = unsafe extern "C" fn(u64, &Str, bool, bool);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetConVarValue: Option<_SetConVarValue> = None;
