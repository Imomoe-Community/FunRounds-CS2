// Generated from s2sdk.pplugin (group: protobuf)
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

/// Creates a UserMessage from a message ID.
///
/// # Arguments
/// * `messageId` - (int16): The ID of the message.
///
/// # Returns - (ptr64):
/// A pointer to the newly created UserMessage.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageCreateFromId(messageId: i16) -> usize {
    unsafe { __s2sdk_UserMessageCreateFromId.expect("UserMessageCreateFromId function was not found")(messageId) }
}
pub type _UserMessageCreateFromId = unsafe extern "C" fn(i16) -> usize;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageCreateFromId: Option<_UserMessageCreateFromId> = None;

/// Destroys a UserMessage and frees its memory.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage to destroy.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageDestroy(userMessage: usize) {
    unsafe { __s2sdk_UserMessageDestroy.expect("UserMessageDestroy function was not found")(userMessage) }
}
pub type _UserMessageDestroy = unsafe extern "C" fn(usize);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageDestroy: Option<_UserMessageDestroy> = None;

/// Sends a UserMessage to the specified recipients.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage to send.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageSend(userMessage: usize) {
    unsafe { __s2sdk_UserMessageSend.expect("UserMessageSend function was not found")(userMessage) }
}
pub type _UserMessageSend = unsafe extern "C" fn(usize);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageSend: Option<_UserMessageSend> = None;

/// Gets the ID of the message.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage instance.
///
/// # Returns - (int16):
/// The ID of the message.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageGetMessageID(userMessage: usize) -> i16 {
    unsafe { __s2sdk_UserMessageGetMessageID.expect("UserMessageGetMessageID function was not found")(userMessage) }
}
pub type _UserMessageGetMessageID = unsafe extern "C" fn(usize) -> i16;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageGetMessageID: Option<_UserMessageGetMessageID> = None;

/// Finds a message ID by its name.
///
/// # Arguments
/// * `messageName` - (string): The name of the message.
///
/// # Returns - (int16):
/// The ID of the message, or 0 if the message was not found.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageFindMessageIdByName(messageName: &Str) -> i16 {
    unsafe { __s2sdk_UserMessageFindMessageIdByName.expect("UserMessageFindMessageIdByName function was not found")(messageName) }
}
pub type _UserMessageFindMessageIdByName = unsafe extern "C" fn(&Str) -> i16;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageFindMessageIdByName: Option<_UserMessageFindMessageIdByName> = None;

/// Adds a single recipient (player) to the UserMessage.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage instance.
/// * `playerSlot` - (int32): The slot index of the player to add as a recipient.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageAddRecipient(userMessage: usize, playerSlot: i32) {
    unsafe { __s2sdk_UserMessageAddRecipient.expect("UserMessageAddRecipient function was not found")(userMessage, playerSlot) }
}
pub type _UserMessageAddRecipient = unsafe extern "C" fn(usize, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageAddRecipient: Option<_UserMessageAddRecipient> = None;

/// Sets an unsigned 32-bit integer value for a field in the UserMessage.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage instance.
/// * `fieldName` - (string): The name of the field.
/// * `value` - (uint32): The value to set.
///
/// # Returns - (bool):
/// True if the field was successfully set, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PbSetUInt32(userMessage: usize, fieldName: &Str, value: u32) -> bool {
    unsafe { __s2sdk_PbSetUInt32.expect("PbSetUInt32 function was not found")(userMessage, fieldName, value) }
}
pub type _PbSetUInt32 = unsafe extern "C" fn(usize, &Str, u32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PbSetUInt32: Option<_PbSetUInt32> = None;

/// Sets a float value for a field in the UserMessage.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage instance.
/// * `fieldName` - (string): The name of the field.
/// * `value` - (float): The value to set.
///
/// # Returns - (bool):
/// True if the field was successfully set, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PbSetFloat(userMessage: usize, fieldName: &Str, value: f32) -> bool {
    unsafe { __s2sdk_PbSetFloat.expect("PbSetFloat function was not found")(userMessage, fieldName, value) }
}
pub type _PbSetFloat = unsafe extern "C" fn(usize, &Str, f32) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PbSetFloat: Option<_PbSetFloat> = None;

/// Hooks a user message with a callback.
///
/// # Arguments
/// * `messageId` - (int16): The ID of the message to hook.
/// * `callback` - (function): The callback function to invoke when the message is received.
/// * `mode` - (uint8): Whether to hook the message in the post mode (after processing) or pre mode (before processing).
///
/// # Returns - (bool):
/// True if the hook was successfully added, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn HookUserMessage(messageId: i16, callback: UserMessageCallback, mode: HookMode) -> bool {
    unsafe { __s2sdk_HookUserMessage.expect("HookUserMessage function was not found")(messageId, callback, mode) }
}
pub type _HookUserMessage = unsafe extern "C" fn(i16, UserMessageCallback, HookMode) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_HookUserMessage: Option<_HookUserMessage> = None;

/// Unhooks a previously hooked user message.
///
/// # Arguments
/// * `messageId` - (int16): The ID of the message to unhook.
/// * `callback` - (function): The callback function to remove.
/// * `mode` - (uint8): Whether the hook was in post mode (after processing) or pre mode (before processing).
///
/// # Returns - (bool):
/// True if the hook was successfully removed, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UnhookUserMessage(messageId: i16, callback: UserMessageCallback, mode: HookMode) -> bool {
    unsafe { __s2sdk_UnhookUserMessage.expect("UnhookUserMessage function was not found")(messageId, callback, mode) }
}
pub type _UnhookUserMessage = unsafe extern "C" fn(i16, UserMessageCallback, HookMode) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UnhookUserMessage: Option<_UnhookUserMessage> = None;

/// Creates a UserMessage from a message name.
///
/// # Arguments
/// * `messageName` - (string): The name of the message.
///
/// # Returns - (ptr64):
/// A pointer to the newly created UserMessage.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageCreateFromName(messageName: &Str) -> usize {
    unsafe { __s2sdk_UserMessageCreateFromName.expect("UserMessageCreateFromName function was not found")(messageName) }
}
pub type _UserMessageCreateFromName = unsafe extern "C" fn(&Str) -> usize;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageCreateFromName: Option<_UserMessageCreateFromName> = None;

/// Gets the name of the message.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage instance.
///
/// # Returns - (string):
/// The name of the message as a string.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageGetMessageName(userMessage: usize) -> Str {
    unsafe { __s2sdk_UserMessageGetMessageName.expect("UserMessageGetMessageName function was not found")(userMessage) }
}
pub type _UserMessageGetMessageName = unsafe extern "C" fn(usize) -> Str;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageGetMessageName: Option<_UserMessageGetMessageName> = None;

/// Adds all connected players as recipients to the UserMessage.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage instance.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageAddAllPlayers(userMessage: usize) {
    unsafe { __s2sdk_UserMessageAddAllPlayers.expect("UserMessageAddAllPlayers function was not found")(userMessage) }
}
pub type _UserMessageAddAllPlayers = unsafe extern "C" fn(usize);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageAddAllPlayers: Option<_UserMessageAddAllPlayers> = None;

/// Gets the count of repeated fields in a field of the UserMessage.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage instance.
/// * `fieldName` - (string): The name of the field.
///
/// # Returns - (int32):
/// The count of repeated fields, or -1 if the field is not repeated or does not exist.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn UserMessageGetRepeatedFieldCount(userMessage: usize, fieldName: &Str) -> i32 {
    unsafe { __s2sdk_UserMessageGetRepeatedFieldCount.expect("UserMessageGetRepeatedFieldCount function was not found")(userMessage, fieldName) }
}
pub type _UserMessageGetRepeatedFieldCount = unsafe extern "C" fn(usize, &Str) -> i32;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_UserMessageGetRepeatedFieldCount: Option<_UserMessageGetRepeatedFieldCount> = None;

/// Gets a repeated string value from a field in the UserMessage.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage instance.
/// * `fieldName` - (string): The name of the field.
/// * `index` - (int32): The index of the repeated field.
/// * `out` - (string&): The output string.
///
/// # Returns - (bool):
/// True if the field was successfully retrieved, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PbGetRepeatedString(userMessage: usize, fieldName: &Str, index: i32, out: &mut Str) -> bool {
    unsafe { __s2sdk_PbGetRepeatedString.expect("PbGetRepeatedString function was not found")(userMessage, fieldName, index, out) }
}
pub type _PbGetRepeatedString = unsafe extern "C" fn(usize, &Str, i32, &mut Str) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PbGetRepeatedString: Option<_PbGetRepeatedString> = None;

/// Adds a string value to a repeated field in the UserMessage.
///
/// # Arguments
/// * `userMessage` - (ptr64): The UserMessage instance.
/// * `fieldName` - (string): The name of the field.
/// * `value` - (string): The value to add.
///
/// # Returns - (bool):
/// True if the value was successfully added, false otherwise.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PbAddString(userMessage: usize, fieldName: &Str, value: &Str) -> bool {
    unsafe { __s2sdk_PbAddString.expect("PbAddString function was not found")(userMessage, fieldName, value) }
}
pub type _PbAddString = unsafe extern "C" fn(usize, &Str, &Str) -> bool;
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PbAddString: Option<_PbAddString> = None;
