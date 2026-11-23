// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "touch.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cstdint>
#include <memory>
#include <vector>

#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"

namespace funrounds::native::detail {
namespace {

using TouchCallbacks = std::vector<EntityTouchPreCallback>;
using TouchCallbackSnapshot = std::shared_ptr<const TouchCallbacks>;
using TouchCallbackSlot = std::atomic<TouchCallbackSnapshot>;

struct TouchHook {
    void* address;
    polyhook::HookHandle handle;
    bool callbackRegistered;
    std::shared_ptr<TouchCallbackSlot> callbacks;
};

std::vector<TouchHook> g_hooks;
#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
bool g_probeFailNextTouchAdd = false;
bool g_probeFailNextTouchRemove = false;

std::int32_t TouchRetryProbeCallback(std::int32_t, std::int32_t) {
    return 0;
}

std::int32_t TouchForeignProbeCallback(std::int32_t, std::int32_t) {
    // The foreign callback is only passed to UnhookTouch, but keep its body
    // distinct from TouchRetryProbeCallback so Release ICF cannot alias them.
    return 1;
}
#endif

polyhook::ResultType TouchPre(polyhook::HookHandle hook,
                              polyhook::ParametersHandle params,
                              std::int32_t,
                              polyhook::ReturnHandle,
                              polyhook::CallbackType type) noexcept {
    if (type != polyhook::CallbackType::Pre) {
        return polyhook::ResultType::Ignored;
    }

    try {
        auto* entity = polyhook::GetArgument<void*>(params, 0);
        auto* other = polyhook::GetArgument<void*>(params, 1);
        if (!entity) {
            return polyhook::ResultType::Ignored;
        }

        const auto entityHandle = HandleFromEntity(entity);
        if (entityHandle == -1 || EntityFromHandle(entityHandle) != entity) {
            return polyhook::ResultType::Ignored;
        }
        auto otherHandle = other ? HandleFromEntity(other) : -1;
        if (otherHandle != -1 && EntityFromHandle(otherHandle) != other) {
            otherHandle = -1;
        }

        const auto record = std::find_if(g_hooks.begin(), g_hooks.end(), [hook](const auto& item) {
            return item.handle == hook;
        });
        if (record == g_hooks.end()) {
            return polyhook::ResultType::Ignored;
        }
        const auto callbacks = record->callbacks->load(std::memory_order_acquire);
        if (!callbacks || callbacks->empty()) {
            return polyhook::ResultType::Ignored;
        }

        bool suppressOriginal = false;
        for (const auto callback : *callbacks) {
            try {
                suppressOriginal = callback(entityHandle, otherHandle) != 0 || suppressOriginal;
            } catch (...) {
                // One consumer cannot prevent the other Touch consumers from running.
            }
        }
        return suppressOriginal ? polyhook::ResultType::Supercede : polyhook::ResultType::Ignored;
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
}

bool RemoveHook(TouchHook& record, bool unhookIfEmpty = true) noexcept {
    try {
        auto current = polyhook::FindDetour(record.address);
        if (!current) {
            record.handle = nullptr;
            record.callbackRegistered = false;
            return true;
        }
        if (current != record.handle) {
            return false;
        }
#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
        if (g_probeFailNextTouchRemove) {
            g_probeFailNextTouchRemove = false;
            return false;
        }
#endif

        const auto callbacks = record.callbacks ? record.callbacks->load(std::memory_order_acquire) : nullptr;
        if (callbacks && !callbacks->empty()) {
            return false;
        }
        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, TouchPre)) {
            (void)polyhook::RemoveCallback(current, polyhook::CallbackType::Pre, TouchPre);
        }
        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, TouchPre)) {
            return false;
        }
        record.callbackRegistered = false;

        // PolyHook handles are shared by all plugin consumers in the process.
        // The native provider may detach only after its callback was the last
        // callback registered on this implementation.
        if (polyhook::AreCallbacksRegistered(current)) {
            record.handle = nullptr;
            return true;
        }
        if (!unhookIfEmpty) {
            record.handle = nullptr;
            return true;
        }

        (void)polyhook::UnhookDetour(record.address);
        if (polyhook::FindDetour(record.address)) {
            return false;
        }
        record.handle = nullptr;
        return true;
    } catch (...) {
        return false;
    }
}

bool DetachAll() noexcept {
    bool detached = true;
    for (auto it = g_hooks.begin(); it != g_hooks.end();) {
        if (it->callbacks) {
            it->callbacks->store({}, std::memory_order_release);
        }
        if (RemoveHook(*it)) {
            it = g_hooks.erase(it);
        } else {
            detached = false;
            ++it;
        }
    }
    return detached;
}

bool HasCallback(const TouchHook& hook, EntityTouchPreCallback callback) noexcept {
    const auto callbacks = hook.callbacks ? hook.callbacks->load(std::memory_order_acquire) : nullptr;
    return callbacks && std::find(callbacks->begin(), callbacks->end(), callback) != callbacks->end();
}

bool AddCallback(TouchHook& hook, EntityTouchPreCallback callback) {
    if (!hook.callbacks) {
        return false;
    }
    auto callbacks = hook.callbacks->load(std::memory_order_acquire);
    if (callbacks && std::find(callbacks->begin(), callbacks->end(), callback) != callbacks->end()) {
        return true;
    }
    auto updated = std::make_shared<std::vector<EntityTouchPreCallback>>();
    if (callbacks) {
        *updated = *callbacks;
    }
    updated->push_back(callback);
    hook.callbacks->store(TouchCallbackSnapshot(std::move(updated)), std::memory_order_release);
    return true;
}

void RemoveCallback(TouchHook& hook, EntityTouchPreCallback callback) {
    if (!hook.callbacks) {
        return;
    }
    const auto callbacks = hook.callbacks->load(std::memory_order_acquire);
    if (!callbacks || std::find(callbacks->begin(), callbacks->end(), callback) == callbacks->end()) {
        return;
    }
    auto updated = std::make_shared<std::vector<EntityTouchPreCallback>>();
    updated->reserve(callbacks->size() - 1);
    for (const auto current : *callbacks) {
        if (current != callback) {
            updated->push_back(current);
        }
    }
    hook.callbacks->store(TouchCallbackSnapshot(std::move(updated)), std::memory_order_release);
}

bool AddTouchCallback(polyhook::HookHandle hook) {
#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
    if (g_probeFailNextTouchAdd) {
        g_probeFailNextTouchAdd = false;
        return false;
    }
#endif
    return polyhook::AddCallback(hook, polyhook::CallbackType::Pre, TouchPre);
}

bool GetTouchAddress(std::int32_t entityHandle, void*& implementation) {
    implementation = nullptr;
    if (!EnsureEngine() || entityHandle == -1) {
        return false;
    }
    void* entity = EntityFromHandle(entityHandle);
    if (!entity || HandleFromEntity(entity) != entityHandle) {
        return false;
    }

    const auto offset = s2sdk::GetGameConfigOffset(GameConfig(), "CBaseEntity::Touch");
    if (offset < 0) {
        return false;
    }
    auto** vtable = *reinterpret_cast<void***>(entity);
    if (!vtable || !vtable[offset]) {
        return false;
    }
    implementation = vtable[offset];
    return true;
}

} // namespace

bool IsTouchAvailable() {
    if (!EnsureEngine()) {
        return false;
    }
    const auto offset = s2sdk::GetGameConfigOffset(GameConfig(), "CBaseEntity::Touch");
    return offset >= 0;
}

bool HookTouch(std::int32_t entityHandle, EntityTouchPreCallback callback) {
    if (!callback || !__polyhook_HookDetour || !__polyhook_FindDetour ||
        !__polyhook_AddCallback || !__polyhook_IsCallbackRegistered ||
        !__polyhook_RemoveCallback || !__polyhook_UnhookDetour ||
        !__polyhook_AreCallbacksRegistered) {
        return false;
    }
    void* implementation = nullptr;
    if (!GetTouchAddress(entityHandle, implementation)) {
        return false;
    }

    const auto existing = std::find_if(g_hooks.begin(), g_hooks.end(), [implementation](const auto& hook) {
        return hook.address == implementation;
    });
    if (existing != g_hooks.end()) {
        if (existing->handle != polyhook::FindDetour(implementation)) {
            return false;
        }
        const bool alreadySubscribed = HasCallback(*existing, callback);
        if (!alreadySubscribed) {
            AddCallback(*existing, callback);
        }
        if (!existing->callbackRegistered ||
            !polyhook::IsCallbackRegistered(existing->handle, polyhook::CallbackType::Pre, TouchPre)) {
            try {
                (void)AddTouchCallback(existing->handle);
                existing->callbackRegistered =
                    polyhook::IsCallbackRegistered(existing->handle, polyhook::CallbackType::Pre, TouchPre);
            } catch (...) {
                existing->callbackRegistered =
                    polyhook::IsCallbackRegistered(existing->handle, polyhook::CallbackType::Pre, TouchPre);
            }
        }
        if (!existing->callbackRegistered && !alreadySubscribed) {
            RemoveCallback(*existing, callback);
        }
        return existing->callbackRegistered;
    }

    if (const auto current = polyhook::FindDetour(implementation); current != nullptr) {
        // This entry has a known typed ABI. A borrowed empty detour remains
        // the creator's cleanup responsibility until our callback is live.

        g_hooks.reserve(g_hooks.size() + 1);
        auto callbacks = std::make_shared<const std::vector<EntityTouchPreCallback>>(1, callback);
        g_hooks.push_back({implementation, current, false,
                           std::make_shared<TouchCallbackSlot>(TouchCallbackSnapshot(std::move(callbacks)))});
        auto& record = g_hooks.back();
        try {
            (void)AddTouchCallback(current);
            record.callbackRegistered =
                polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, TouchPre);
        } catch (...) {
            record.callbackRegistered =
                polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, TouchPre);
        }
        if (!record.callbackRegistered) {
            record.callbacks->store({}, std::memory_order_release);
            if (RemoveHook(record, false)) {
                g_hooks.pop_back();
            }
            return false;
        }
        return true;
    }

    g_hooks.reserve(g_hooks.size() + 1);
    constexpr std::array argumentKinds{polyhook::DataType::Pointer, polyhook::DataType::Pointer};
    const plg::vector<polyhook::DataType> argumentTypes(argumentKinds.begin(), argumentKinds.end());
    const auto hook = polyhook::HookDetour(implementation,
                                           polyhook::DataType::Void,
                                           argumentTypes,
                                           -1,
                                           "FunRounds::CBaseEntity::Touch");
    if (!hook) {
        return false;
    }

    g_hooks.push_back({implementation, hook, false,
                       std::make_shared<TouchCallbackSlot>(
                           std::make_shared<const std::vector<EntityTouchPreCallback>>(1, callback))});
    auto& record = g_hooks.back();
    try {
        (void)AddTouchCallback(hook);
        record.callbackRegistered = polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Pre, TouchPre);
    } catch (...) {
        record.callbackRegistered = false;
    }
    if (!record.callbackRegistered) {
        record.callbacks->store({}, std::memory_order_release);
        if (RemoveHook(record)) {
            g_hooks.pop_back();
        }
        return false;
    }
    return true;
}

#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
bool ProbeTouchUnhookContract(std::int32_t entityHandle) noexcept {
    if (g_owner || !g_hooks.empty()) {
        return false;
    }
    try {
        // Unhooking an unused callback is idempotent, while null is never a
        // valid callback identity.
        if (UnhookTouch(nullptr) || !UnhookTouch(TouchForeignProbeCallback) || !g_hooks.empty() ||
            g_owner || g_callback) {
            return false;
        }

        g_probeFailNextTouchAdd = true;
        g_probeFailNextTouchRemove = true;
        const bool firstHook = HookTouch(entityHandle, TouchRetryProbeCallback);
        const bool retained = g_owner == TouchRetryProbeCallback && g_hooks.size() == 1 &&
                              !g_hooks.front().callbackRegistered;
        const bool retryHook = HookTouch(entityHandle, TouchRetryProbeCallback);
        const bool ready = retryHook && g_callback == TouchRetryProbeCallback &&
                           g_hooks.front().callbackRegistered;

        // A different callback may unhook its own empty lease, but must not
        // disturb the callback that owns the live Touch detours.
        const bool foreignNoOp = UnhookTouch(TouchForeignProbeCallback);
        const bool ownerPreserved = g_owner == TouchRetryProbeCallback &&
                                    g_callback == TouchRetryProbeCallback && g_hooks.size() == 1 &&
                                    g_hooks.front().callbackRegistered &&
                                    polyhook::IsCallbackRegistered(g_hooks.front().handle,
                                                                   polyhook::CallbackType::Pre,
                                                                   TouchPre);

        // Keep a failed owner detach recorded and retryable. A no-owner call in
        // between must not erase or detach that retained owner state.
        g_probeFailNextTouchRemove = true;
        const bool firstDetach = UnhookTouch(TouchRetryProbeCallback);
        const bool retainedAfterDetachFailure = !firstDetach &&
                                                g_owner == TouchRetryProbeCallback &&
                                                !g_callback && g_hooks.size() == 1 &&
                                                g_hooks.front().callbackRegistered &&
                                                polyhook::IsCallbackRegistered(g_hooks.front().handle,
                                                                               polyhook::CallbackType::Pre,
                                                                               TouchPre);
        const bool foreignNoOpOnRetainedOwner = UnhookTouch(TouchForeignProbeCallback);
        const bool retainedAfterForeignNoOp = g_owner == TouchRetryProbeCallback &&
                                              g_hooks.size() == 1 &&
                                              !g_callback &&
                                              g_hooks.front().callbackRegistered &&
                                              polyhook::IsCallbackRegistered(g_hooks.front().handle,
                                                                             polyhook::CallbackType::Pre,
                                                                             TouchPre);
        const bool unhooked = UnhookTouch(TouchRetryProbeCallback);
        g_probeFailNextTouchAdd = false;
        g_probeFailNextTouchRemove = false;
        return !firstHook && retained && ready && foreignNoOp && ownerPreserved &&
               retainedAfterDetachFailure && foreignNoOpOnRetainedOwner && retainedAfterForeignNoOp &&
               unhooked && g_hooks.empty() && !g_owner && !g_callback;
    } catch (...) {
        g_probeFailNextTouchAdd = false;
        g_probeFailNextTouchRemove = false;
        (void)ShutdownTouch();
        return false;
    }
}
#endif

bool UnhookTouch(EntityTouchPreCallback callback) {
    if (!callback) {
        return false;
    }
    bool detached = true;
    for (auto it = g_hooks.begin(); it != g_hooks.end();) {
        RemoveCallback(*it, callback);
        const auto callbacks = it->callbacks ? it->callbacks->load(std::memory_order_acquire) : nullptr;
        if (callbacks && !callbacks->empty()) {
            ++it;
            continue;
        }
        if (RemoveHook(*it)) {
            it = g_hooks.erase(it);
        } else {
            detached = false;
            ++it;
        }
    }
    return detached;
}

bool ShutdownTouch() noexcept {
    return DetachAll();
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool IsEntityTouchPreHookAvailable() {
    try {
        return funrounds::native::detail::IsTouchAvailable();
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool HookEntityTouchPre(std::int32_t entityHandle,
                                               funrounds::native::detail::EntityTouchPreCallback callback) {
    try {
        return funrounds::native::detail::HookTouch(entityHandle, callback);
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool UnhookEntityTouchPre(
    funrounds::native::detail::EntityTouchPreCallback callback) {
    try {
        return funrounds::native::detail::UnhookTouch(callback);
    } catch (...) {
        return false;
    }
}
