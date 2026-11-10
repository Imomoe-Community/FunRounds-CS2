// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <bit>
#include <cmath>
#include <cstdint>

namespace funrounds::native::bighead {

constexpr std::uint32_t kBigHeadMutatorBit = 1U;

struct MutatorBitLease {
    bool originallySet{};
    bool owned{};
};

constexpr MutatorBitLease BeginMutatorBitLease(std::uint32_t current) noexcept {
    const bool originallySet = (current & kBigHeadMutatorBit) != 0;
    return {originallySet, !originallySet};
}

constexpr std::uint32_t EnableOwnedMutatorBit(std::uint32_t current, MutatorBitLease lease) noexcept {
    return lease.owned ? current | kBigHeadMutatorBit : current;
}

constexpr std::uint32_t RestoreOwnedMutatorBit(std::uint32_t current, MutatorBitLease lease) noexcept {
    if (!lease.owned || lease.originallySet || (current & kBigHeadMutatorBit) == 0) {
        return current;
    }
    return current & ~kBigHeadMutatorBit;
}

inline bool IsValidScale(float scale) noexcept {
    return std::isfinite(scale) && scale > 0.0F;
}

inline bool StillOwnsScale(float current, float lastWritten) noexcept {
    return std::bit_cast<std::uint32_t>(current) == std::bit_cast<std::uint32_t>(lastWritten);
}

} // namespace funrounds::native::bighead
