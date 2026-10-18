// Generated from s2sdk.pplugin (group: console)

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

/// Sends a message to the server console.
///
/// # Arguments
/// * `msg` - (string): The message to be sent to the server console.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PrintToServer(msg: &Str) {
    unsafe { __s2sdk_PrintToServer.expect("PrintToServer function was not found")(msg) }
}
pub type _PrintToServer = unsafe extern "C" fn(&Str);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PrintToServer: Option<_PrintToServer> = None;

/// Prints a message to all clients in the chat area.
///
/// # Arguments
/// * `message` - (string): The message to be printed in the chat area for all clients.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PrintToChatAll(message: &Str) {
    unsafe { __s2sdk_PrintToChatAll.expect("PrintToChatAll function was not found")(message) }
}
pub type _PrintToChatAll = unsafe extern "C" fn(&Str);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PrintToChatAll: Option<_PrintToChatAll> = None;

/// Sends a reply message to a player or to the server console depending on the command context.
///
/// # Arguments
/// * `context` - (int32): The context from which the command was called (e.g., Console or Chat).
/// * `playerSlot` - (int32): The slot/index of the player receiving the message.
/// * `message` - (string): The message string to be sent as a reply.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn ReplyToCommand(context: ConCommandContext, playerSlot: i32, message: &Str) {
    unsafe { __s2sdk_ReplyToCommand.expect("ReplyToCommand function was not found")(context, playerSlot, message) }
}
pub type _ReplyToCommand = unsafe extern "C" fn(ConCommandContext, i32, &Str);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_ReplyToCommand: Option<_ReplyToCommand> = None;

/// Prints a message to all clients in the center of the screen.
///
/// # Arguments
/// * `message` - (string): The message to be printed in the center of the screen for all clients.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PrintCenterTextAll(message: &Str) {
    unsafe { __s2sdk_PrintCenterTextAll.expect("PrintCenterTextAll function was not found")(message) }
}
pub type _PrintCenterTextAll = unsafe extern "C" fn(&Str);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PrintCenterTextAll: Option<_PrintCenterTextAll> = None;

/// Prints a html message to all clients in the center of the screen.
///
/// # Arguments
/// * `message` - (string): The HTML-formatted message to be printed in the center of the screen for all clients.
/// * `duration` - (int32): The duration of the message in seconds.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PrintCentreHtmlAll(message: &Str, duration: i32) {
    unsafe { __s2sdk_PrintCentreHtmlAll.expect("PrintCentreHtmlAll function was not found")(message, duration) }
}
pub type _PrintCentreHtmlAll = unsafe extern "C" fn(&Str, i32);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PrintCentreHtmlAll: Option<_PrintCentreHtmlAll> = None;

/// Sends a message to every client's console.
///
/// # Arguments
/// * `message` - (string): The message to be sent to all clients' consoles.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PrintToConsoleAll(message: &Str) {
    unsafe { __s2sdk_PrintToConsoleAll.expect("PrintToConsoleAll function was not found")(message) }
}
pub type _PrintToConsoleAll = unsafe extern "C" fn(&Str);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PrintToConsoleAll: Option<_PrintToConsoleAll> = None;

/// Prints a message to a specific client in the chat area.
///
/// # Arguments
/// * `playerSlot` - (int32): The index of the player's slot to whom the message will be sent.
/// * `message` - (string): The message to be printed in the chat area.
#[track_caller]
#[allow(dead_code, non_snake_case)]
pub fn PrintToChat(playerSlot: i32, message: &Str) {
    unsafe { __s2sdk_PrintToChat.expect("PrintToChat function was not found")(playerSlot, message) }
}
pub type _PrintToChat = unsafe extern "C" fn(i32, &Str);
#[allow(dead_code, non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static mut __s2sdk_PrintToChat: Option<_PrintToChat> = None;
