// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "burn.hpp"

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <string_view>

#if !defined(_WIN32)
#include <immintrin.h>
#endif

#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"

namespace {

constexpr std::int32_t kInvalidHandle = -1;
constexpr std::int32_t kIgniteMode = 14;
constexpr char kEntityClass[] = "CBaseEntity";
constexpr char kPlayerPawnClass[] = "CBasePlayerPawn";
constexpr char kEntityFlameClass[] = "CEntityFlame";
constexpr std::string_view kEntityFlameName = "entityflame";

#if defined(_WIN32)
constexpr std::size_t kIgniteVtableSlot = 266;
extern "C" void FunRoundsInvokeIgnite(void* pawn, float duration, std::int32_t mode);
#elif defined(__linux__) && defined(__x86_64__)
constexpr std::size_t kIgniteVtableSlot = 267;
#else
#error "Player ignition is supported only on Windows x64 and Linux x64"
#endif

bool RequiredImportsAvailable() noexcept {
    return __s2sdk_IsValidEntHandle != nullptr && __s2sdk_EntHandleToEntPointer != nullptr &&
           __s2sdk_EntPointerToEntHandle != nullptr && __s2sdk_IsEntityPlayerPawn != nullptr &&
           __s2sdk_GetEntSchema2 != nullptr && __s2sdk_GetEntSchemaEnt2 != nullptr &&
           __s2sdk_GetEntityClassname != nullptr && __s2sdk_GetMaxClients != nullptr &&
           __s2sdk_IsClientInGame != nullptr && __s2sdk_PlayerSlotToEntHandle != nullptr &&
           __s2sdk_GetClientPawn != nullptr && __s2sdk_IsClientAlive != nullptr &&
           __s2sdk_RemoveEntity != nullptr;
}

bool HasRequiredSchema() {
    using funrounds::native::detail::SchemaField;
    using funrounds::native::detail::ServerSchema;

    if (!funrounds::native::detail::EnsureEngine()) {
        return false;
    }

    const auto hasField = [](const char* className, const char* fieldName) {
        SchemaField field{};
        return funrounds::native::detail::ResolveSchemaField(className, fieldName, 0, field);
    };
    const auto isHandleField = [](const char* className, const char* fieldName) {
        auto* const typeScope = ServerSchema();
        auto* const classInfo = typeScope == nullptr ? nullptr : typeScope->FindDeclaredClass(className).Get();
        if (classInfo == nullptr || (classInfo->m_nFieldCount != 0 && classInfo->m_pFields == nullptr)) {
            return false;
        }
        for (std::uint16_t index = 0; index < classInfo->m_nFieldCount; ++index) {
            const auto& field = classInfo->m_pFields[index];
            if (field.m_pszName == nullptr || std::strcmp(field.m_pszName, fieldName) != 0 ||
                field.m_pType == nullptr) {
                continue;
            }
            int size = 0;
            std::uint8_t alignment = 0;
            if (!field.m_pType->GetSizeAndAlignment(size, alignment) || size != sizeof(std::uint32_t)) {
                return false;
            }
            const char* const typeName = field.m_pType->m_sTypeName.Get();
            return typeName != nullptr && field.m_pType->m_eTypeCategory == SCHEMA_TYPE_ATOMIC &&
                   std::string_view(typeName).starts_with("CHandle<");
        }
        return false;
    };

    return hasField(kEntityClass, "m_iHealth") && hasField(kEntityClass, "m_lifeState") &&
           isHandleField(kEntityClass, "m_hEffectEntity") && isHandleField(kPlayerPawnClass, "m_hController") &&
           isHandleField(kEntityFlameClass, "m_hEntAttached");
}

bool IsEntityClass(std::int32_t handle, std::string_view expected) {
    if (!s2sdk::IsValidEntHandle(handle)) {
        return false;
    }
    const auto classname = s2sdk::GetEntityClassname(handle);
    const char* const text = classname.c_str();
    return text != nullptr && std::string_view(text) == expected;
}

bool IsCurrentLivePlayerPawn(std::int32_t pawnHandle,
                             void* expectedPawn,
                             std::int32_t& controllerHandle) {
    if (expectedPawn == nullptr || pawnHandle == kInvalidHandle ||
        !s2sdk::IsValidEntHandle(pawnHandle) || !s2sdk::IsEntityPlayerPawn(pawnHandle) ||
        s2sdk::EntHandleToEntPointer(pawnHandle) != expectedPawn ||
        s2sdk::EntPointerToEntHandle(expectedPawn) != pawnHandle) {
        return false;
    }

    if (s2sdk::GetEntSchema2(expectedPawn, kEntityClass, "m_iHealth", 0) <= 0 ||
        s2sdk::GetEntSchema2(expectedPawn, kEntityClass, "m_lifeState", 0) != 0) {
        return false;
    }

    controllerHandle = s2sdk::GetEntSchemaEnt2(expectedPawn, kPlayerPawnClass, "m_hController", 0);
    if (controllerHandle == kInvalidHandle || !s2sdk::IsValidEntHandle(controllerHandle)) {
        return false;
    }

    const auto maxClients = s2sdk::GetMaxClients();
    for (std::int32_t slot = 0; slot < maxClients; ++slot) {
        if (s2sdk::IsClientInGame(slot) && s2sdk::PlayerSlotToEntHandle(slot) == controllerHandle &&
            s2sdk::GetClientPawn(slot) == expectedPawn && s2sdk::IsClientAlive(slot)) {
            return true;
        }
    }
    return false;
}

bool HasValidAttachedEffect(void* pawn, std::int32_t& effectHandle) {
    effectHandle = s2sdk::GetEntSchemaEnt2(pawn, kEntityClass, "m_hEffectEntity", 0);
    return effectHandle != kInvalidHandle && s2sdk::IsValidEntHandle(effectHandle);
}

void RemoveExactNewFlame(void* pawn, std::int32_t pawnHandle, std::int32_t effectHandle) noexcept {
    try {
        if (pawn == nullptr || effectHandle == kInvalidHandle ||
            s2sdk::EntHandleToEntPointer(pawnHandle) != pawn ||
            !IsEntityClass(effectHandle, kEntityFlameName)) {
            return;
        }
        void* const effect = s2sdk::EntHandleToEntPointer(effectHandle);
        if (effect == nullptr) {
            return;
        }
        const auto attached = s2sdk::GetEntSchemaEnt2(pawn, kEntityClass, "m_hEffectEntity", 0);
        const auto flamePawn = s2sdk::GetEntSchemaEnt2(
            effect, kEntityFlameClass, "m_hEntAttached", 0);
        if (attached == effectHandle && flamePawn == pawnHandle) {
            s2sdk::RemoveEntity(effectHandle);
        }
    } catch (...) {
        // Best-effort cleanup must not turn a failed engine call into an escaping ABI exception.
    }
}

void InvokeIgnite(void* pawn, float duration) {
    auto** const vtable = *reinterpret_cast<void***>(pawn);
    if (vtable == nullptr || vtable[kIgniteVtableSlot] == nullptr) {
        return;
    }
#if defined(_WIN32)
    FunRoundsInvokeIgnite(pawn, duration, kIgniteMode);
#else
    using IgniteMethod = void (*)(void*, std::int32_t, void*, __m128, float, double, float);
    constexpr double kEngineIgniteSentinel = 1.4044474254567505e306;
    const auto method = reinterpret_cast<IgniteMethod>(vtable[kIgniteVtableSlot]);
    method(pawn, kIgniteMode, nullptr, _mm_set_ss(duration), 0.0F,
           kEngineIgniteSentinel, std::numeric_limits<float>::max());
#endif
}

std::int32_t TryIgnitePlayerImpl(std::int32_t pawnHandle, float duration) {
    if (!RequiredImportsAvailable() || !HasRequiredSchema() || !std::isfinite(duration) || duration <= 0.0F ||
        pawnHandle == kInvalidHandle || !s2sdk::IsValidEntHandle(pawnHandle) ||
        !s2sdk::IsEntityPlayerPawn(pawnHandle)) {
        return kInvalidHandle;
    }

    void* const pawn = s2sdk::EntHandleToEntPointer(pawnHandle);
    std::int32_t controllerHandle = kInvalidHandle;
    if (pawn == nullptr || !IsCurrentLivePlayerPawn(pawnHandle, pawn, controllerHandle)) {
        return kInvalidHandle;
    }

    std::int32_t previousEffect = kInvalidHandle;
    if (HasValidAttachedEffect(pawn, previousEffect)) {
        return kInvalidHandle;
    }

    InvokeIgnite(pawn, duration);

    std::int32_t currentController = kInvalidHandle;
    if (!IsCurrentLivePlayerPawn(pawnHandle, pawn, currentController) ||
        currentController != controllerHandle) {
        if (s2sdk::IsValidEntHandle(pawnHandle) && s2sdk::EntHandleToEntPointer(pawnHandle) == pawn) {
            std::int32_t partialEffect = kInvalidHandle;
            if (HasValidAttachedEffect(pawn, partialEffect) && partialEffect != previousEffect) {
                RemoveExactNewFlame(pawn, pawnHandle, partialEffect);
            }
        }
        return kInvalidHandle;
    }

    std::int32_t effectHandle = kInvalidHandle;
    if (!HasValidAttachedEffect(pawn, effectHandle) || effectHandle == previousEffect ||
        !IsEntityClass(effectHandle, kEntityFlameName)) {
        return kInvalidHandle;
    }

    void* const effect = s2sdk::EntHandleToEntPointer(effectHandle);
    const auto attachedPawn = effect == nullptr ? kInvalidHandle :
        s2sdk::GetEntSchemaEnt2(effect, kEntityFlameClass, "m_hEntAttached", 0);
    if (attachedPawn != pawnHandle || s2sdk::EntHandleToEntPointer(pawnHandle) != pawn ||
        s2sdk::GetEntSchemaEnt2(pawn, kEntityClass, "m_hEffectEntity", 0) != effectHandle) {
        RemoveExactNewFlame(pawn, pawnHandle, effectHandle);
        return kInvalidHandle;
    }
    return effectHandle;
}

} // namespace

extern "C" PLUGIN_API std::int32_t TryIgnitePlayer(std::int32_t pawnHandle, float duration) {
    try {
        return TryIgnitePlayerImpl(pawnHandle, duration);
    } catch (...) {
        return kInvalidHandle;
    }
}
