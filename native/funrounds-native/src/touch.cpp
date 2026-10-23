#include "touch.hpp"

#include <algorithm>
#include <array>
#include <cstdint>
#include <vector>

#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"

namespace funrounds::native::detail {
namespace {

struct TouchHook {
    void* address;
    polyhook::HookHandle handle;
    bool callbackRegistered;
};

std::vector<TouchHook> g_hooks;
EntityTouchPreCallback g_owner = nullptr;
EntityTouchPreCallback g_callback = nullptr;

polyhook::ResultType TouchPre(polyhook::HookHandle,
                              polyhook::ParametersHandle params,
                              std::int32_t,
                              polyhook::ReturnHandle,
                              polyhook::CallbackType type) noexcept {
    if (type != polyhook::CallbackType::Pre) {
        return polyhook::ResultType::Ignored;
    }

    const auto callback = g_callback;
    if (!callback) {
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

        return callback(entityHandle, otherHandle) == 0 ? polyhook::ResultType::Ignored
                                                        : polyhook::ResultType::Supercede;
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
}

bool RemoveHook(TouchHook& record) noexcept {
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

        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, TouchPre)) {
            (void)polyhook::RemoveCallback(current, polyhook::CallbackType::Pre, TouchPre);
        }
        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, TouchPre)) {
            return false;
        }
        record.callbackRegistered = false;

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
    g_callback = nullptr;
    bool detached = true;
    for (auto it = g_hooks.begin(); it != g_hooks.end();) {
        if (RemoveHook(*it)) {
            it = g_hooks.erase(it);
        } else {
            detached = false;
            ++it;
        }
    }
    if (g_hooks.empty()) {
        g_owner = nullptr;
    }
    return detached;
}

bool AddTouchCallback(polyhook::HookHandle hook) {
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
        !__polyhook_RemoveCallback || !__polyhook_UnhookDetour) {
        return false;
    }
    if (g_owner && g_owner != callback) {
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
        if (existing->callbackRegistered &&
            polyhook::IsCallbackRegistered(existing->handle, polyhook::CallbackType::Pre, TouchPre)) {
            g_callback = callback;
            return true;
        }

        try {
            (void)AddTouchCallback(existing->handle);
            existing->callbackRegistered =
                polyhook::IsCallbackRegistered(existing->handle, polyhook::CallbackType::Pre, TouchPre);
        } catch (...) {
            existing->callbackRegistered = false;
        }
        if (existing->callbackRegistered) {
            g_callback = callback;
            return true;
        }

        if (RemoveHook(*existing)) {
            g_hooks.erase(existing);
            if (g_hooks.empty()) {
                g_owner = nullptr;
                g_callback = nullptr;
            }
        }
        return false;
    }
    if (polyhook::FindDetour(implementation)) {
        return false;
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

    g_owner = callback;
    g_hooks.push_back({implementation, hook, false});
    auto& record = g_hooks.back();
    try {
        (void)AddTouchCallback(hook);
        record.callbackRegistered = polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Pre, TouchPre);
    } catch (...) {
        record.callbackRegistered = false;
    }
    if (!record.callbackRegistered) {
        if (RemoveHook(record)) {
            g_hooks.pop_back();
            if (g_hooks.empty()) {
                g_owner = nullptr;
                g_callback = nullptr;
            }
        }
        return false;
    }

    g_callback = callback;
    return true;
}

bool UnhookTouch(EntityTouchPreCallback callback) {
    if (!callback || callback != g_owner) {
        return false;
    }
    return DetachAll();
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
