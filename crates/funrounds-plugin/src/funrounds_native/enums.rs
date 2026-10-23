// Generated from funrounds_native.pplugin

#[allow(unused_imports)]
use plugify::vector_enum_traits;

/// Ownership type for RAII wrappers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Borrowed,
    Owned,
}
