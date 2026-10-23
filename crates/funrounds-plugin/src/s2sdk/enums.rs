// Generated from s2sdk.pplugin

#[allow(unused_imports)]
use plugify::vector_enum_traits;

/// Enum representing various flags for ConVars and ConCommands.
#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConVarFlag {
    /// The default, no flags at all.
    None = 0,
    /// Linked to a ConCommand.
    LinkedConcommand = 1,
    /// Hidden in released products. Automatically removed if ALLOW_DEVELOPMENT_CVARS is defined.
    DevelopmentOnly = 2,
    /// Defined by the game DLL.
    GameDll = 4,
    /// Defined by the client DLL.
    ClientDll = 8,
    /// Hidden. Doesn't appear in find or auto-complete. Like DEVELOPMENTONLY but cannot be compiled out.
    Hidden = 16,
    /// Server cvar; data is not sent since it's sensitive (e.g., passwords).
    Protected = 32,
    /// This cvar cannot be changed by clients connected to a multiplayer server.
    SpOnly = 64,
    /// Saved to vars.rc.
    Archive = 128,
    /// Notifies players when changed.
    Notify = 256,
    /// Changes the client's info string.
    UserInfo = 512,
    /// Hides the cvar from lookups.
    Missing0 = 1024,
    /// If this is a server cvar, changes are not logged to the file or console.
    Unlogged = 2048,
    /// Hides the cvar from lookups.
    Missing1 = 4096,
    /// Server-enforced setting on clients.
    Replicated = 8192,
    /// Only usable in singleplayer/debug or multiplayer with sv_cheats.
    Cheat = 16384,
    /// Causes auto-generated varnameN for splitscreen slots.
    PerUser = 32768,
    /// Records this cvar when starting a demo file.
    Demo = 65536,
    /// Excluded from demo files.
    DontRecord = 131072,
    /// Reserved for future use.
    Missing2 = 262144,
    /// Cvars tagged with this are available to customers.
    Release = 524288,
    /// Marks the cvar as a menu bar item.
    MenuBarItem = 1048576,
    /// Reserved for future use.
    Missing3 = 2097152,
    /// Cannot be changed by a client connected to a server.
    NotConnected = 4194304,
    /// Enables fuzzy matching for vconsole.
    VconsoleFuzzyMatching = 8388608,
    /// The server can execute this command on clients.
    ServerCanExecute = 16777216,
    /// Allows clients to execute this command.
    ClientCanExecute = 33554432,
    /// The server cannot query this cvar's value.
    ServerCannotQuery = 67108864,
    /// Sets focus in the vconsole.
    VconsoleSetFocus = 134217728,
    /// IVEngineClient::ClientCmd can execute this command.
    ClientCmdCanExecute = 268435456,
    /// Executes the cvar every tick.
    ExecutePerTick = 536870912,
}
vector_enum_traits!(ConVarFlag, i64);

/// Enum representing the possible results of an operation.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultType {
    /// The action continues to be processed without interruption.
    Continue = 0,
    /// Indicates that the action has altered the state or behavior during execution.
    Changed = 1,
    /// The action has been successfully handled, and no further action is required.
    Handled = 2,
    /// The action processing is halted, and no further steps will be executed.
    Stop = 3,
}
vector_enum_traits!(ResultType, i32);

/// The command execution context.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConCommandContext {
    /// The command execute from the client's console.
    Console = 0,
    /// The command execute from the client's chat.
    Chat = 1,
}
vector_enum_traits!(ConCommandContext, i32);

/// Enum representing the type of callback.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookMode {
    /// Callback will be executed before the original function
    Pre = 0,
    /// Callback will be executed after the original function
    Post = 1,
}
vector_enum_traits!(HookMode, u8);

/// Enum representing the type of callback.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventHookError {
    /// Indicates that the event hook was successfully created.
    Okay = 0,
    /// Indicates that the event name provided is invalid or does not exist.
    InvalidEvent = 1,
    /// Indicates that the event system is not currently active or initialized.
    NotActive = 2,
    /// Indicates that the callback function provided is invalid or not compatible with the event system.
    InvalidCallback = 3,
}
vector_enum_traits!(EventHookError, i32);

/// Enum representing the possible teams in Counter-Strike.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CSTeam {
    /// No team.
    None = 0,
    /// Spectator team.
    Spectator = 1,
    /// Terrorist team.
    T = 2,
    /// Counter-Terrorist team.
    CT = 3,
}
vector_enum_traits!(CSTeam, i32);

/// Enum representing the possible reasons for a round ending in Counter-Strike.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CSRoundEndReason {
    /// Target successfully bombed.
    TargetBombed = 1,
    /// The VIP has escaped (not present in CS:GO).
    VIPEscaped = 2,
    /// VIP has been assassinated (not present in CS:GO).
    VIPKilled = 3,
    /// The terrorists have escaped.
    TerroristsEscaped = 4,
    /// The CTs have prevented most of the terrorists from escaping.
    CTStoppedEscape = 5,
    /// Escaping terrorists have all been neutralized.
    TerroristsStopped = 6,
    /// The bomb has been defused.
    BombDefused = 7,
    /// Counter-Terrorists win.
    CTWin = 8,
    /// Terrorists win.
    TerroristWin = 9,
    /// Round draw.
    Draw = 10,
    /// All hostages have been rescued.
    HostagesRescued = 11,
    /// Target has been saved.
    TargetSaved = 12,
    /// Hostages have not been rescued.
    HostagesNotRescued = 13,
    /// Terrorists have not escaped.
    TerroristsNotEscaped = 14,
    /// VIP has not escaped (not present in CS:GO).
    VIPNotEscaped = 15,
    /// Game commencing.
    GameStart = 16,
    /// Terrorists surrender.
    TerroristsSurrender = 17,
    /// CTs surrender.
    CTSurrender = 18,
    /// Terrorists planted the bomb.
    TerroristsPlanted = 19,
    /// CTs reached the hostage.
    CTsReachedHostage = 20,
    /// Survival mode win.
    SurvivalWin = 21,
    /// Survival mode draw.
    SurvivalDraw = 22,
}
vector_enum_traits!(CSRoundEndReason, i32);

/// Enum representing various movement types for entities.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveType {
    /// Never moves.
    None = 0,
    /// Previously isometric movement type.
    Isometric = 1,
    /// Player only - moving on the ground.
    Walk = 2,
    /// No gravity, but still collides with stuff.
    Fly = 3,
    /// Flies through the air and is affected by gravity.
    Flygravity = 4,
    /// Uses VPHYSICS for simulation.
    Vphysics = 5,
    /// No clip to world, push and crush.
    Push = 6,
    /// No gravity, no collisions, still has velocity/avelocity.
    Noclip = 7,
    /// Used by players only when going onto a ladder.
    Ladder = 8,
    /// Observer movement, depends on player's observer mode.
    Observer = 9,
    /// Allows the entity to describe its own physics.
    Custom = 10,
}
vector_enum_traits!(MoveType, i32);

/// Represents the possible types of data that can be passed as a value in input actions.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// Automatically detect the type of the value.
    Auto = 0,
    /// A 32-bit floating-point number.
    Float32 = 1,
    /// A 64-bit floating-point number.
    Float64 = 2,
    /// A 32-bit signed integer.
    Int32 = 3,
    /// A 32-bit unsigned integer.
    UInt32 = 4,
    /// A 64-bit signed integer.
    Int64 = 5,
    /// A 64-bit unsigned integer.
    UInt64 = 6,
    /// A boolean value (true or false).
    Boolean = 7,
    /// A single character.
    Character = 8,
    /// A managed string object.
    String = 9,
    /// A null-terminated C-style string.
    CString = 10,
    /// A script handle, typically for scripting integration.
    HScript = 11,
    /// An entity handle, used to reference an entity within the system.
    EHandle = 12,
    /// A resource handle, such as a file or asset reference.
    Resource = 13,
    /// A 3D vector, typically representing position or direction.
    Vector3d = 14,
    /// A 2D vector, for planar data or coordinates.
    Vector2d = 15,
    /// A 4D vector, often used for advanced mathematical representations.
    Vector4d = 16,
    /// A 32-bit color value (RGBA).
    Color32 = 17,
    /// A quaternion-based angle representation.
    QAngle = 18,
    /// A quaternion, used for rotation and orientation calculations.
    Quaternion = 19,
}
vector_enum_traits!(FieldType, i32);

/// Ownership type for RAII wrappers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Borrowed,
    Owned,
}
