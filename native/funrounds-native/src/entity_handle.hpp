// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <bit>
#include <cstdint>

namespace funrounds::native {

constexpr std::uint32_t ToNativeHandle(std::int32_t handle) noexcept {
    return std::bit_cast<std::uint32_t>(handle);
}

constexpr std::int32_t ToApiHandle(std::uint32_t handle) noexcept {
    return std::bit_cast<std::int32_t>(handle);
}

constexpr bool MatchesHandle(std::uint32_t nativeHandle, std::int32_t apiHandle) noexcept {
    return nativeHandle == ToNativeHandle(apiHandle);
}

} // namespace funrounds::native
