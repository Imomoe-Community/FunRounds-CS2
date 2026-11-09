// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "standard_grenade.hpp"

#include <algorithm>
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

#include <entity2/entityinstance.h>
#include <plugin_export.h>
#include <plg/plugin.hpp>
#include <polyhook/polyhook.hpp>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"
#include "standard_grenade_policy.hpp"

namespace funrounds::native::detail {
namespace {

struct PendingDetonation {
    std::int32_t handle{-1};
    void* entity{};
};

struct DetonateHook {
    void* address{};
    polyhook::HookHandle handle{};
    bool attempted{};
};

std::vector<PendingDetonation> g_pending;
DetonateHook g_smokeHook;
DetonateHook g_molotovHook;
bool g_mapEndRegistered{};
bool g_smokeFailureLogged{};
bool g_fireFailureLogged{};

polyhook::ResultType DetonateEntry(polyhook::HookHandle,
                                   polyhook::ParametersHandle parameters,
                                   std::int32_t,
                                   polyhook::ReturnHandle,
                                   polyhook::CallbackType type) noexcept;

const char* MissingRequiredImport() noexcept {
    if (!__s2sdk_GetGameConfigVTable) return "missing S2SDK GetGameConfigVTable import";
    if (!__s2sdk_GetGameConfigOffset) return "missing S2SDK GetGameConfigOffset import";
    if (!__s2sdk_IsValidEntHandle) return "missing S2SDK IsValidEntHandle import";
    if (!__s2sdk_EntHandleToEntPointer) return "missing S2SDK EntHandleToEntPointer import";
    if (!__s2sdk_EntPointerToEntHandle) return "missing S2SDK EntPointerToEntHandle import";
    if (!__s2sdk_GetEntityClassname) return "missing S2SDK GetEntityClassname import";
    if (!__s2sdk_OnMapEnd_Register) return "missing S2SDK OnMapEnd_Register import";
    if (!__s2sdk_OnMapEnd_Unregister) return "missing S2SDK OnMapEnd_Unregister import";
    if (!__polyhook_HookDetour) return "missing PolyHook HookDetour import";
    if (!__polyhook_FindDetour) return "missing PolyHook FindDetour import";
    if (!__polyhook_AddCallback) return "missing PolyHook AddCallback import";
    if (!__polyhook_IsCallbackRegistered) return "missing PolyHook IsCallbackRegistered import";
    if (!__polyhook_RemoveCallback) return "missing PolyHook RemoveCallback import";
    if (!__polyhook_AreCallbacksRegistered) return "missing PolyHook AreCallbacksRegistered import";
    if (!__polyhook_UnhookDetour) return "missing PolyHook UnhookDetour import";
    if (!__polyhook_GetArgumentPointer) return "missing PolyHook GetArgumentPointer import";
    return nullptr;
}

void LogSupportFailure(funrounds::native::grenade::Kind kind, const char* reason) noexcept {
    bool& logged = kind == funrounds::native::grenade::Kind::Smoke ? g_smokeFailureLogged : g_fireFailureLogged;
    if (logged) return;
    logged = true;
    try {
        if (plg::Log != nullptr) {
            std::string message = kind == funrounds::native::grenade::Kind::Smoke
                                      ? "Standard smoke Detonate unavailable: "
                                      : "Standard Molotov/incendiary Detonate unavailable: ";
            message += reason;
            plg::Log(plg::string(message), plg::Severity::Error, plg::source_location::current());
        }
    } catch (...) {
    }
}

void ForgetPendingOnMapEnd() noexcept {
    g_pending.clear();
}

bool RegisterMapEndCallback() noexcept {
    if (g_mapEndRegistered) return true;
    try {
        s2sdk::OnMapEnd_Register(ForgetPendingOnMapEnd);
        g_mapEndRegistered = true;
        return true;
    } catch (...) {
        return false;
    }
}

bool UnregisterMapEndCallback() noexcept {
    if (!g_mapEndRegistered) return true;
    try {
        s2sdk::OnMapEnd_Unregister(ForgetPendingOnMapEnd);
        g_mapEndRegistered = false;
        return true;
    } catch (...) {
        return false;
    }
}

std::int32_t DetonateSlot() noexcept {
    try {
        const auto slot = s2sdk::GetGameConfigOffset(GameConfig(), "CSGrenadeProjectileBase::Detonate");
        return slot >= 0 && slot < 4096 ? slot : -1;
    } catch (...) {
        return -1;
    }
}

void* ProjectileDetonate(funrounds::native::grenade::Kind kind) noexcept {
    try {
        const auto name = funrounds::native::grenade::VTableName(kind);
        if (name.empty()) return nullptr;
        auto* const table = static_cast<void**>(s2sdk::GetGameConfigVTable(GameConfig(), std::string(name)));
        const auto slot = DetonateSlot();
        if (!table || slot < 0) return nullptr;
        return table[slot];
    } catch (...) {
        return nullptr;
    }
}

DetonateHook& HookFor(funrounds::native::grenade::Kind kind) noexcept {
    return kind == funrounds::native::grenade::Kind::Smoke ? g_smokeHook : g_molotovHook;
}

bool CurrentProjectile(std::int32_t handle,
                       funrounds::native::grenade::Kind kind,
                       void*& entity) noexcept {
    entity = nullptr;
    try {
        if (handle == -1 || !s2sdk::IsValidEntHandle(handle)) return false;
        entity = EntityFromHandle(handle);
        if (!entity || HandleFromEntity(entity) != handle ||
            reinterpret_cast<CEntityInstance*>(entity)->GetRefEHandle().ToInt() != static_cast<std::uint32_t>(handle)) {
            entity = nullptr;
            return false;
        }
        const auto classname = s2sdk::GetEntityClassname(handle);
        if (std::string_view(classname.c_str()) != funrounds::native::grenade::ProjectileClass(kind)) {
            entity = nullptr;
            return false;
        }
        return true;
    } catch (...) {
        entity = nullptr;
        return false;
    }
}

bool IsKnownGrenadeProjectile(std::int32_t handle, void* entity) {
    if (handle == -1 || !entity || !s2sdk::IsValidEntHandle(handle) || EntityFromHandle(handle) != entity ||
        HandleFromEntity(entity) != handle ||
        reinterpret_cast<CEntityInstance*>(entity)->GetRefEHandle().ToInt() != static_cast<std::uint32_t>(handle)) {
        return false;
    }
    const auto classname = s2sdk::GetEntityClassname(handle);
    const std::string_view value(classname.c_str());
    return value == funrounds::native::grenade::ProjectileClass(funrounds::native::grenade::Kind::Smoke) ||
           value == funrounds::native::grenade::ProjectileClass(funrounds::native::grenade::Kind::Molotov);
}

void RetireStalePending() noexcept {
    std::erase_if(g_pending, [](const PendingDetonation& pending) {
        try {
            return pending.handle == -1 || !s2sdk::IsValidEntHandle(pending.handle) ||
                   EntityFromHandle(pending.handle) != pending.entity || HandleFromEntity(pending.entity) != pending.handle ||
                   reinterpret_cast<CEntityInstance*>(pending.entity)->GetRefEHandle().ToInt() != static_cast<std::uint32_t>(pending.handle);
        } catch (...) {
            return true;
        }
    });
}

bool IsPending(std::int32_t handle) noexcept {
    return std::any_of(g_pending.begin(), g_pending.end(), [handle](const PendingDetonation& item) {
        return item.handle == handle;
    });
}

bool IsPending(std::int32_t handle, void* entity) noexcept {
    return std::any_of(g_pending.begin(), g_pending.end(), [handle, entity](const PendingDetonation& item) {
        return item.handle == handle && item.entity == entity;
    });
}

bool RemoveHook(DetonateHook& state) noexcept;

bool ClaimDetonation(std::int32_t handle, void* entity) noexcept {
    RetireStalePending();
    if (IsPending(handle)) return false;
    try {
        g_pending.push_back({handle, entity});
        return true;
    } catch (...) {
        return false;
    }
}

bool InstallDetonateHook(funrounds::native::grenade::Kind kind, void* address) noexcept {
    auto& state = HookFor(kind);
    auto& other = kind == funrounds::native::grenade::Kind::Smoke ? g_molotovHook : g_smokeHook;
    if (!address) return false;
    try {
        if (other.address == address) {
            const auto current = polyhook::FindDetour(address);
            if (!other.handle && other.attempted) other.handle = current;
            return current && other.handle == current &&
                   polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, DetonateEntry);
        }
        if (state.address) {
            const auto current = polyhook::FindDetour(state.address);
            if (!state.handle && state.attempted) state.handle = current;
            return current && state.handle == current &&
                   polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, DetonateEntry);
        }
        if (polyhook::FindDetour(address)) return false;

        const plg::vector<polyhook::DataType> arguments{polyhook::DataType::Pointer};
        state.address = address;
        state.attempted = true;
        state.handle = polyhook::HookDetour(address,
                                            polyhook::DataType::Void,
                                            arguments,
                                            -1,
                                            plg::string(kind == funrounds::native::grenade::Kind::Smoke
                                                            ? "FunRounds::SmokeGrenadeProjectile::Detonate"
                                                            : "FunRounds::MolotovProjectile::Detonate"));
        if (!state.handle) {
            (void)RemoveHook(state);
            return false;
        }
        (void)polyhook::AddCallback(state.handle, polyhook::CallbackType::Pre, DetonateEntry);
        if (!polyhook::IsCallbackRegistered(state.handle, polyhook::CallbackType::Pre, DetonateEntry)) {
            (void)RemoveHook(state);
            return false;
        }
        return true;
    } catch (...) {
        (void)RemoveHook(state);
        return false;
    }
}

bool RemoveHook(DetonateHook& state) noexcept {
    try {
        if (!state.address) return !state.handle;
        const auto current = polyhook::FindDetour(state.address);
        if (!current) {
            state = {};
            return true;
        }
        if (!state.handle && state.attempted) state.handle = current;
        if (!state.handle || current != state.handle) return false;
        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, DetonateEntry)) {
            (void)polyhook::RemoveCallback(current, polyhook::CallbackType::Pre, DetonateEntry);
            if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, DetonateEntry)) return false;
        }
        if (polyhook::AreCallbacksRegistered(current)) {
            state = {};
            return true;
        }
        (void)polyhook::UnhookDetour(state.address);
        if (polyhook::FindDetour(state.address)) return false;
        state = {};
        return true;
    } catch (...) {
        return false;
    }
}

polyhook::ResultType DetonateEntry(polyhook::HookHandle,
                                   polyhook::ParametersHandle parameters,
                                   std::int32_t,
                                   polyhook::ReturnHandle,
                                   polyhook::CallbackType type) noexcept {
    if (type != polyhook::CallbackType::Pre || !parameters) return polyhook::ResultType::Ignored;
    try {
        auto* const entity = polyhook::GetArgument<void*>(parameters, 0);
        if (!entity) return polyhook::ResultType::Ignored;
        const auto handle = HandleFromEntity(entity);
        if (!IsKnownGrenadeProjectile(handle, entity)) return polyhook::ResultType::Ignored;

        // This pre-entry claim catches natural fuse/collision paths as well as our
        // direct request. For smoke it is set before the asynchronous volume build
        // can emit smokegrenade_detonate or set m_bDidSmokeEffect.
        (void)ClaimDetonation(handle, entity);
    } catch (...) {
    }
    return polyhook::ResultType::Ignored;
}

} // namespace

bool HasGrenadeDetonateSupport(std::int32_t value) noexcept {
    if (!funrounds::native::grenade::ValidKind(value)) return false;
    const auto kind = static_cast<funrounds::native::grenade::Kind>(value);
    const auto fail = [kind](const char* reason) noexcept {
        LogSupportFailure(kind, reason);
        return false;
    };

    try {
        if (const auto* missing = MissingRequiredImport()) return fail(missing);
        if (!EnsureEngine()) return fail("engine schema or native gamedata is not ready");
        if (!RegisterMapEndCallback()) return fail("map-end claim cleanup callback could not be registered");
        void* const detonate = ProjectileDetonate(kind);
        if (!detonate) return fail("configured projectile vtable or Detonate offset did not resolve");
        if (!InstallDetonateHook(kind, detonate)) return fail("Detonate entry hook is unavailable or not owned");
        return true;
    } catch (...) {
        return fail("support probe threw while resolving the configured native entry");
    }
}

std::int32_t GetGrenadeDetonationState(std::int32_t projectileHandle, std::int32_t value) noexcept {
    using funrounds::native::grenade::StateResult;
    using funrounds::native::grenade::StateValue;
    if (!funrounds::native::grenade::ValidKind(value) || projectileHandle == -1) {
        return StateValue(StateResult::Failed);
    }
    const auto kind = static_cast<funrounds::native::grenade::Kind>(value);
    try {
        if (!HasGrenadeDetonateSupport(value)) return StateValue(StateResult::Failed);
        if (!s2sdk::IsValidEntHandle(projectileHandle)) return StateValue(StateResult::StartedOrGone);

        // Validate the live full handle and its concrete projectile class before
        // consulting the shared natural/direct Detonate-entry claim set.
        void* entity{};
        if (!CurrentProjectile(projectileHandle, kind, entity)) {
            return StateValue(s2sdk::IsValidEntHandle(projectileHandle)
                                  ? StateResult::Failed
                                  : StateResult::StartedOrGone);
        }
        return StateValue(IsPending(projectileHandle, entity) ? StateResult::StartedOrGone
                                                              : StateResult::NotStarted);
    } catch (...) {
        return StateValue(StateResult::Failed);
    }
}

std::int32_t DetonateGrenade(std::int32_t projectileHandle, std::int32_t value) noexcept {
    using funrounds::native::grenade::RequestResult;
    using funrounds::native::grenade::ResultValue;
    if (!funrounds::native::grenade::ValidKind(value)) return ResultValue(RequestResult::Failed);
    const auto kind = static_cast<funrounds::native::grenade::Kind>(value);
    try {
        if (!HasGrenadeDetonateSupport(value)) return ResultValue(RequestResult::Failed);
        if (projectileHandle == -1) return ResultValue(RequestResult::Failed);
        // A non-sentinel full handle can become invalid between the adapter's
        // last read and this request when a natural fuse/collision wins the race.
        if (!s2sdk::IsValidEntHandle(projectileHandle)) return ResultValue(RequestResult::AlreadyStartedOrGone);
        RetireStalePending();
        void* const detonate = ProjectileDetonate(kind);
        void* entity{};
        if (!detonate) return ResultValue(RequestResult::Failed);
        if (IsPending(projectileHandle)) return ResultValue(RequestResult::AlreadyStartedOrGone);
        if (!CurrentProjectile(projectileHandle, kind, entity)) {
            return ResultValue(s2sdk::IsValidEntHandle(projectileHandle)
                                   ? RequestResult::Failed
                                   : RequestResult::AlreadyStartedOrGone);
        }

        // Claim before dispatch so a second request in this frame cannot race the
        // entry callback. The hook also claims natural engine entry points.
        if (!ClaimDetonation(projectileHandle, entity)) {
            return ResultValue(IsPending(projectileHandle) ? RequestResult::AlreadyStartedOrGone
                                                           : RequestResult::Failed);
        }
        using Detonate = void (*)(void*);
        reinterpret_cast<Detonate>(detonate)(entity);
        return ResultValue(RequestResult::Submitted);
    } catch (...) {
        // Preserve the claim if dispatch began: retrying an ambiguous native call
        // could run the engine's effects twice.
        return ResultValue(RequestResult::Failed);
    }
}

bool ShutdownStandardGrenades() noexcept {
    // Remove the observers before discarding claims; a failed detach retains both
    // the hook identity and the pending handles for a later cleanup retry.
    const bool smokeClean = RemoveHook(g_smokeHook);
    const bool molotovClean = RemoveHook(g_molotovHook);
    if (!smokeClean || !molotovClean) return false;
    if (!UnregisterMapEndCallback()) return false;
    g_pending.clear();
    return true;
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasGrenadeDetonateSupport(std::int32_t kind) {
    return funrounds::native::detail::HasGrenadeDetonateSupport(kind);
}

extern "C" PLUGIN_API std::int32_t GetGrenadeDetonationState(std::int32_t projectileHandle, std::int32_t kind) {
    return funrounds::native::detail::GetGrenadeDetonationState(projectileHandle, kind);
}

extern "C" PLUGIN_API std::int32_t DetonateGrenade(std::int32_t projectileHandle, std::int32_t kind) {
    return funrounds::native::detail::DetonateGrenade(projectileHandle, kind);
}
