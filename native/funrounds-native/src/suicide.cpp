#include "suicide.hpp"

#include <array>
#include <cstdint>
#include <cstring>

#include <entity2/entityidentity.h>
#include <entity2/entitynetwork.h>
#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "engine.hpp"

namespace funrounds::native::detail {
namespace {

constexpr std::int32_t kPickupMethod = 0;
constexpr std::int8_t kPickupBlocked = 8;
constexpr char kCanAcquireSignature[] = "CCSPlayer_ItemServices::CanAcquire";
constexpr char kCanAcquireHookName[] = "FunRounds::CCSPlayer_ItemServices::CanAcquire";
constexpr std::uint64_t kLookAtWeaponButton = std::uint64_t{1} << 35;
constexpr std::uint8_t kLifeAlive = 0;

struct InspectSchema {
    SchemaField movementServices{};
    SchemaField movementButtons{};
    SchemaField buttonStates{};
    SchemaField lifeState{};
    SchemaField itemServicesChain{};
    SchemaField pawnController{};
    bool ready{};
};

InspectSchema g_schema;

struct HookState {
    void* address{};
    polyhook::HookHandle handle{};
    PlayerCanAcquireCallback owner{};
    PlayerCanAcquireCallback callback{};
};

HookState g_hook;

polyhook::ResultType CanAcquirePre(polyhook::HookHandle,
                                   polyhook::ParametersHandle,
                                   std::int32_t,
                                   polyhook::ReturnHandle,
                                   polyhook::CallbackType) noexcept;

void* CanAcquireAddress() noexcept {
    if (!EnsureEngine() || !__s2sdk_GetGameConfigSignature) {
        return nullptr;
    }
    return s2sdk::GetGameConfigSignature(GameConfig(), kCanAcquireSignature);
}

bool ResolveItemServicesChain() noexcept {
    auto* schema = ServerSchema();
    if (!schema) {
        return false;
    }
    auto* componentClass = schema->FindDeclaredClass("CPlayerPawnComponent").Get();
    if (!componentClass || componentClass->m_nSize < 0 ||
        (componentClass->m_nFieldCount != 0 && !componentClass->m_pFields)) {
        return false;
    }

    for (std::uint16_t index = 0; index < componentClass->m_nFieldCount; ++index) {
        const auto& field = componentClass->m_pFields[index];
        if (!field.m_pszName || std::strcmp(field.m_pszName, "__m_pChainEntity") != 0) {
            continue;
        }
        if (!field.m_pType || field.m_pType->m_eTypeCategory != SCHEMA_TYPE_DECLARED_CLASS ||
            field.m_nSingleInheritanceOffset < 0) {
            return false;
        }

        const auto* declared = static_cast<const CSchemaType_DeclaredClass*>(field.m_pType);
        const auto* chainClass = declared->m_pClassInfo;
        if (!chainClass || !chainClass->m_pszName ||
            std::strcmp(chainClass->m_pszName, "CNetworkVarChainer") != 0 ||
            chainClass->m_nSize != static_cast<int>(sizeof(CNetworkVarChainer))) {
            return false;
        }

        int fieldSize = 0;
        std::uint8_t alignment = 0;
        if (!field.m_pType->GetSizeAndAlignment(fieldSize, alignment) ||
            fieldSize != static_cast<int>(sizeof(CNetworkVarChainer))) {
            return false;
        }

        const auto offset = static_cast<std::size_t>(field.m_nSingleInheritanceOffset);
        const auto objectSize = static_cast<std::size_t>(componentClass->m_nSize);
        if (offset > objectSize || sizeof(CNetworkVarChainer) > objectSize - offset) {
            return false;
        }
        g_schema.itemServicesChain = {field.m_nSingleInheritanceOffset, sizeof(CNetworkVarChainer)};
        return true;
    }
    return false;
}

bool HasInspectSchema() noexcept {
    if (g_schema.ready) {
        return true;
    }
    if (!EnsureEngine() ||
        !ResolveSchemaField("CBasePlayerPawn", "m_pMovementServices", sizeof(void*), g_schema.movementServices) ||
        !ResolveSchemaField("CPlayer_MovementServices", "m_nButtons", 0, g_schema.movementButtons) ||
        !ResolveSchemaField("CInButtonState", "m_pButtonStates", sizeof(std::uint64_t[3]), g_schema.buttonStates) ||
        !ResolveSchemaField("CBaseEntity", "m_lifeState", sizeof(std::uint8_t), g_schema.lifeState) ||
        !ResolveItemServicesChain() ||
        !ResolveSchemaField("CBasePlayerPawn", "m_hController", sizeof(std::uint32_t), g_schema.pawnController)) {
        return false;
    }
    const auto buttonsEnd = static_cast<std::size_t>(g_schema.buttonStates.offset) + g_schema.buttonStates.size;
    if (g_schema.movementButtons.offset < 0 || g_schema.buttonStates.offset < 0 ||
        buttonsEnd > g_schema.movementButtons.size || g_schema.movementServices.offset < 0 ||
        g_schema.lifeState.offset < 0 || g_schema.itemServicesChain.offset < 0 ||
        g_schema.pawnController.offset < 0) {
        g_schema = {};
        return false;
    }
    g_schema.ready = true;
    return true;
}

template <typename T>
T ReadField(const void* object, const SchemaField& field) noexcept {
    T value{};
    if (object && field.offset >= 0 && field.size >= sizeof(T)) {
        std::memcpy(&value, static_cast<const std::byte*>(object) + field.offset, sizeof(T));
    }
    return value;
}

void* MovementServices(void* pawn) noexcept {
    return ReadField<void*>(pawn, g_schema.movementServices);
}

std::uint8_t LifeState(void* pawn) noexcept {
    return ReadField<std::uint8_t>(pawn, g_schema.lifeState);
}

std::int32_t PawnForItemServices(void* services) noexcept {
    if (!services || g_schema.itemServicesChain.offset < 0 ||
        g_schema.itemServicesChain.size < sizeof(CNetworkVarChainer)) {
        return -1;
    }
    auto* chain = reinterpret_cast<CNetworkVarChainer*>(
        static_cast<std::byte*>(services) + g_schema.itemServicesChain.offset);
    auto* pawn = chain->GetObject();
    const auto handle = HandleFromEntity(pawn);
    return pawn && IsPlayerPawn(handle) && HandleFromEntity(pawn) == handle ? handle : -1;
}

bool RemoveHook() noexcept {
    g_hook.callback = nullptr;
    if (!g_hook.address) {
        g_hook = {};
        return true;
    }
    try {
        const auto current = polyhook::FindDetour(g_hook.address);
        if (!current) {
            g_hook = {};
            return true;
        }
        if (current != g_hook.handle) {
            return false;
        }
        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, CanAcquirePre)) {
            (void)polyhook::RemoveCallback(current, polyhook::CallbackType::Pre, CanAcquirePre);
        }
        if (polyhook::IsCallbackRegistered(current, polyhook::CallbackType::Pre, CanAcquirePre)) {
            return false;
        }
        (void)polyhook::UnhookDetour(g_hook.address);
        if (polyhook::FindDetour(g_hook.address)) {
            return false;
        }
        g_hook = {};
        return true;
    } catch (...) {
        return false;
    }
}

polyhook::ResultType CanAcquirePre(polyhook::HookHandle,
                                   polyhook::ParametersHandle params,
                                   std::int32_t,
                                   polyhook::ReturnHandle ret,
                                   polyhook::CallbackType type) noexcept {
    if (type != polyhook::CallbackType::Pre || !g_hook.callback) {
        return polyhook::ResultType::Ignored;
    }
    try {
        auto* services = polyhook::GetArgument<void*>(params, 0);
        const auto method = polyhook::GetArgument<std::int32_t>(params, 2);
        if (method != kPickupMethod || !services) {
            return polyhook::ResultType::Ignored;
        }
        const auto pawnHandle = PawnForItemServices(services);
        void* pawn = EntityFromHandle(pawnHandle);
        if (pawnHandle == -1 || !pawn || LifeState(pawn) != kLifeAlive) {
            return polyhook::ResultType::Ignored;
        }
        const auto controllerHandle = static_cast<std::int32_t>(ReadField<std::uint32_t>(pawn, g_schema.pawnController));
        auto* controller = EntityFromHandle(controllerHandle);
        if (controllerHandle == -1 || HandleFromEntity(pawn) != pawnHandle ||
            !IsPlayerController(controllerHandle) || !controller ||
            HandleFromEntity(controller) != controllerHandle) {
            return polyhook::ResultType::Ignored;
        }
        if (g_hook.callback(pawnHandle, controllerHandle) == 0) {
            return polyhook::ResultType::Ignored;
        }
        polyhook::SetReturn<std::int8_t>(ret, kPickupBlocked);
        return polyhook::ResultType::Supercede;
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
}

} // namespace

bool HasSuicideSupport() {
    const auto address = CanAcquireAddress();
    const bool inspectSchemaReady = HasInspectSchema();
    const bool polyhookReady = __polyhook_HookDetour && __polyhook_FindDetour && __polyhook_AddCallback &&
                               __polyhook_IsCallbackRegistered && __polyhook_RemoveCallback &&
                               __polyhook_UnhookDetour;
    return address && inspectSchemaReady && polyhookReady;
}

std::int32_t LookAtWeaponButtonState(std::int32_t pawnHandle) {
    if (pawnHandle == -1 || !HasInspectSchema() || !s2sdk::IsEntityPlayerPawn(pawnHandle)) {
        return -1;
    }
    void* pawn = EntityFromHandle(pawnHandle);
    if (!pawn || HandleFromEntity(pawn) != pawnHandle || LifeState(pawn) != kLifeAlive) {
        return -1;
    }
    void* movement = MovementServices(pawn);
    if (!movement) {
        return -1;
    }
    const auto buttons = ReadField<std::uint64_t>(static_cast<std::byte*>(movement) + g_schema.movementButtons.offset, g_schema.buttonStates);
    return (buttons & kLookAtWeaponButton) != 0 ? 1 : 0;
}

bool HookPlayerCanAcquire(PlayerCanAcquireCallback callback) {
    if (!callback) {
        return false;
    }
    if (!HasSuicideSupport()) {
        return false;
    }
    if (g_hook.owner && g_hook.owner != callback) {
        return false;
    }
    const auto address = CanAcquireAddress();
    if (!address || (g_hook.address && g_hook.address != address)) {
        return false;
    }
    if (g_hook.address) {
        if (polyhook::FindDetour(address) != g_hook.handle) {
            return false;
        }
        try {
            if (!polyhook::IsCallbackRegistered(g_hook.handle, polyhook::CallbackType::Pre, CanAcquirePre)) {
                (void)polyhook::AddCallback(g_hook.handle, polyhook::CallbackType::Pre, CanAcquirePre);
            }
            if (!polyhook::IsCallbackRegistered(g_hook.handle, polyhook::CallbackType::Pre, CanAcquirePre)) {
                return false;
            }
            g_hook.owner = callback;
            g_hook.callback = callback;
            return true;
        } catch (...) {
            return false;
        }
    }
    if (polyhook::FindDetour(address)) {
        return false;
    }

    constexpr std::array argumentKinds{polyhook::DataType::Pointer,
                                       polyhook::DataType::Pointer,
                                       polyhook::DataType::Int32,
                                       polyhook::DataType::Pointer};
    const plg::vector<polyhook::DataType> argumentTypes(argumentKinds.begin(), argumentKinds.end());
    const auto hook = polyhook::HookDetour(address, polyhook::DataType::Int8,
                                           argumentTypes, -1,
                                           kCanAcquireHookName);
    if (!hook) {
        return false;
    }
    g_hook = {address, hook, callback, nullptr};
    try {
        (void)polyhook::AddCallback(hook, polyhook::CallbackType::Pre, CanAcquirePre);
        if (!polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Pre, CanAcquirePre)) {
            (void)RemoveHook();
            return false;
        }
        g_hook.callback = callback;
        return true;
    } catch (...) {
        (void)RemoveHook();
        return false;
    }
}

bool UnhookPlayerCanAcquire(PlayerCanAcquireCallback callback) {
    if (!callback || callback != g_hook.owner) {
        return false;
    }
    return RemoveHook();
}

bool ShutdownSuicide() noexcept {
    return RemoveHook();
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasSuicideSupport() {
    try {
        return funrounds::native::detail::HasSuicideSupport();
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API std::int32_t GetLookAtWeaponButtonState(std::int32_t pawnHandle) {
    try {
        return funrounds::native::detail::LookAtWeaponButtonState(pawnHandle);
    } catch (...) {
        return -1;
    }
}

extern "C" PLUGIN_API bool HookPlayerCanAcquire(
    funrounds::native::detail::PlayerCanAcquireCallback callback) {
    try {
        return funrounds::native::detail::HookPlayerCanAcquire(callback);
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool UnhookPlayerCanAcquire(
    funrounds::native::detail::PlayerCanAcquireCallback callback) {
    try {
        return funrounds::native::detail::UnhookPlayerCanAcquire(callback);
    } catch (...) {
        return false;
    }
}
