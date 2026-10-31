// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "ground_reverse.hpp"

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>

#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"
#include "entity_handle.hpp"
#include "ground_reverse_policy.hpp"

namespace funrounds::native::detail {
namespace {

constexpr char kCallsiteSignature[] = "FunRounds::GroundReverseAccelerationCallsite";
constexpr char kHookName[] = "FunRounds::GroundReverseAccelerationCallsite";
constexpr std::int32_t kInvalidHandle = -1;
constexpr std::int32_t kMovementPawnOffset = 56;
constexpr float kDefaultAcceleration = 5.5F;

#if defined(_WIN32)
constexpr std::ptrdiff_t kCallsiteOffset = 0;
#elif defined(__linux__)
constexpr std::ptrdiff_t kCallsiteOffset = 16;
#else
#error "Ground reverse is only supported on Windows x64 and Linux x64"
#endif

struct GroundReverseState {
    void* address{};
    polyhook::HookHandle hook{};
    SchemaField pawnController{};
    bool schemaReady{};
    bool enabled{};
    float acceleration{kDefaultAcceleration};
};

GroundReverseState g_state;

polyhook::ResultType GroundReversePre(polyhook::HookHandle,
                                      polyhook::ParametersHandle,
                                      std::int32_t,
                                      polyhook::ReturnHandle,
                                      polyhook::CallbackType) noexcept;

bool RequiredImportsAvailable() noexcept {
    return __polyhook_HookDetour2 && __polyhook_FindDetour && __polyhook_AddCallback &&
           __polyhook_IsCallbackRegistered && __polyhook_RemoveCallback && __polyhook_UnhookDetour &&
           __polyhook_GetRegisterPointer && __polyhook_SetRegisterFloat && __s2sdk_GetGameConfigSignature &&
           __s2sdk_GetMaxClients && __s2sdk_IsClientInGame && __s2sdk_PlayerSlotToEntHandle &&
           __s2sdk_GetClientPawn && __s2sdk_IsFakeClient && __s2sdk_IsEntityPlayerPawn &&
           __s2sdk_IsEntityPlayerController;
}

void* GroundCallsiteAddress() noexcept {
    if (!EnsureEngine() || !__s2sdk_GetGameConfigSignature) {
        return nullptr;
    }
    auto* match = static_cast<std::byte*>(s2sdk::GetGameConfigSignature(GameConfig(), kCallsiteSignature));
    return match ? static_cast<void*>(match + kCallsiteOffset) : nullptr;
}

bool ResolveGroundIdentitySchema() noexcept {
    if (g_state.schemaReady) {
        return true;
    }
    if (!ResolveSchemaField("CBasePlayerPawn", "m_hController", sizeof(std::uint32_t), g_state.pawnController)) {
        return false;
    }
    g_state.schemaReady = true;
    return true;
}

template <typename T>
T ReadField(const void* object, std::int32_t offset) noexcept {
    T value{};
    if (object && offset >= 0) {
        std::memcpy(&value, static_cast<const std::byte*>(object) + offset, sizeof(T));
    }
    return value;
}

bool ResolveCurrentControl(void* movement,
                           std::int32_t& pawnHandle,
                           GroundControlSource& source) noexcept {
    source = GroundControlSource::Unresolved;
    if (!movement || !ResolveGroundIdentitySchema()) {
        return false;
    }

    void* pawn = ReadField<void*>(movement, kMovementPawnOffset);
    pawnHandle = HandleFromEntity(pawn);
    if (!pawn || pawnHandle == kInvalidHandle || !IsPlayerPawn(pawnHandle) ||
        HandleFromEntity(pawn) != pawnHandle || EntityFromHandle(pawnHandle) != pawn) {
        return false;
    }

    const auto controllerBits = ReadField<std::uint32_t>(pawn, g_state.pawnController.offset);
    const std::int32_t currentControllerHandle = ToApiHandle(controllerBits);
    void* const controller = EntityFromHandle(currentControllerHandle);
    if (currentControllerHandle == kInvalidHandle || !controller || !IsPlayerController(currentControllerHandle) ||
        HandleFromEntity(controller) != currentControllerHandle) {
        return false;
    }

    std::int32_t matchedSlots = 0;
    std::int32_t matchingSlot = -1;
    void* matchingPawn = nullptr;
    bool matchingClientInGame = false;
    bool matchingFakeClient = false;
    const auto maxClients = s2sdk::GetMaxClients();
    if (maxClients <= 0) {
        return false;
    }
    for (std::int32_t slot = 0; slot < maxClients; ++slot) {
        if (!s2sdk::IsClientInGame(slot) || s2sdk::PlayerSlotToEntHandle(slot) != currentControllerHandle) {
            continue;
        }
        void* const slotPawn = s2sdk::GetClientPawn(slot);
        if (!slotPawn || slotPawn != pawn || HandleFromEntity(slotPawn) != pawnHandle) {
            continue;
        }
        ++matchedSlots;
        matchingSlot = slot;
        matchingPawn = slotPawn;
        matchingClientInGame = true;
        matchingFakeClient = s2sdk::IsFakeClient(slot);
    }

    if (matchingSlot == -1) {
        return false;
    }
    const GroundControlIdentity identity{
        .pawnHandle = pawnHandle,
        .currentControllerHandle = currentControllerHandle,
        .slotControllerHandle = s2sdk::PlayerSlotToEntHandle(matchingSlot),
        .pawn = pawn,
        .slotPawn = matchingPawn,
        .exactSlotMatches = matchedSlots,
        .clientInGame = matchingClientInGame,
        .fakeClient = matchingFakeClient,
    };
    source = ClassifyGroundControlSource(identity);
    return source != GroundControlSource::Unresolved;
}

bool RemoveGroundHook() noexcept;

bool InstallGroundHook() {
    if (!RequiredImportsAvailable()) {
        return false;
    }
    void* const address = GroundCallsiteAddress();
    if (!address || !ResolveGroundIdentitySchema() || (g_state.address && g_state.address != address)) {
        return false;
    }
    if (g_state.hook) {
        if (polyhook::FindDetour(address) != g_state.hook ||
            !polyhook::IsCallbackRegistered(g_state.hook, polyhook::CallbackType::Pre, GroundReversePre)) {
            return false;
        }
        g_state.address = address;
        return true;
    }
    if (polyhook::FindDetour(address)) {
        return false;
    }

    const auto hook = polyhook::HookDetour2(address, kHookName);
    if (!hook) {
        return false;
    }
    g_state.address = address;
    g_state.hook = hook;
    try {
        (void)polyhook::AddCallback(hook, polyhook::CallbackType::Pre, GroundReversePre);
        if (!polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Pre, GroundReversePre)) {
            g_state.enabled = false;
            (void)RemoveGroundHook();
            return false;
        }
        return true;
    } catch (...) {
        g_state.enabled = false;
        (void)RemoveGroundHook();
        return false;
    }
}

bool RemoveGroundHook() noexcept {
    g_state.enabled = false;
    if (!g_state.address) {
        g_state.hook = nullptr;
        return true;
    }

    try {
        auto remaining = polyhook::FindDetour(g_state.address);
        if (!remaining) {
            g_state.address = nullptr;
            g_state.hook = nullptr;
            return true;
        }
        if (!g_state.hook || remaining != g_state.hook) {
            return false;
        }
        if (polyhook::IsCallbackRegistered(remaining, polyhook::CallbackType::Pre, GroundReversePre)) {
            (void)polyhook::RemoveCallback(remaining, polyhook::CallbackType::Pre, GroundReversePre);
            if (polyhook::IsCallbackRegistered(remaining, polyhook::CallbackType::Pre, GroundReversePre)) {
                return false;
            }
        }
        if (!polyhook::UnhookDetour(g_state.address) && polyhook::FindDetour(g_state.address)) {
            return false;
        }
        if (polyhook::FindDetour(g_state.address)) {
            return false;
        }
        g_state.address = nullptr;
        g_state.hook = nullptr;
        return true;
    } catch (...) {
        return false;
    }
}

} // namespace

bool HasGroundReverseSupport() noexcept {
    try {
        return RequiredImportsAvailable() && GroundCallsiteAddress() && ResolveGroundIdentitySchema();
    } catch (...) {
        return false;
    }
}

bool SetGroundReverseActive(bool active, float acceleration) noexcept {
    if (!active) {
        return RemoveGroundHook();
    }
    if (!IsValidGroundReverseAcceleration(acceleration)) {
        (void)RemoveGroundHook();
        return false;
    }
    try {
        if (!InstallGroundHook()) {
            g_state.enabled = false;
            (void)RemoveGroundHook();
            return false;
        }
        g_state.acceleration = acceleration;
        g_state.enabled = true;
        return true;
    } catch (...) {
        g_state.enabled = false;
        (void)RemoveGroundHook();
        return false;
    }
}

bool ShutdownGroundReverse() noexcept {
    return RemoveGroundHook();
}

namespace {

polyhook::ResultType GroundReversePre(polyhook::HookHandle,
                                      polyhook::ParametersHandle parameters,
                                      std::int32_t,
                                      polyhook::ReturnHandle,
                                      polyhook::CallbackType type) noexcept {
    try {
        if (type != polyhook::CallbackType::Pre || !g_state.enabled || !parameters) {
            return polyhook::ResultType::Ignored;
        }
        const auto registers = static_cast<polyhook::RegistersHandle>(parameters);
#if defined(_WIN32)
        void* const movement = polyhook::GetRegister<void*>(registers, polyhook::RegisterType::RCX);
        float* const wishDirection = polyhook::GetRegister<float*>(registers, polyhook::RegisterType::R8);
#else
        void* const movement = polyhook::GetRegister<void*>(registers, polyhook::RegisterType::RDI);
        float* const wishDirection = polyhook::GetRegister<float*>(registers, polyhook::RegisterType::RDX);
#endif
        std::int32_t pawnHandle = kInvalidHandle;
        GroundControlSource source = GroundControlSource::Unresolved;
        if (!ResolveCurrentControl(movement, pawnHandle, source)) {
            return polyhook::ResultType::Ignored;
        }
        if (source == GroundControlSource::Human) {
            if (!wishDirection) {
                return polyhook::ResultType::Ignored;
            }
            float vector[3]{};
            std::memcpy(vector, wishDirection, sizeof(vector));
            if (!std::isfinite(vector[0]) || !std::isfinite(vector[1]) || !std::isfinite(vector[2])) {
                return polyhook::ResultType::Ignored;
            }
            ReverseHorizontalWish(vector);
            std::memcpy(wishDirection, vector, sizeof(vector));
        }
        // Linux walk-move passes max speed and wish speed in XMM0/XMM1; acceleration is XMM2.
        SetGroundReverseAccelerationRegister(
            polyhook::RegisterType::XMM0,
            polyhook::RegisterType::XMM1,
            polyhook::RegisterType::XMM2,
            [registers](polyhook::RegisterType reg, float value) {
                polyhook::SetRegisterFloat(registers, reg, value);
            },
            g_state.acceleration);
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
    return polyhook::ResultType::Ignored;
}

} // namespace

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasGroundReverseSupport() {
    return funrounds::native::detail::HasGroundReverseSupport();
}

extern "C" PLUGIN_API bool SetGroundReverseActive(bool active, float acceleration) {
    return funrounds::native::detail::SetGroundReverseActive(active, acceleration);
}
