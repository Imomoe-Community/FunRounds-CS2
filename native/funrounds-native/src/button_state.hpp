// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstdint>

namespace funrounds::native::detail {

// SourceSDK CInButtonState::GetButtonState encodes DOWN_UP (state 2) as group 1 alone;
// IsButtonPressed(..., onlyDown) uses group 0 for the currently held state.
enum class ButtonStateGroup : std::int32_t {
    CurrentDown = 0,
    Changed = 1,
    Scroll = 2,
};

constexpr bool IsButtonCurrentlyDown(std::uint64_t buttonBits, std::uint64_t buttonMask) noexcept {
    return (buttonBits & buttonMask) != 0;
}

} // namespace funrounds::native::detail
