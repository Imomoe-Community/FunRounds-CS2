// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "ghost_visibility.hpp"

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <memory>
#include <vector>

#include <entity2/entityinstance.h>
#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"
#include "ghost_visibility_policy.hpp"

namespace funrounds::native::detail {
namespace {

constexpr char kSignature[] = "CCSBot::IsVisiblePlayer";
constexpr char kHookName[] = "FunRounds::CCSBot::IsVisiblePlayer";

struct GhostVisibilityState {
    void* address{};
    polyhook::HookHandle hook{};
    bool hookAttempted{};
};

GhostVisibilityState g_state;
std::atomic<bool> g_ready{};
std::atomic<std::shared_ptr<const std::vector<std::uint32_t>>> g_targets;

polyhook::ResultType GhostVisibilityPre(polyhook::HookHandle,
                                        polyhook::ParametersHandle parameters,
                                        std::int32_t,
                                        polyhook::ReturnHandle result,
                                        polyhook::CallbackType type) noexcept {
    if (type != polyhook::CallbackType::Pre || !g_ready.load(std::memory_order_acquire) || !parameters || !result) {
        return polyhook::ResultType::Ignored;
    }

    try {
        auto* const pawn = polyhook::GetArgument<void*>(parameters, 1);
        if (!pawn) {
            return polyhook::ResultType::Ignored;
        }

        // This is the only engine-object read in the worker callback. It is an inline
        // SourceSDK serial-handle accessor on the borrowed player argument; all policy
        // and target state are immutable values published by the game thread.
        const auto liveHandle = reinterpret_cast<CEntityInstance*>(pawn)->GetRefEHandle().ToInt();
        if (liveHandle == std::numeric_limits<std::uint32_t>::max()) {
            return polyhook::ResultType::Ignored;
        }
        const auto targets = g_targets.load(std::memory_order_acquire);
        if (!targets || !ghost_visibility::ContainsTarget(*targets, liveHandle)) {
            return polyhook::ResultType::Ignored;
        }

        auto* const visibleParts = static_cast<std::uint8_t*>(polyhook::GetArgument<void*>(parameters, 3));
        if (visibleParts) {
            *visibleParts = 0;
        }
        polyhook::SetReturn<bool>(result, false);
        return polyhook::ResultType::Supercede;
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
}

bool RequiredImportsAvailable() noexcept {
    return __polyhook_HookDetour != nullptr && __polyhook_FindDetour != nullptr &&
           __polyhook_AddCallback != nullptr && __polyhook_IsCallbackRegistered != nullptr &&
           __polyhook_RemoveCallback != nullptr && __polyhook_UnhookDetour != nullptr &&
           __polyhook_GetArgumentPointer != nullptr && __polyhook_SetReturnBool != nullptr &&
           __s2sdk_GetGameConfigSignature != nullptr;
}

bool RemoveHook() noexcept {
    try {
        g_ready.store(false, std::memory_order_release);
        if (!g_state.address) {
            return !g_state.hook;
        }
        const auto current = polyhook::FindDetour(g_state.address);
        if (!current) {
            g_state = {};
            return true;
        }
        if (!g_state.hook && g_state.hookAttempted) {
            g_state.hook = current;
        }
        if (!g_state.hook || current != g_state.hook) {
            return false;
        }
        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, GhostVisibilityPre)) {
            (void)polyhook::RemoveCallback(current, polyhook::CallbackType::Pre, GhostVisibilityPre);
            if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, GhostVisibilityPre)) {
                return false;
            }
        }
        if (!polyhook::UnhookDetour(g_state.address) && polyhook::FindDetour(g_state.address)) {
            return false;
        }
        if (polyhook::FindDetour(g_state.address)) {
            return false;
        }
        g_state = {};
        return true;
    } catch (...) {
        return false;
    }
}

} // namespace

bool InitializeGhostVisibility() noexcept {
    if (g_ready.load(std::memory_order_acquire)) {
        return true;
    }
    if (!RequiredImportsAvailable() || !EnsureEngine()) {
        return false;
    }
    try {
        void* const address = s2sdk::GetGameConfigSignature(GameConfig(), kSignature);
        if (!address) {
            return false;
        }
        if (polyhook::FindDetour(address)) {
            return false;
        }

        const plg::vector<polyhook::DataType> arguments{
            polyhook::DataType::Pointer,
            polyhook::DataType::Pointer,
            polyhook::DataType::Int8,
            polyhook::DataType::Pointer,
        };
        g_state.address = address;
        g_state.hookAttempted = true;
        const auto hook = polyhook::HookDetour(address,
                                               polyhook::DataType::Bool,
                                               arguments,
                                               -1,
                                               plg::string(kHookName));
        g_state.hook = hook;
        if (!hook) {
            (void)RemoveHook();
            return false;
        }
        (void)polyhook::AddCallback(hook, polyhook::CallbackType::Pre, GhostVisibilityPre);
        if (!polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Pre, GhostVisibilityPre)) {
            (void)RemoveHook();
            return false;
        }
        g_ready.store(true, std::memory_order_release);
        return true;
    } catch (...) {
        (void)RemoveHook();
        return false;
    }
}

bool GhostVisibilityReady() noexcept {
    return g_ready.load(std::memory_order_acquire);
}

std::shared_ptr<const std::vector<std::uint32_t>> CurrentGhostVisibilityTargets() noexcept {
    return g_targets.load(std::memory_order_acquire);
}

void ForgetGhostVisibilityTargetsForMapEnd() noexcept {
    // Map-end entity indexes can be reused immediately. Drop old serial-bearing
    // handles without resolving them against the next world's entity list.
    g_targets.store(nullptr, std::memory_order_release);
}

bool SetGhostVisibilityTargets(const plg::vector<std::int32_t>& pawnHandles) noexcept {
    try {
        if (pawnHandles.empty()) {
            g_targets.store(nullptr, std::memory_order_release);
            return true;
        }
        if (!GhostVisibilityReady()) {
            return false;
        }
        const std::vector<std::int32_t> input(pawnHandles.begin(), pawnHandles.end());
        auto targets = ghost_visibility::BuildTargetSet(input);
        if (targets.empty()) {
            g_targets.store(nullptr, std::memory_order_release);
        } else {
            g_targets.store(std::make_shared<const std::vector<std::uint32_t>>(std::move(targets)),
                            std::memory_order_release);
        }
        return true;
    } catch (...) {
        return false;
    }
}

bool ShutdownGhostVisibility() noexcept {
    g_targets.store(nullptr, std::memory_order_release);
    return RemoveHook();
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasGhostVisibilitySupport() {
    return funrounds::native::detail::InitializeGhostVisibility();
}

extern "C" PLUGIN_API bool SetGhostVisibilityTargets(const plg::vector<std::int32_t>& pawnHandles) {
    return funrounds::native::detail::SetGhostVisibilityTargets(pawnHandles);
}
