// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstdint>
#include <cmath>

namespace funrounds::native::detail {

enum class GroundControlSource : std::uint8_t {
    Unresolved,
    Bot,
    Human,
};

struct GroundControlIdentity {
    std::int32_t pawnHandle{-1};
    std::int32_t currentControllerHandle{-1};
    std::int32_t slotControllerHandle{-1};
    const void* pawn{};
    const void* slotPawn{};
    std::int32_t exactSlotMatches{};
    bool clientInGame{};
    bool fakeClient{};
};

constexpr void ReverseHorizontalWish(float (&direction)[3]) noexcept {
    direction[0] = -direction[0];
    direction[1] = -direction[1];
}

template <typename RegisterType, typename SetFloat>
void SetGroundReverseAccelerationRegister(RegisterType xmm0,
                                          RegisterType xmm1,
                                          RegisterType xmm2,
                                          SetFloat&& setFloat,
                                          float acceleration) {
#if defined(__linux__)
    (void)xmm0;
    (void)xmm1;
    setFloat(xmm2, acceleration);
#else
    (void)xmm1;
    (void)xmm2;
    setFloat(xmm0, acceleration);
#endif
}

inline bool IsValidGroundReverseAcceleration(float acceleration) noexcept {
    return std::isfinite(acceleration) && acceleration > 0.0F;
}

constexpr GroundControlSource ClassifyGroundControlSource(const GroundControlIdentity& identity) noexcept {
    if (identity.pawnHandle == -1 || identity.currentControllerHandle == -1 ||
        identity.currentControllerHandle != identity.slotControllerHandle || !identity.pawn ||
        identity.slotPawn != identity.pawn || identity.exactSlotMatches != 1 || !identity.clientInGame) {
        return GroundControlSource::Unresolved;
    }
    return identity.fakeClient ? GroundControlSource::Bot : GroundControlSource::Human;
}

} // namespace funrounds::native::detail
