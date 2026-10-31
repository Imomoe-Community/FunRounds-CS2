// Generated from s2sdk.pplugin
// SPDX-License-Identifier: AGPL-3.0-or-later

#[allow(unused_imports)]
use super::aliases::*;
#[allow(unused_imports)]
use super::enums::*;
#[allow(unused_imports)]
use plugify::{Arr, Mat4x4, Str, Var, Vec2, Vec3, Vec4};

/// Handles the execution of a command triggered by a caller. This function processes the command, interprets its context, and handles any provided arguments.
#[allow(unused)]
pub type ConCommandCallback = unsafe extern "C" fn(i32, ConCommandContext, &Arr<Str>) -> ResultType;

/// Handles events triggered by the game event system. This function processes the event data, determines the necessary action, and optionally prevents event broadcasting.
#[allow(unused)]
pub type EventCallback = unsafe extern "C" fn(&Str, usize, bool) -> ResultType;

/// Callback function for user messages.
#[allow(unused)]
pub type UserMessageCallback = unsafe extern "C" fn(usize) -> ResultType;

/// Called before server activation to build game session manifest.
#[allow(unused)]
pub type OnBuildGameSessionManifestCallback = unsafe extern "C" fn();

/// Called on every map end.
#[allow(unused)]
pub type OnMapEndCallback = unsafe extern "C" fn();
