// Generated from s2sdk.pplugin (group: protobuf)

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
