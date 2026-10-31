// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "ghost_radar.hpp"

#include <array>
#include <atomic>
#include <bit>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <utility>
#include <vector>

#include <entity2/entityinstance.h>
#include <entity2/entitynetwork.h>
#include <eiface.h>
#include <polyhook/polyhook.hpp>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"
#include "entity_handle.hpp"
#include "ghost_visibility.hpp"

namespace funrounds::native::detail {
namespace {

struct SpottedLayout {
    std::int32_t spottedOffset{};
    std::int32_t spottedByMaskOffset{};
    bool ready{};
};

struct OwnedSpottedState {
    std::int32_t pawnHandle{-1};
    bool ownsSpotted{};
    bool originalSpotted{};
    bool filteredSpotted{};
    bool ownsSpottedByMask{};
    std::uint64_t originalSpottedByMask{};
    std::uint64_t filteredSpottedByMask{};
};

struct StateFilterHooks {
    ISource2Server* server{};
    polyhook::HookHandle preWorldUpdateHook{};
    polyhook::HookHandle gameFrameHook{};
    std::atomic<bool> ready{};
};

constexpr std::size_t kSpottedMaskWordCount = 2;
constexpr char kSpottedStateClass[] = "EntitySpottedState_t";
constexpr char kPawnClass[] = "CCSPlayerPawn";
constexpr char kSpottedStateField[] = "m_entitySpottedState";
constexpr char kSpottedField[] = "m_bSpotted";
constexpr char kSpottedByMaskField[] = "m_bSpottedByMask";

SpottedLayout g_layout;
StateFilterHooks g_hooks;
std::vector<OwnedSpottedState> g_ownedStates;
bool g_mapEndRegistered{};

polyhook::ResultType GhostRadarPreWorldUpdateCallback(polyhook::HookHandle,
                                                       polyhook::ParametersHandle,
                                                       std::int32_t,
                                                       polyhook::ReturnHandle,
                                                       polyhook::CallbackType) noexcept;
polyhook::ResultType GhostRadarGameFrameCallback(polyhook::HookHandle,
                                                  polyhook::ParametersHandle,
                                                  std::int32_t,
                                                  polyhook::ReturnHandle,
                                                  polyhook::CallbackType) noexcept;
void GhostRadarMapEndCallback() noexcept;

bool RequiredImportsAvailable() noexcept {
    return __polyhook_FindVirtual2 != nullptr && __polyhook_AddCallback != nullptr &&
           __polyhook_IsCallbackRegistered != nullptr && __polyhook_RemoveCallback != nullptr &&
           __s2sdk_QueryInterface != nullptr && __s2sdk_NetworkStateChanged != nullptr &&
           __s2sdk_OnMapEnd_Register != nullptr && __s2sdk_OnMapEnd_Unregister != nullptr &&
           __s2sdk_EntHandleToEntPointer != nullptr && __s2sdk_EntPointerToEntHandle != nullptr &&
           __s2sdk_IsEntityPlayerPawn != nullptr;
}

bool ResolveSpottedLayout() noexcept {
    if (g_layout.ready) {
        return true;
    }

    SchemaField stateField{};
    SchemaField spottedField{};
    SchemaField spottedByMaskField{};
    if (!ResolveSchemaField(kPawnClass, kSpottedStateField, 0, stateField) ||
        !ResolveSchemaField(kSpottedStateClass, kSpottedField, sizeof(bool), spottedField) ||
        !ResolveSchemaField(kSpottedStateClass,
                            kSpottedByMaskField,
                            kSpottedMaskWordCount * sizeof(std::uint32_t),
                            spottedByMaskField)) {
        return false;
    }

    const auto stateSize = static_cast<std::int64_t>(stateField.size);
    const auto spottedOffset = static_cast<std::int64_t>(spottedField.offset);
    const auto spottedByMaskOffset = static_cast<std::int64_t>(spottedByMaskField.offset);
    const auto spottedByMaskSize = static_cast<std::int64_t>(spottedByMaskField.size);
    if (stateField.offset < 0 || spottedOffset < 0 || spottedByMaskOffset < 0 ||
        spottedOffset + static_cast<std::int64_t>(spottedField.size) > stateSize ||
        spottedByMaskOffset + spottedByMaskSize > stateSize) {
        return false;
    }

    const auto flattenedSpottedOffset = static_cast<std::int64_t>(stateField.offset) + spottedOffset;
    const auto flattenedMaskOffset = static_cast<std::int64_t>(stateField.offset) + spottedByMaskOffset;
    if (flattenedSpottedOffset > std::numeric_limits<std::int32_t>::max() ||
        flattenedMaskOffset > std::numeric_limits<std::int32_t>::max()) {
        return false;
    }

    g_layout.spottedOffset = static_cast<std::int32_t>(flattenedSpottedOffset);
    g_layout.spottedByMaskOffset = static_cast<std::int32_t>(flattenedMaskOffset);
    g_layout.ready = true;
    return true;
}

void* CurrentPlayerPawn(std::int32_t pawnHandle) noexcept {
    if (pawnHandle == -1 || !IsPlayerPawn(pawnHandle)) {
        return nullptr;
    }
    void* const pawn = EntityFromHandle(pawnHandle);
    if (pawn == nullptr || HandleFromEntity(pawn) != pawnHandle ||
        reinterpret_cast<CEntityInstance*>(pawn)->GetRefEHandle().ToInt() != ToNativeHandle(pawnHandle)) {
        return nullptr;
    }
    return pawn;
}

bool* SpottedField(void* pawn) noexcept {
    if (pawn == nullptr || g_layout.spottedOffset < 0) {
        return nullptr;
    }
    return reinterpret_cast<bool*>(static_cast<std::byte*>(pawn) + g_layout.spottedOffset);
}

std::byte* SpottedByMaskField(void* pawn) noexcept {
    if (pawn == nullptr || g_layout.spottedByMaskOffset < 0) {
        return nullptr;
    }
    return static_cast<std::byte*>(pawn) + g_layout.spottedByMaskOffset;
}

std::uint64_t ReadSpottedByMask(const void* pawn) noexcept {
    std::array<std::uint32_t, kSpottedMaskWordCount> words{};
    const auto* const field = SpottedByMaskField(const_cast<void*>(pawn));
    if (field != nullptr) {
        std::memcpy(words.data(), field, sizeof(words));
    }
    return static_cast<std::uint64_t>(words[0]) | (static_cast<std::uint64_t>(words[1]) << 32);
}

void WriteSpottedByMask(void* pawn, std::uint64_t value) noexcept {
    const std::array<std::uint32_t, kSpottedMaskWordCount> words{
        static_cast<std::uint32_t>(value),
        static_cast<std::uint32_t>(value >> 32),
    };
    if (auto* const field = SpottedByMaskField(pawn); field != nullptr) {
        std::memcpy(field, words.data(), sizeof(words));
    }
}

void MarkSpottedStateChanged(std::int32_t pawnHandle) noexcept {
    try {
        s2sdk::NetworkStateChanged(pawnHandle, kPawnClass, kSpottedStateField);
    } catch (...) {
    }
}

void ForgetOwnedStateForMapEnd() noexcept {
    // Map-end handles can be recycled before the next callback. Drop our leases
    // without resolving or restoring old-map entities.
    g_ownedStates.clear();
    ForgetGhostVisibilityTargetsForMapEnd();
    ForgetGhostRadarTargetsForMapEnd();
}

void GhostRadarMapEndCallback() noexcept {
    ForgetOwnedStateForMapEnd();
}

bool RegisterMapEndCallback() noexcept {
    if (g_mapEndRegistered) {
        return true;
    }
    try {
        s2sdk::OnMapEnd_Register(GhostRadarMapEndCallback);
        g_mapEndRegistered = true;
        return true;
    } catch (...) {
        return false;
    }
}

bool UnregisterMapEndCallback() noexcept {
    if (!g_mapEndRegistered) {
        return true;
    }
    try {
        s2sdk::OnMapEnd_Unregister(GhostRadarMapEndCallback);
        g_mapEndRegistered = false;
        return true;
    } catch (...) {
        return false;
    }
}

void RestoreOwnedSpottedStates() noexcept {
    if (!g_layout.ready) {
        g_ownedStates.clear();
        return;
    }

    for (const auto& owned : g_ownedStates) {
        void* const pawn = CurrentPlayerPawn(owned.pawnHandle);
        if (pawn == nullptr) {
            continue;
        }

        bool changed = false;
        bool* const spotted = SpottedField(pawn);
        if (owned.ownsSpotted && spotted != nullptr && *spotted == owned.filteredSpotted &&
            *spotted != owned.originalSpotted) {
            *spotted = owned.originalSpotted;
            changed = true;
        }

        const std::uint64_t spottedByMask = ReadSpottedByMask(pawn);
        if (owned.ownsSpottedByMask && spottedByMask == owned.filteredSpottedByMask &&
            spottedByMask != owned.originalSpottedByMask) {
            WriteSpottedByMask(pawn, owned.originalSpottedByMask);
            changed = true;
        }

        if (changed) {
            MarkSpottedStateChanged(owned.pawnHandle);
        }
    }
    g_ownedStates.clear();
}

void ApplySpottedStateFilter() {
    RestoreOwnedSpottedStates();
    const auto snapshot = CurrentGhostRadarSnapshot();
    if (!snapshot || snapshot->targets.empty()) {
        return;
    }

    std::vector<OwnedSpottedState> nextOwned;
    nextOwned.reserve(snapshot->targets.size());
    for (const auto& target : snapshot->targets) {
        const std::int32_t pawnHandle = ToApiHandle(target.fullHandle);
        void* const pawn = CurrentPlayerPawn(pawnHandle);
        if (pawn == nullptr) {
            continue;
        }

        bool* const spotted = SpottedField(pawn);
        if (spotted == nullptr || SpottedByMaskField(pawn) == nullptr) {
            continue;
        }

        const bool originalSpotted = *spotted;
        const std::uint64_t originalMask = ReadSpottedByMask(pawn);
        const auto filtered = ghost_radar::FilterSpottedState(originalSpotted,
                                                               originalMask,
                                                               target.enemyRecipients,
                                                               snapshot->connectedRecipientMask);
        OwnedSpottedState owned{
            .pawnHandle = pawnHandle,
            .ownsSpotted = originalSpotted != filtered.spotted,
            .originalSpotted = originalSpotted,
            .filteredSpotted = filtered.spotted,
            .ownsSpottedByMask = originalMask != filtered.spottedByMask,
            .originalSpottedByMask = originalMask,
            .filteredSpottedByMask = filtered.spottedByMask,
        };
        if (!owned.ownsSpotted && !owned.ownsSpottedByMask) {
            continue;
        }

        if (owned.ownsSpotted) {
            *spotted = filtered.spotted;
        }
        if (owned.ownsSpottedByMask) {
            WriteSpottedByMask(pawn, filtered.spottedByMask);
        }
        nextOwned.push_back(owned);
        MarkSpottedStateChanged(pawnHandle);
    }
    g_ownedStates = std::move(nextOwned);
}

bool RemoveStateCallback(polyhook::HookHandle hook,
                         polyhook::CallbackType type,
                         polyhook::CallbackHandler callback) noexcept {
    try {
        if (hook == nullptr || !polyhook::IsCallbackRegistered(hook, type, callback)) {
            return true;
        }
        (void)polyhook::RemoveCallback(hook, type, callback);
        return !polyhook::IsCallbackRegistered(hook, type, callback);
    } catch (...) {
        return false;
    }
}

polyhook::ResultType GhostRadarPreWorldUpdateCallback(polyhook::HookHandle,
                                                       polyhook::ParametersHandle,
                                                       std::int32_t,
                                                       polyhook::ReturnHandle,
                                                       polyhook::CallbackType type) noexcept {
    if (!g_hooks.ready.load(std::memory_order_acquire)) {
        return polyhook::ResultType::Ignored;
    }
    try {
        if (type == polyhook::CallbackType::Pre) {
            RestoreOwnedSpottedStates();
        } else if (type == polyhook::CallbackType::Post) {
            ApplySpottedStateFilter();
        }
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
    return polyhook::ResultType::Ignored;
}

polyhook::ResultType GhostRadarGameFrameCallback(polyhook::HookHandle,
                                                  polyhook::ParametersHandle,
                                                  std::int32_t,
                                                  polyhook::ReturnHandle,
                                                  polyhook::CallbackType type) noexcept {
    if (type == polyhook::CallbackType::Pre && g_hooks.ready.load(std::memory_order_acquire)) {
        RestoreOwnedSpottedStates();
    }
    return polyhook::ResultType::Ignored;
}

bool InstallStateCallbacks() {
    auto preWorldUpdate = &ISource2Server::PreWorldUpdate;
    auto gameFrame = &IServerGameDLL::GameFrame;
    g_hooks.preWorldUpdateHook = polyhook::FindVirtual2(g_hooks.server, reinterpret_cast<void*&>(preWorldUpdate));
    g_hooks.gameFrameHook = polyhook::FindVirtual2(g_hooks.server, reinterpret_cast<void*&>(gameFrame));
    if (g_hooks.preWorldUpdateHook == nullptr || g_hooks.gameFrameHook == nullptr) {
        return false;
    }

    (void)polyhook::AddCallback(g_hooks.preWorldUpdateHook,
                                polyhook::CallbackType::Pre,
                                GhostRadarPreWorldUpdateCallback);
    (void)polyhook::AddCallback(g_hooks.preWorldUpdateHook,
                                polyhook::CallbackType::Post,
                                GhostRadarPreWorldUpdateCallback);
    (void)polyhook::AddCallback(g_hooks.gameFrameHook,
                                polyhook::CallbackType::Pre,
                                GhostRadarGameFrameCallback);
    return polyhook::IsCallbackRegistered(g_hooks.preWorldUpdateHook,
                                          polyhook::CallbackType::Pre,
                                          GhostRadarPreWorldUpdateCallback) &&
           polyhook::IsCallbackRegistered(g_hooks.preWorldUpdateHook,
                                          polyhook::CallbackType::Post,
                                          GhostRadarPreWorldUpdateCallback) &&
           polyhook::IsCallbackRegistered(g_hooks.gameFrameHook,
                                          polyhook::CallbackType::Pre,
                                          GhostRadarGameFrameCallback);
}

} // namespace

bool InitializeGhostRadarStateFilter() noexcept {
    if (g_hooks.ready.load(std::memory_order_acquire)) {
        return true;
    }
    if (!RequiredImportsAvailable() || !EnsureEngine() || !ResolveSpottedLayout()) {
        return false;
    }

    try {
        g_hooks.server = static_cast<ISource2Server*>(
            s2sdk::QueryInterface("server", INTERFACEVERSION_SERVERGAMEDLL));
        if (g_hooks.server == nullptr || !InstallStateCallbacks() || !RegisterMapEndCallback()) {
            (void)ShutdownGhostRadarStateFilter();
            return false;
        }
        g_hooks.ready.store(true, std::memory_order_release);
        return true;
    } catch (...) {
        (void)ShutdownGhostRadarStateFilter();
        return false;
    }
}

bool GhostRadarStateFilterReady() noexcept {
    return g_hooks.ready.load(std::memory_order_acquire);
}

bool ShutdownGhostRadarStateFilter() noexcept {
    g_hooks.ready.store(false, std::memory_order_release);
    const bool mapEndClean = UnregisterMapEndCallback();
    RestoreOwnedSpottedStates();
    const bool preWorldPreClean = RemoveStateCallback(g_hooks.preWorldUpdateHook,
                                                       polyhook::CallbackType::Pre,
                                                       GhostRadarPreWorldUpdateCallback);
    const bool preWorldPostClean = RemoveStateCallback(g_hooks.preWorldUpdateHook,
                                                        polyhook::CallbackType::Post,
                                                        GhostRadarPreWorldUpdateCallback);
    const bool gameFrameClean = RemoveStateCallback(g_hooks.gameFrameHook,
                                                     polyhook::CallbackType::Pre,
                                                     GhostRadarGameFrameCallback);
    if (!mapEndClean || !preWorldPreClean || !preWorldPostClean || !gameFrameClean) {
        return false;
    }
    g_hooks.preWorldUpdateHook = nullptr;
    g_hooks.gameFrameHook = nullptr;
    g_hooks.server = nullptr;
    g_hooks.ready.store(false, std::memory_order_release);
    return true;
}

} // namespace funrounds::native::detail
