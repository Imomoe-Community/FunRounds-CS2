// Generated from s2sdk.pplugin (group: clients)
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

/// Returns the entity handle associated with a player slot index.
///
/// # Arguments
/// * `playerSlot` - (int32): Index of the player slot.
///
/// # Returns - (int32):
/// The index of the entity, or -1 if the handle is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PlayerSlotToEntHandle(playerSlot: i32) -> i32 {
    unsafe { __s2sdk_PlayerSlotToEntHandle.expect("PlayerSlotToEntHandle function was not found")(playerSlot) }
}
pub type _PlayerSlotToEntHandle = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PlayerSlotToEntHandle: Option<_PlayerSlotToEntHandle> = None;

/// Checks if a certain player has entered the game.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (bool):
/// true if the player is in the game, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn IsClientInGame(playerSlot: i32) -> bool {
    unsafe { __s2sdk_IsClientInGame.expect("IsClientInGame function was not found")(playerSlot) }
}
pub type _IsClientInGame = unsafe extern "C" fn(i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_IsClientInGame: Option<_IsClientInGame> = None;

/// Checks if the client is alive or dead.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (bool):
/// true if the client is alive, false if dead.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn IsClientAlive(playerSlot: i32) -> bool {
    unsafe { __s2sdk_IsClientAlive.expect("IsClientAlive function was not found")(playerSlot) }
}
pub type _IsClientAlive = unsafe extern "C" fn(i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_IsClientAlive: Option<_IsClientAlive> = None;

/// Retrieves the gravity scale of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose gravity scale is to be retrieved.
///
/// # Returns - (float):
/// The gravity scale of the client, or 0.0f if the client is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientGravity(playerSlot: i32) -> f32 {
    unsafe { __s2sdk_GetClientGravity.expect("GetClientGravity function was not found")(playerSlot) }
}
pub type _GetClientGravity = unsafe extern "C" fn(i32) -> f32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientGravity: Option<_GetClientGravity> = None;

/// Sets the gravity scale of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose gravity scale is to be set.
/// * `gravity` - (float): The new gravity scale to set for the client.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetClientGravity(playerSlot: i32, gravity: f32) {
    unsafe { __s2sdk_SetClientGravity.expect("SetClientGravity function was not found")(playerSlot, gravity) }
}
pub type _SetClientGravity = unsafe extern "C" fn(i32, f32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetClientGravity: Option<_SetClientGravity> = None;

/// Retrieves the health of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose health is to be retrieved.
///
/// # Returns - (int32):
/// The health of the client, or 0 if the client is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientHealth(playerSlot: i32) -> i32 {
    unsafe { __s2sdk_GetClientHealth.expect("GetClientHealth function was not found")(playerSlot) }
}
pub type _GetClientHealth = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientHealth: Option<_GetClientHealth> = None;

/// Sets the health of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose health is to be set.
/// * `health` - (int32): The new health value to set for the client.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetClientHealth(playerSlot: i32, health: i32) {
    unsafe { __s2sdk_SetClientHealth.expect("SetClientHealth function was not found")(playerSlot, health) }
}
pub type _SetClientHealth = unsafe extern "C" fn(i32, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetClientHealth: Option<_SetClientHealth> = None;

/// Retrieves the pawn entity pointer associated with a client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (ptr64):
/// A pointer to the client's pawn entity, or nullptr if the client or controller is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientPawn(playerSlot: i32) -> usize {
    unsafe { __s2sdk_GetClientPawn.expect("GetClientPawn function was not found")(playerSlot) }
}
pub type _GetClientPawn = unsafe extern "C" fn(i32) -> usize;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientPawn: Option<_GetClientPawn> = None;

/// Retrieves the handle of the client's currently active weapon.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (int32):
/// The entity handle of the active weapon, or INVALID_EHANDLE_INDEX if the client is invalid or has no active weapon.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientActiveWeapon(playerSlot: i32) -> i32 {
    unsafe { __s2sdk_GetClientActiveWeapon.expect("GetClientActiveWeapon function was not found")(playerSlot) }
}
pub type _GetClientActiveWeapon = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientActiveWeapon: Option<_GetClientActiveWeapon> = None;

/// Retrieves a list of weapon handles owned by the client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (int32[]):
/// A vector of entity handles for the client's weapons, or an empty vector if the client is invalid or has no weapons.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientWeapons(playerSlot: i32) -> Arr<i32> {
    unsafe { __s2sdk_GetClientWeapons.expect("GetClientWeapons function was not found")(playerSlot) }
}
pub type _GetClientWeapons = unsafe extern "C" fn(i32) -> Arr<i32>;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientWeapons: Option<_GetClientWeapons> = None;

/// Removes a player's weapon.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
/// * `weaponHandle` - (int32): The handle of weapon to remove.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn RemoveWeapon(playerSlot: i32, weaponHandle: i32) {
    unsafe { __s2sdk_RemoveWeapon.expect("RemoveWeapon function was not found")(playerSlot, weaponHandle) }
}
pub type _RemoveWeapon = unsafe extern "C" fn(i32, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_RemoveWeapon: Option<_RemoveWeapon> = None;

/// Gives a named item (e.g., weapon) to a client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
/// * `itemName` - (string): The name of the item to give.
///
/// # Returns - (int32):
/// The entity handle of the created item, or INVALID_EHANDLE_INDEX if the client or item is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GiveNamedItem(playerSlot: i32, itemName: &Str) -> i32 {
    unsafe { __s2sdk_GiveNamedItem.expect("GiveNamedItem function was not found")(playerSlot, itemName) }
}
pub type _GiveNamedItem = unsafe extern "C" fn(i32, &Str) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GiveNamedItem: Option<_GiveNamedItem> = None;

/// Returns the client's armor value.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (int32):
/// The armor value of the client.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientArmor(playerSlot: i32) -> i32 {
    unsafe { __s2sdk_GetClientArmor.expect("GetClientArmor function was not found")(playerSlot) }
}
pub type _GetClientArmor = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientArmor: Option<_GetClientArmor> = None;

/// Sets the client's armor value.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
/// * `armor` - (int32): The armor value to set.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetClientArmor(playerSlot: i32, armor: i32) {
    unsafe { __s2sdk_SetClientArmor.expect("SetClientArmor function was not found")(playerSlot, armor) }
}
pub type _SetClientArmor = unsafe extern "C" fn(i32, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetClientArmor: Option<_SetClientArmor> = None;

/// Retrieves the team number of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose team number is to be retrieved.
///
/// # Returns - (int32):
/// The team number of the client, or 0 if the client is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientTeam(playerSlot: i32) -> CSTeam {
    unsafe { __s2sdk_GetClientTeam.expect("GetClientTeam function was not found")(playerSlot) }
}
pub type _GetClientTeam = unsafe extern "C" fn(i32) -> CSTeam;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientTeam: Option<_GetClientTeam> = None;

/// Retrieves the render color of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose render color is to be retrieved.
///
/// # Returns - (vec4):
/// The raw color value of the client's render color, or 0 if the client is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientRenderColor(playerSlot: i32) -> Vec4 {
    unsafe { __s2sdk_GetClientRenderColor.expect("GetClientRenderColor function was not found")(playerSlot) }
}
pub type _GetClientRenderColor = unsafe extern "C" fn(i32) -> Vec4;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientRenderColor: Option<_GetClientRenderColor> = None;

/// Sets the render color of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose render color is to be set.
/// * `color` - (vec4): The new raw color value to set for the client's render color.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetClientRenderColor(playerSlot: i32, color: &Vec4) {
    unsafe { __s2sdk_SetClientRenderColor.expect("SetClientRenderColor function was not found")(playerSlot, color) }
}
pub type _SetClientRenderColor = unsafe extern "C" fn(i32, &Vec4);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetClientRenderColor: Option<_SetClientRenderColor> = None;

/// Respawns a player.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot to respawn.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn RespawnClient(playerSlot: i32) {
    unsafe { __s2sdk_RespawnClient.expect("RespawnClient function was not found")(playerSlot) }
}
pub type _RespawnClient = unsafe extern "C" fn(i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_RespawnClient: Option<_RespawnClient> = None;

/// Forces a full engine respawn for a player, including when the current pawn is alive. Supported in CS2.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot to respawn.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn ForceRespawnClient(playerSlot: i32) {
    unsafe { __s2sdk_ForceRespawnClient.expect("ForceRespawnClient function was not found")(playerSlot) }
}
pub type _ForceRespawnClient = unsafe extern "C" fn(i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_ForceRespawnClient: Option<_ForceRespawnClient> = None;

/// Returns the client's speed value.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (float):
/// The speed value of the client.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientSpeed(playerSlot: i32) -> f32 {
    unsafe { __s2sdk_GetClientSpeed.expect("GetClientSpeed function was not found")(playerSlot) }
}
pub type _GetClientSpeed = unsafe extern "C" fn(i32) -> f32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientSpeed: Option<_GetClientSpeed> = None;

/// Sets the client's speed value.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
/// * `speed` - (float): The speed value to set.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetClientSpeed(playerSlot: i32, speed: f32) {
    unsafe { __s2sdk_SetClientSpeed.expect("SetClientSpeed function was not found")(playerSlot, speed) }
}
pub type _SetClientSpeed = unsafe extern "C" fn(i32, f32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetClientSpeed: Option<_SetClientSpeed> = None;

/// Retrieves the absolute velocity of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose absolute velocity is to be retrieved.
///
/// # Returns - (vec3):
/// A vector where the absolute velocity will be stored.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientAbsVelocity(playerSlot: i32) -> Vec3 {
    unsafe { __s2sdk_GetClientAbsVelocity.expect("GetClientAbsVelocity function was not found")(playerSlot) }
}
pub type _GetClientAbsVelocity = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientAbsVelocity: Option<_GetClientAbsVelocity> = None;

/// Sets the absolute velocity of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose absolute velocity is to be set.
/// * `velocity` - (vec3): The new absolute velocity to set for the client.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SetClientAbsVelocity(playerSlot: i32, velocity: &Vec3) {
    unsafe { __s2sdk_SetClientAbsVelocity.expect("SetClientAbsVelocity function was not found")(playerSlot, velocity) }
}
pub type _SetClientAbsVelocity = unsafe extern "C" fn(i32, &Vec3);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SetClientAbsVelocity: Option<_SetClientAbsVelocity> = None;

/// Get the qangles that this client is looking at.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot
///
/// # Returns - (vec3):
/// Eye angles as a vector (pitch, yaw, roll)
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientEyeAngles(playerSlot: i32) -> Vec3 {
    unsafe { __s2sdk_GetClientEyeAngles.expect("GetClientEyeAngles function was not found")(playerSlot) }
}
pub type _GetClientEyeAngles = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientEyeAngles: Option<_GetClientEyeAngles> = None;

/// Retrieves the state of a specific button for a client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
/// * `buttonIndex` - (int32): The index of the button (0-2).
///
/// # Returns - (uint64):
/// uint64_t The state of the specified button, or 0 if the client or button index is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientButtons(playerSlot: i32, buttonIndex: i32) -> u64 {
    unsafe { __s2sdk_GetClientButtons.expect("GetClientButtons function was not found")(playerSlot, buttonIndex) }
}
pub type _GetClientButtons = unsafe extern "C" fn(i32, i32) -> u64;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientButtons: Option<_GetClientButtons> = None;

/// Retrieves the flags of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose flags are to be retrieved.
///
/// # Returns - (int32):
/// The flags of the client, or 0 if the client is invalid.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientFlags(playerSlot: i32) -> i32 {
    unsafe { __s2sdk_GetClientFlags.expect("GetClientFlags function was not found")(playerSlot) }
}
pub type _GetClientFlags = unsafe extern "C" fn(i32) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientFlags: Option<_GetClientFlags> = None;

/// Checks if a certain player is a fake client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (bool):
/// true if the client is a fake client, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn IsFakeClient(playerSlot: i32) -> bool {
    unsafe { __s2sdk_IsFakeClient.expect("IsFakeClient function was not found")(playerSlot) }
}
pub type _IsFakeClient = unsafe extern "C" fn(i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_IsFakeClient: Option<_IsFakeClient> = None;

/// Retrieves the absolute origin of an client.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot whose absolute origin is to be retrieved.
///
/// # Returns - (vec3):
/// A vector where the absolute origin will be stored.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientAbsOrigin(playerSlot: i32) -> Vec3 {
    unsafe { __s2sdk_GetClientAbsOrigin.expect("GetClientAbsOrigin function was not found")(playerSlot) }
}
pub type _GetClientAbsOrigin = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientAbsOrigin: Option<_GetClientAbsOrigin> = None;

/// Get vector to eye position - absolute coords.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot
///
/// # Returns - (vec3):
/// Eye position in absolute/world coordinates
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientEyePosition(playerSlot: i32) -> Vec3 {
    unsafe { __s2sdk_GetClientEyePosition.expect("GetClientEyePosition function was not found")(playerSlot) }
}
pub type _GetClientEyePosition = unsafe extern "C" fn(i32) -> Vec3;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientEyePosition: Option<_GetClientEyePosition> = None;

/// Forces a player to drop their weapon.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
/// * `weaponHandle` - (int32): The handle of weapon to drop.
/// * `target` - (vec3): Target direction.
/// * `velocity` - (vec3): Velocity to toss weapon or zero to just drop weapon.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn DropWeapon(playerSlot: i32, weaponHandle: i32, target: &Vec3, velocity: &Vec3) {
    unsafe { __s2sdk_DropWeapon.expect("DropWeapon function was not found")(playerSlot, weaponHandle, target, velocity) }
}
pub type _DropWeapon = unsafe extern "C" fn(i32, i32, &Vec3, &Vec3);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_DropWeapon: Option<_DropWeapon> = None;

/// Selects a player's weapon.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
/// * `weaponHandle` - (int32): The handle of weapon to bump.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SelectWeapon(playerSlot: i32, weaponHandle: i32) {
    unsafe { __s2sdk_SelectWeapon.expect("SelectWeapon function was not found")(playerSlot, weaponHandle) }
}
pub type _SelectWeapon = unsafe extern "C" fn(i32, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SelectWeapon: Option<_SelectWeapon> = None;

/// Switches a player's weapon.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
/// * `weaponHandle` - (int32): The handle of weapon to switch.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn SwitchWeapon(playerSlot: i32, weaponHandle: i32) {
    unsafe { __s2sdk_SwitchWeapon.expect("SwitchWeapon function was not found")(playerSlot, weaponHandle) }
}
pub type _SwitchWeapon = unsafe extern "C" fn(i32, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_SwitchWeapon: Option<_SwitchWeapon> = None;

/// Checks if a certain player has been authenticated.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (bool):
/// true if the player is authenticated, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn IsClientAuthorized(playerSlot: i32) -> bool {
    unsafe { __s2sdk_IsClientAuthorized.expect("IsClientAuthorized function was not found")(playerSlot) }
}
pub type _IsClientAuthorized = unsafe extern "C" fn(i32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_IsClientAuthorized: Option<_IsClientAuthorized> = None;

/// Returns the client's SteamID64 - a unique 64-bit identifier of a Steam account.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (uint64):
/// uint64_t The client's SteamID64.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientSteamID64(playerSlot: i32) -> u64 {
    unsafe { __s2sdk_GetClientSteamID64.expect("GetClientSteamID64 function was not found")(playerSlot) }
}
pub type _GetClientSteamID64 = unsafe extern "C" fn(i32) -> u64;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientSteamID64: Option<_GetClientSteamID64> = None;

/// Retrieves a client's language as an ISO code. Unlike GetClientLanguage, which returns the raw cl_language value ("english", "schinese"), this returns the code translation files are keyed by ("en", "zh-CN"). Empty until the client answers the cl_language query, and always empty for bots. Reliably available starting from OnClientAuthenticated and OnClientPutInServer.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot.
///
/// # Returns - (string):
/// The client's ISO language code.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn GetClientLanguageCode(playerSlot: i32) -> Str {
    unsafe { __s2sdk_GetClientLanguageCode.expect("GetClientLanguageCode function was not found")(playerSlot) }
}
pub type _GetClientLanguageCode = unsafe extern "C" fn(i32) -> Str;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_GetClientLanguageCode: Option<_GetClientLanguageCode> = None;
