// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include "damage.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <memory>
#include <new>
#include <string_view>

#include <gametrace.h>
#include <mathlib/vectorws.h>
#include <navphysicsinterface.h>
#include <schemasystem/schemasystem.h>
#include <schemalib/schemaenuminfo.h>
#include <tier0/utlleanvector.h>

#include <polyhook/polyhook.hpp>
#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "damage_call_scope.hpp"
#include "damage_frame_stack.hpp"
#include "engine.hpp"
#include "entity_handle.hpp"

namespace funrounds::native::detail {
namespace {

constexpr std::int32_t kActionAllow = 0;
constexpr std::int32_t kWeaponOther = 0;
constexpr std::int32_t kWeaponFirearm = 1;
constexpr std::int32_t kWeaponKnife = 2;
constexpr std::int32_t kWeaponZeus = 3;
constexpr std::uint32_t kDamageBlast = 0x40;
constexpr std::uint32_t kDamageClub = 0x80;
constexpr std::uint32_t kDamageShock = 0x100;
constexpr std::uint32_t kDamageBurn = 0x8;
constexpr std::uint32_t kDamageFall = 0x20;
constexpr std::uint32_t kDamageDrown = 0x4000;
constexpr std::uint32_t kDamageSlash = 0x4;
constexpr std::uint32_t kDamageBullet = 0x2;
constexpr std::uint32_t kDamageBlastSurface = 0x400000;
constexpr std::uint32_t kDamageBuckshot = 0x1000000;
constexpr std::int32_t kHitGroupInvalid = -1;
constexpr std::int32_t kHitGroupHead = 1;
constexpr std::uint32_t kInvalidEntityHandle = 0xffffffffU;
constexpr std::string_view kDamageHookName = "FunRounds::CBaseEntity::TakeDamageOld";

struct DamageInfoOffsets {
    SchemaField inflictorHandle{};
    SchemaField attackerHandle{};
    SchemaField abilityHandle{};
    SchemaField damage{};
    SchemaField originalDamage{};
    SchemaField damageType{};
    SchemaField shouldBleed{};
    SchemaField shouldSpark{};
    SchemaField damageFlags{};
    SchemaField hitGroupId{};
    std::int32_t trace = -1;
};

struct DamageResultOffsets {
    SchemaField healthLost{};
    SchemaField damageDealt{};
    SchemaField totalledHealthLost{};
    SchemaField totalledDamageDealt{};
    SchemaField damageSuppressed{};
};

struct CTakeDamageInfo;

struct CTakeDamageResult {
    CTakeDamageInfo* originatingInfo;
    CUtlLeanVector<int> destructibleHitGroupRequests;
    std::int32_t healthLost;
    std::int32_t healthBefore;
    float damageDealt;
    float preModifiedDamage;
    VectorWS damagePosition;
    std::int32_t totalledHealthLost;
    float totalledDamageDealt;
    float totalledPreModifiedDamage;
    float newDamageAccumulatorValue;
    std::uint64_t damageFlags;
    bool damageSuppressed;
    bool suppressFlinch;
    std::int32_t overrideFlinchHitGroup;
    std::uint8_t unknown[8];

    static std::int32_t DamageToHealthLost(float value) noexcept {
        if (!std::isfinite(value)) {
            return 0;
        }
        if (value >= static_cast<float>(std::numeric_limits<std::int32_t>::max())) {
            return std::numeric_limits<std::int32_t>::max();
        }
        if (value <= static_cast<float>(std::numeric_limits<std::int32_t>::min())) {
            return std::numeric_limits<std::int32_t>::min();
        }
        return static_cast<std::int32_t>(value);
    }

    explicit CTakeDamageResult(float value)
        : originatingInfo(nullptr),
          destructibleHitGroupRequests(),
          healthLost(DamageToHealthLost(value)),
          healthBefore(0),
          damageDealt(value),
          preModifiedDamage(value),
          damagePosition{},
          totalledHealthLost(DamageToHealthLost(value)),
          totalledDamageDealt(value),
          totalledPreModifiedDamage(value),
          newDamageAccumulatorValue(0.0f),
          damageFlags(0),
          damageSuppressed(false),
          suppressFlinch(false),
          overrideFlinchHitGroup(0),
          unknown{} {}

    void CopyFrom(CTakeDamageInfo* info, float value) noexcept {
        originatingInfo = info;
        healthLost = DamageToHealthLost(value);
        healthBefore = 0;
        damageDealt = value;
        preModifiedDamage = value;
        damagePosition = {};
        totalledHealthLost = DamageToHealthLost(value);
        totalledDamageDealt = value;
        totalledPreModifiedDamage = value;
        newDamageAccumulatorValue = 0.0f;
        damageFlags = 0;
        damageSuppressed = false;
        suppressFlinch = false;
        overrideFlinchHitGroup = 0;
        std::memset(unknown, 0, sizeof(unknown));
    }
};
static_assert(sizeof(CTakeDamageResult) == 96);

struct DamageState {
    DamageInfoOffsets info{};
    DamageResultOffsets result{};
    std::int32_t infoSize = 0;
    std::int32_t resultSize = 0;
    CSchemaClassInfo* infoClass = nullptr;
    std::uint32_t headshotMask = 0;
    void* takeDamageAddress = nullptr;
    void* damageConstructorAddress = nullptr;
    void** navVtable = nullptr;
    polyhook::HookHandle hook = nullptr;
    DamagePreCallback owner = nullptr;
    DamagePreCallback callback = nullptr;
    EntityDamagePreCallback entityOwner = nullptr;
    EntityDamagePreCallback entityCallback = nullptr;
    bool postInstalled = false;
    bool commonReady = false;
    bool traceReady = false;
    bool ready = false;
};

DamageState g_damage;
#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
bool g_probeFailDamagePostRegistration = false;
bool g_probeFailDamagePostRemoval = false;
std::int32_t g_probeDamageTargetCalls = 0;
std::uintptr_t g_probeDamageTargetChecksum = 0;
polyhook::_RemoveCallback g_probeOriginalRemoveCallback = nullptr;
#if defined(_MSC_VER)
#define FUNROUNDS_DAMAGE_PROBE_NOINLINE __declspec(noinline)
#else
#define FUNROUNDS_DAMAGE_PROBE_NOINLINE __attribute__((noinline))
#endif

extern "C" FUNROUNDS_DAMAGE_PROBE_NOINLINE std::int64_t DamageRegistrationProbeTarget(void* first,
                                                                                       void* second,
                                                                                       void* third) {
    ++g_probeDamageTargetCalls;
    const auto firstValue = reinterpret_cast<std::uintptr_t>(first);
    const auto secondValue = reinterpret_cast<std::uintptr_t>(second);
    const auto thirdValue = reinterpret_cast<std::uintptr_t>(third);
    g_probeDamageTargetChecksum = ((firstValue << 1U) ^ secondValue) +
                                 ((thirdValue << 2U) ^ g_probeDamageTargetChecksum ^ 0x9E3779B97F4A7C15ULL);
    g_probeDamageTargetChecksum ^= static_cast<std::uintptr_t>(g_probeDamageTargetCalls);
    return static_cast<std::int64_t>(g_probeDamageTargetCalls + 41);
}

std::int32_t DamageRegistrationProbeCallback(std::int32_t,
                                             std::int32_t,
                                             std::int32_t,
                                             std::int32_t,
                                             std::int32_t,
                                             bool,
                                             float,
                                             std::uint32_t,
                                             float&,
                                             bool&,
                                             std::int32_t,
                                             bool) {
    return kActionAllow;
}

bool FailPostDamageRemovalOnce(polyhook::HookHandle hook,
                               polyhook::CallbackType type,
                               polyhook::CallbackHandler handler) {
    if (type == polyhook::CallbackType::Post && g_probeFailDamagePostRemoval) {
        g_probeFailDamagePostRemoval = false;
        throw std::bad_alloc{};
    }
    return g_probeOriginalRemoveCallback(hook, type, handler);
}

class ScopedDamageRemoveFailure {
public:
    ScopedDamageRemoveFailure() noexcept : original_(__polyhook_RemoveCallback) {
        g_probeOriginalRemoveCallback = original_;
        __polyhook_RemoveCallback = FailPostDamageRemovalOnce;
    }

    ~ScopedDamageRemoveFailure() {
        __polyhook_RemoveCallback = original_;
        g_probeOriginalRemoveCallback = nullptr;
    }

    ScopedDamageRemoveFailure(const ScopedDamageRemoveFailure&) = delete;
    ScopedDamageRemoveFailure& operator=(const ScopedDamageRemoveFailure&) = delete;

private:
    polyhook::_RemoveCallback original_;
};
#endif

polyhook::ResultType DamageHook(polyhook::HookHandle,
                                polyhook::ParametersHandle,
                                std::int32_t,
                                polyhook::ReturnHandle,
                                polyhook::CallbackType) noexcept;

template <typename T>
T ReadField(const void* source, std::int32_t offset) noexcept {
    T value{};
    std::memcpy(&value, static_cast<const std::byte*>(source) + offset, sizeof(value));
    return value;
}

template <typename T>
void WriteField(void* destination, std::int32_t offset, const T& value) noexcept {
    std::memcpy(static_cast<std::byte*>(destination) + offset, &value, sizeof(value));
}

bool ResolveDamageInfo() noexcept {
    auto* schema = ServerSchema();
    if (!schema) {
        return false;
    }
    g_damage.infoClass = schema->FindDeclaredClass("CTakeDamageInfo").Get();
    if (!g_damage.infoClass || g_damage.infoClass->m_nSize <= 0) {
        return false;
    }
    g_damage.infoSize = g_damage.infoClass->m_nSize;

    auto& info = g_damage.info;
    if (!ResolveSchemaField("CTakeDamageInfo", "m_hInflictor", sizeof(std::uint32_t), info.inflictorHandle) ||
        !ResolveSchemaField("CTakeDamageInfo", "m_hAttacker", sizeof(std::uint32_t), info.attackerHandle) ||
        !ResolveSchemaField("CTakeDamageInfo", "m_hAbility", sizeof(std::uint32_t), info.abilityHandle) ||
        !ResolveSchemaField("CTakeDamageInfo", "m_flDamage", sizeof(float), info.damage) ||
        !ResolveSchemaField("CTakeDamageInfo", "m_flOriginalDamage", sizeof(float), info.originalDamage) ||
        !ResolveSchemaField("CTakeDamageInfo", "m_bitsDamageType", sizeof(std::uint32_t), info.damageType) ||
        !ResolveSchemaField("CTakeDamageInfo", "m_bShouldBleed", sizeof(bool), info.shouldBleed) ||
        !ResolveSchemaField("CTakeDamageInfo", "m_bShouldSpark", sizeof(bool), info.shouldSpark) ||
        !ResolveSchemaField("CTakeDamageInfo", "m_nDamageFlags", sizeof(std::uint64_t), info.damageFlags) ||
        !ResolveSchemaField("CTakeDamageInfo", "m_iHitGroupId", sizeof(std::int32_t), info.hitGroupId)) {
        return false;
    }

    if (info.shouldBleed.offset != info.originalDamage.offset + static_cast<std::int32_t>(sizeof(float)) ||
        info.shouldSpark.offset != info.shouldBleed.offset + static_cast<std::int32_t>(sizeof(bool)) ||
        info.damageFlags.offset + static_cast<std::int32_t>(sizeof(std::uint64_t)) != info.hitGroupId.offset) {
        return false;
    }
    return true;
}

bool ResolveDamageTrace() noexcept {
    const auto& info = g_damage.info;
    const auto traceOffset = info.damageFlags.offset - static_cast<std::int32_t>(sizeof(CGameTrace*));
    const auto afterSpark = info.shouldSpark.offset + static_cast<std::int32_t>(sizeof(bool));
    const auto alignedTraceOffset = (afterSpark + static_cast<std::int32_t>(alignof(CGameTrace*)) - 1) &
                                    ~(static_cast<std::int32_t>(alignof(CGameTrace*)) - 1);
    if (traceOffset != alignedTraceOffset || traceOffset < 0 ||
        traceOffset > g_damage.infoSize - static_cast<std::int32_t>(sizeof(CGameTrace*))) {
        return false;
    }
    g_damage.info.trace = traceOffset;
    return true;
}

bool ResolveDamageResult() noexcept {
    auto* schema = ServerSchema();
    if (!schema) {
        return false;
    }
    auto* resultClass = schema->FindDeclaredClass("CTakeDamageResult").Get();
    if (!resultClass || resultClass->m_nSize != static_cast<std::int32_t>(sizeof(CTakeDamageResult))) {
        return false;
    }
    g_damage.resultSize = resultClass->m_nSize;

    auto& result = g_damage.result;
    if (!ResolveSchemaField("CTakeDamageResult", "m_nHealthLost", sizeof(std::int32_t), result.healthLost) ||
        !ResolveSchemaField("CTakeDamageResult", "m_flDamageDealt", sizeof(float), result.damageDealt) ||
        !ResolveSchemaField("CTakeDamageResult", "m_nTotalledHealthLost", sizeof(std::int32_t), result.totalledHealthLost) ||
        !ResolveSchemaField("CTakeDamageResult", "m_flTotalledDamageDealt", sizeof(float), result.totalledDamageDealt) ||
        !ResolveSchemaField("CTakeDamageResult", "m_bWasDamageSuppressed", sizeof(bool), result.damageSuppressed)) {
        return false;
    }

    return result.healthLost.offset == static_cast<std::int32_t>(offsetof(CTakeDamageResult, healthLost)) &&
           result.damageDealt.offset == static_cast<std::int32_t>(offsetof(CTakeDamageResult, damageDealt)) &&
           result.totalledHealthLost.offset == static_cast<std::int32_t>(offsetof(CTakeDamageResult, totalledHealthLost)) &&
           result.totalledDamageDealt.offset == static_cast<std::int32_t>(offsetof(CTakeDamageResult, totalledDamageDealt)) &&
           result.damageSuppressed.offset == static_cast<std::int32_t>(offsetof(CTakeDamageResult, damageSuppressed));
}

bool ResolveHeadshotMask() noexcept {
    auto* schema = ServerSchema();
    if (!schema) {
        return false;
    }
    const auto* damageTypes = schema->FindDeclaredEnum("DamageTypes_t").Get();
    if (!damageTypes || damageTypes->m_nSize != sizeof(std::uint32_t) || !damageTypes->m_pEnumerators) {
        return false;
    }
    for (std::uint16_t index = 0; index < damageTypes->m_nEnumeratorCount; ++index) {
        const auto& enumerator = damageTypes->m_pEnumerators[index];
        if (enumerator.m_pszName && std::string_view(enumerator.m_pszName) == "DMG_HEADSHOT" &&
            enumerator.m_nValue > 0 &&
            static_cast<std::uint64_t>(enumerator.m_nValue) <= std::numeric_limits<std::uint32_t>::max()) {
            const auto mask = static_cast<std::uint32_t>(enumerator.m_nValue);
            if ((mask & (mask - 1)) == 0) {
                g_damage.headshotMask = mask;
                return true;
            }
        }
    }
    return false;
}

bool ResolveNavPhysics() noexcept {
    if (!__s2sdk_GetGameConfigVTable) {
        return false;
    }
    auto* navVtable = static_cast<void**>(s2sdk::GetGameConfigVTable(GameConfig(), "CNavPhysicsInterface"));
    constexpr std::size_t kNavTraceIndex = 2;
    if (!navVtable || !navVtable[kNavTraceIndex]) {
        return false;
    }
    g_damage.navVtable = navVtable;
    return true;
}

bool ResolveDamageAddresses() noexcept {
    if (!__s2sdk_GetGameConfigSignature) {
        return false;
    }
    g_damage.takeDamageAddress = s2sdk::GetGameConfigSignature(GameConfig(), "CBaseEntity_TakeDamageOld");
    g_damage.damageConstructorAddress = s2sdk::GetGameConfigSignature(GameConfig(), "CTakeDamageInfo::Constructor");
    return g_damage.takeDamageAddress && g_damage.damageConstructorAddress;
}

bool IsWeaponEntity(void* entity) noexcept {
    if (!entity || !EnsureEngine() || !__s2sdk_GetGameConfigOffset) {
        return false;
    }

    const auto offset = s2sdk::GetGameConfigOffset(GameConfig(), "CBaseEntity::IsWeapon");
    if (offset < 0) {
        return false;
    }
    auto** vtable = *reinterpret_cast<void***>(entity);
    if (!vtable || !vtable[offset]) {
        return false;
    }

    using IsWeaponFunction = bool (*)(void*);
    return reinterpret_cast<IsWeaponFunction>(vtable[offset])(entity);
}

bool ResolveWeaponHandle(std::int32_t handle, void*& weapon) noexcept {
    weapon = nullptr;
    if (handle == -1) {
        return true;
    }
    weapon = EntityFromHandle(handle);
    return weapon && HandleFromEntity(weapon) == handle && IsWeaponEntity(weapon);
}

bool EnsureDamageCommonReady() {
    if (g_damage.commonReady) {
        return true;
    }
    if (!EnsureEngine() || !ResolveDamageInfo() || !ResolveDamageResult() ||
        !ResolveDamageAddresses()) {
        return false;
    }
    g_damage.commonReady = true;
    return true;
}

bool EnsureDamageReady() {
    if (g_damage.ready) {
        return true;
    }
    if (!EnsureDamageCommonReady() ||
        (!g_damage.traceReady && !(g_damage.traceReady = ResolveDamageTrace())) ||
        !ResolveHeadshotMask() || !ResolveNavPhysics()) {
        return false;
    }
    g_damage.ready = true;
    return true;
}

void* ResolvePlayerPawn(void* entity, std::int32_t handle) noexcept {
    if (!entity) {
        return nullptr;
    }
    if (IsPlayerPawn(handle)) {
        return entity;
    }
    if (!IsPlayerController(handle)) {
        return nullptr;
    }

    SchemaField playerPawn{};
    if (!ResolveSchemaField("CCSPlayerController", "m_hPlayerPawn", sizeof(std::uint32_t), playerPawn)) {
        return nullptr;
    }
    const auto pawnHandle = ToApiHandle(ReadField<std::uint32_t>(entity, playerPawn.offset));
    void* pawn = EntityFromHandle(pawnHandle);
    return pawn && HandleFromEntity(pawn) == pawnHandle && IsPlayerPawn(pawnHandle) ? pawn : nullptr;
}

std::int32_t ClassifyWeapon(void* attackerPawn, std::uint32_t damageType) noexcept {
    if (!attackerPawn ||
        (damageType & (kDamageBlast | kDamageBlastSurface | kDamageBurn | kDamageDrown | kDamageFall)) != 0) {
        return kWeaponOther;
    }

    SchemaField weaponServicesField{};
    SchemaField activeWeaponField{};
    SchemaField weaponTypeField{};
    if (!ResolveSchemaField("CBasePlayerPawn", "m_pWeaponServices", sizeof(void*), weaponServicesField) ||
        !ResolveSchemaField("CPlayer_WeaponServices", "m_hActiveWeapon", sizeof(std::uint32_t), activeWeaponField) ||
        !ResolveSchemaField("CCSWeaponBaseVData", "m_WeaponType", sizeof(std::uint32_t), weaponTypeField)) {
        return kWeaponOther;
    }

    void* weaponServices = ReadField<void*>(attackerPawn, weaponServicesField.offset);
    if (!weaponServices) {
        return kWeaponOther;
    }
    const auto activeWeaponHandle = ToApiHandle(ReadField<std::uint32_t>(weaponServices, activeWeaponField.offset));
    void* weapon = EntityFromHandle(activeWeaponHandle);
    if (!weapon || HandleFromEntity(weapon) != activeWeaponHandle) {
        return kWeaponOther;
    }

    SchemaField subclassId{};
    if (!ResolveSchemaField("CBaseEntity", "m_nSubclassID", sizeof(std::uint32_t), subclassId)) {
        return kWeaponOther;
    }
    void* weaponData = ReadField<void*>(weapon, subclassId.offset + static_cast<std::int32_t>(sizeof(std::uint32_t)));
    if (!weaponData) {
        return kWeaponOther;
    }

    const auto weaponType = ReadField<std::uint32_t>(weaponData, weaponTypeField.offset);
    switch (weaponType) {
        case 0: // knife
            return (damageType & (kDamageSlash | kDamageClub)) != 0 ? kWeaponKnife : kWeaponOther;
        case 8: // taser
            return (damageType & kDamageShock) != 0 ? kWeaponZeus : kWeaponOther;
        case 1: // pistol
        case 2: // submachine gun
        case 3: // rifle
        case 4: // shotgun
        case 5: // sniper rifle
        case 6: // machine gun
            return (damageType & (kDamageBullet | kDamageBuckshot)) != 0 ? kWeaponFirearm : kWeaponOther;
        default:
            return kWeaponOther;
    }
}

bool RegisterDamageCallbacks(polyhook::HookHandle hook,
                             DamagePreCallback playerCallback,
                             EntityDamagePreCallback entityCallback) {
    g_damage.owner = playerCallback;
    g_damage.callback = nullptr;
    g_damage.entityOwner = entityCallback;
    g_damage.entityCallback = nullptr;
    g_damage.postInstalled = false;

    try {
        (void)polyhook::AddCallback(hook, polyhook::CallbackType::Pre, DamageHook);
        if (!polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Pre, DamageHook)) {
            return false;
        }
#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
        if (g_probeFailDamagePostRegistration) {
            g_probeFailDamagePostRegistration = false;
            throw std::bad_alloc{};
        }
#endif
        (void)polyhook::AddCallback(hook, polyhook::CallbackType::Post, DamageHook);
        if (!polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Post, DamageHook)) {
            return false;
        }
    } catch (...) {
        return false;
    }

    g_damage.postInstalled = true;
    g_damage.callback = playerCallback;
    g_damage.entityCallback = entityCallback;
    DamageFrameStack::SetEnabled(playerCallback != nullptr || entityCallback != nullptr);
    return true;
}

bool TraceVictimHead(std::int32_t attackerPawnHandle, void* attackerPawn, void* victim) noexcept {
    if (!g_damage.navVtable || !attackerPawn || !victim || attackerPawnHandle == -1 ||
        EntityFromHandle(attackerPawnHandle) != attackerPawn || HandleFromEntity(attackerPawn) != attackerPawnHandle ||
        !IsPlayerPawn(attackerPawnHandle) ||
        !__s2sdk_GetEntityEyePosition || !__s2sdk_GetEntityEyeAngles) {
        return false;
    }

    const auto startValue = s2sdk::GetEntityEyePosition(attackerPawnHandle);
    const auto angleValue = s2sdk::GetEntityEyeAngles(attackerPawnHandle);
    Vector start{startValue.x, startValue.y, startValue.z};
    const float radians = 0.017453292519943295769f;
    const float pitch = angleValue.x * radians;
    const float yaw = angleValue.y * radians;
    const Vector end{start.x + std::cos(pitch) * std::cos(yaw) * 8192.0f,
                     start.y + std::cos(pitch) * std::sin(yaw) * 8192.0f,
                     start.z - std::sin(pitch) * 8192.0f};

    CGameTrace trace;
    // Keep the vtable pointer in g_damage: a local copy passed as an interface would dangle.
    auto* navPhysics = reinterpret_cast<INavPhysicsInterface*>(&g_damage.navVtable);
    navPhysics->Nav_TraceLine(start,
                              end,
                              reinterpret_cast<const CBaseEntity*>(attackerPawn),
                              MASK_SHOT,
                              static_cast<uint8>(COLLISION_GROUP_DEFAULT),
                              RNQUERY_OBJECTS_ALL,
                              &trace);
    return trace.m_pEnt == reinterpret_cast<CEntityInstance*>(victim) && trace.m_pHitbox &&
           trace.m_pHitbox->m_nGroupId == kHitGroupHead;
}

bool DetachDamageHook() noexcept {
    try {
        if (DamageFrameStack::ActiveCalls() != 0) {
            return false;
        }
        if (!g_damage.takeDamageAddress) {
            g_damage.hook = nullptr;
            g_damage.postInstalled = false;
            return true;
        }

        auto current = polyhook::FindDetour(g_damage.takeDamageAddress);
        if (!current) {
            g_damage.hook = nullptr;
            g_damage.postInstalled = false;
            return true;
        }
        if (!g_damage.hook || current != g_damage.hook) {
            return false;
        }

        // Stop both halves from touching the per-thread stack before a callback removal can throw.
        // A surviving Post callback after a partial detach must not underflow ActiveCalls.
        g_damage.postInstalled = false;
        for (const auto type : {polyhook::CallbackType::Pre, polyhook::CallbackType::Post}) {
            if (polyhook::IsCallbackRegistered(current, type, DamageHook)) {
                (void)polyhook::RemoveCallback(current, type, DamageHook);
            }
            if (polyhook::IsCallbackRegistered(current, type, DamageHook)) {
                return false;
            }
        }

        (void)polyhook::UnhookDetour(g_damage.takeDamageAddress);
        if (polyhook::FindDetour(g_damage.takeDamageAddress)) {
            return false;
        }
        g_damage.hook = nullptr;
        g_damage.takeDamageAddress = nullptr;
        g_damage.postInstalled = false;
        g_damage.commonReady = false;
        g_damage.traceReady = false;
        g_damage.ready = false;
        return true;
    } catch (...) {
        return false;
    }
}

void ClearDamageSubscriptions() noexcept {
    g_damage.owner = nullptr;
    g_damage.callback = nullptr;
    g_damage.entityOwner = nullptr;
    g_damage.entityCallback = nullptr;
    DamageFrameStack::SetEnabled(false);
}

void RefreshDamageFrameTracking() noexcept {
    DamageFrameStack::SetEnabled(g_damage.callback != nullptr || g_damage.entityCallback != nullptr);
}

void SuppressDamage(void* info, polyhook::ParametersHandle params, polyhook::ReturnHandle result) noexcept {
    WriteField(info, g_damage.info.shouldBleed.offset, false);
    WriteField(info, g_damage.info.shouldSpark.offset, false);
    auto* damageResult = polyhook::GetArgument<void*>(params, 2);
    if (damageResult) {
        const auto& offsets = g_damage.result;
        WriteField(damageResult, offsets.healthLost.offset, std::int32_t{0});
        WriteField(damageResult, offsets.damageDealt.offset, 0.0f);
        WriteField(damageResult, offsets.totalledHealthLost.offset, std::int32_t{0});
        WriteField(damageResult, offsets.totalledDamageDealt.offset, 0.0f);
        WriteField(damageResult, offsets.damageSuppressed.offset, true);
    }
    polyhook::SetReturn<std::int64_t>(result, 1);
}

polyhook::ResultType DamageHook(polyhook::HookHandle,
                                polyhook::ParametersHandle params,
                                std::int32_t,
                                polyhook::ReturnHandle result,
                                polyhook::CallbackType type) noexcept {
    if (type == polyhook::CallbackType::Post) {
        if (g_damage.postInstalled) {
            DamageFrameStack::Pop();
        }
        return polyhook::ResultType::Ignored;
    }

    if (!g_damage.postInstalled) {
        return polyhook::ResultType::Ignored;
    }
    DamageFrameStack::Push();
    try {
        auto* info = polyhook::GetArgument<void*>(params, 1);
        auto* victim = polyhook::GetArgument<void*>(params, 0);
        if (!info || !victim || !g_damage.commonReady) {
            return polyhook::ResultType::Ignored;
        }

        const auto victimHandle = HandleFromEntity(victim);
        if (victimHandle == -1) {
            return polyhook::ResultType::Ignored;
        }

        if (!IsPlayerPawn(victimHandle)) {
            const auto callback = g_damage.entityCallback;
            if (!callback || EntityFromHandle(victimHandle) != victim || HandleFromEntity(victim) != victimHandle) {
                return polyhook::ResultType::Ignored;
            }

            const auto& fields = g_damage.info;
            const auto attackerHandle = ToApiHandle(ReadField<std::uint32_t>(info, fields.attackerHandle.offset));
            const auto inflictorHandle = ToApiHandle(ReadField<std::uint32_t>(info, fields.inflictorHandle.offset));
            const auto abilityHandle = ToApiHandle(ReadField<std::uint32_t>(info, fields.abilityHandle.offset));
            const auto damageType = ReadField<std::uint32_t>(info, fields.damageType.offset);
            const auto damage = ReadField<float>(info, fields.damage.offset);
            const bool skipPluginRules = DamageCallScope<EntityDamagePreCallback>::SkipRules(info, callback);
            if (callback(victimHandle,
                         attackerHandle,
                         inflictorHandle,
                         abilityHandle,
                         damage,
                         damageType,
                         skipPluginRules) != kActionAllow) {
                SuppressDamage(info, params, result);
                return polyhook::ResultType::Supercede;
            }
            return polyhook::ResultType::Ignored;
        }

        const auto callback = g_damage.callback;
        if (!callback || !g_damage.ready) {
            return polyhook::ResultType::Ignored;
        }

        const auto& fields = g_damage.info;
        const auto attackerHandle = ToApiHandle(ReadField<std::uint32_t>(info, fields.attackerHandle.offset));
        const auto inflictorHandle = ToApiHandle(ReadField<std::uint32_t>(info, fields.inflictorHandle.offset));
        const auto abilityHandle = ToApiHandle(ReadField<std::uint32_t>(info, fields.abilityHandle.offset));
        const auto damageType = ReadField<std::uint32_t>(info, fields.damageType.offset);
        const auto damage = ReadField<float>(info, fields.damage.offset);
        auto* trace = ReadField<CGameTrace*>(info, fields.trace);
        auto* traceEntity = trace ? trace->m_pEnt : nullptr;
        const auto* traceHitbox = trace && traceEntity == reinterpret_cast<CEntityInstance*>(victim)
                                      ? trace->m_pHitbox
                                      : nullptr;
        const auto hitGroup = traceHitbox ? traceHitbox->m_nGroupId : kHitGroupInvalid;

        void* attackerEntity = EntityFromHandle(attackerHandle);
        if (attackerEntity && HandleFromEntity(attackerEntity) != attackerHandle) {
            attackerEntity = nullptr;
        }
        void* attackerPawn = ResolvePlayerPawn(attackerEntity, attackerHandle);
        const auto weaponKind = ClassifyWeapon(attackerPawn, damageType);
        const auto attackerPawnHandle = attackerPawn ? HandleFromEntity(attackerPawn) : -1;
        const bool tracedHead = (weaponKind == kWeaponKnife || weaponKind == kWeaponZeus) &&
                                TraceVictimHead(attackerPawnHandle, attackerPawn, victim);

        float multiplier = 1.0f;
        bool markHeadshot = false;
        const bool skipPluginRules = DamageCallScope<DamagePreCallback>::SkipRules(info, callback);
        const auto action = callback(victimHandle,
                                     attackerHandle,
                                     inflictorHandle,
                                     weaponKind,
                                     hitGroup,
                                     tracedHead,
                                     damage,
                                     damageType,
                                     multiplier,
                                     markHeadshot,
                                     abilityHandle,
                                     skipPluginRules);
        const bool blocked = action != kActionAllow || !std::isfinite(multiplier) || multiplier <= 0.0f;
        if (blocked) {
            SuppressDamage(info, params, result);
            return polyhook::ResultType::Supercede;
        }

        if (multiplier != 1.0f) {
            WriteField(info, fields.damage.offset, damage * multiplier);
            const auto original = ReadField<float>(info, fields.originalDamage.offset);
            WriteField(info, fields.originalDamage.offset, original * multiplier);
        }
        if (markHeadshot) {
            WriteField(info, fields.damageType.offset, damageType | g_damage.headshotMask);
            WriteField(info, fields.hitGroupId.offset, kHitGroupHead);
        }
        return polyhook::ResultType::Ignored;
    } catch (...) {
        return polyhook::ResultType::Ignored;
    }
}

using NativeDamageConstructor = void (*)(void*,
                                         CBaseEntity*,
                                         CBaseEntity*,
                                         CBaseEntity*,
                                         const Vector*,
                                         const Vector*,
                                         float,
                                         std::int32_t,
                                         std::int32_t,
                                         void*);
using TakeDamageFunction = std::int64_t (*)(CBaseEntity*, CTakeDamageInfo*, CTakeDamageResult*);

class NativeDamageInfoStorage {
public:
    explicit NativeDamageInfoStorage(CSchemaClassInfo* classInfo) noexcept : classInfo_(classInfo) {}
    ~NativeDamageInfoStorage() { (void)Destruct(); }

    bool Construct(NativeDamageConstructor constructor,
                   void* inflictor,
                   void* attacker,
                   void* weapon,
                   const Vector& force,
                   const Vector& position,
                   float damage,
                   std::int32_t damageType) {
        if (!classInfo_ || classInfo_->m_nSize <= 0 || !classInfo_->m_pfnManipulator) {
            return false;
        }
        const auto alignment = static_cast<std::size_t>(classInfo_->m_nAlignment);
        if (alignment == 0 || alignment > alignof(std::max_align_t) || (alignment & (alignment - 1)) != 0) {
            return false;
        }
        const auto constructFlags = classInfo_->m_nFlags1 &
                                    (SCHEMA_CF1_IS_ABSTRACT | SCHEMA_CF1_CONSTRUCT_ALLOWED |
                                     SCHEMA_CF1_CONSTRUCT_DISALLOWED);
        if (constructFlags != SCHEMA_CF1_CONSTRUCT_ALLOWED) {
            return false;
        }

        storage_.reset(new (std::nothrow) std::byte[static_cast<std::size_t>(classInfo_->m_nSize)]{});
        if (!storage_) {
            return false;
        }

        constructor(storage_.get(),
                    static_cast<CBaseEntity*>(inflictor),
                    static_cast<CBaseEntity*>(attacker),
                    static_cast<CBaseEntity*>(weapon),
                    &force,
                    &position,
                    damage,
                    damageType,
                    0,
                    nullptr);
        constructed_ = true;
        return true;
    }

    void* Get() const noexcept { return storage_.get(); }

    bool Destruct() noexcept {
        if (!constructed_) {
            return true;
        }
        constructed_ = false;
        return classInfo_->m_pfnManipulator(SCHEMA_CLASS_MANIPULATOR_ACTION_DESCTRUCT_IN_PLACE,
                                             storage_.get()) != nullptr;
    }

private:
    CSchemaClassInfo* classInfo_;
    std::unique_ptr<std::byte[]> storage_;
    bool constructed_ = false;
};

bool InstallDamageHook(DamagePreCallback playerCallback, EntityDamagePreCallback entityCallback) {
    if ((!playerCallback && !entityCallback) ||
        (playerCallback && !EnsureDamageReady()) ||
        (entityCallback && !EnsureDamageCommonReady()) || !__polyhook_HookDetour ||
        !__polyhook_AddCallback || !__polyhook_IsCallbackRegistered || !__polyhook_FindDetour) {
        return false;
    }
    if (polyhook::FindDetour(g_damage.takeDamageAddress)) {
        return false;
    }

    constexpr std::array argumentKinds{polyhook::DataType::Pointer,
                                       polyhook::DataType::Pointer,
                                       polyhook::DataType::Pointer};
    const plg::vector<polyhook::DataType> argumentTypes(argumentKinds.begin(), argumentKinds.end());
    auto hook = polyhook::HookDetour(g_damage.takeDamageAddress,
                                     polyhook::DataType::Int64,
                                     argumentTypes,
                                     -1,
                                     std::string(kDamageHookName));
    if (!hook) {
        return false;
    }
    g_damage.hook = hook;
    if (!RegisterDamageCallbacks(hook, playerCallback, entityCallback)) {
        if (DetachDamageHook()) {
            ClearDamageSubscriptions();
        }
        return false;
    }
    return true;
}

bool DamageHookIsActive() noexcept {
    if (!g_damage.hook || !g_damage.takeDamageAddress || !g_damage.postInstalled ||
        !__polyhook_FindDetour || !__polyhook_IsCallbackRegistered) {
        return false;
    }
    try {
        return polyhook::FindDetour(g_damage.takeDamageAddress) == g_damage.hook &&
               polyhook::IsCallbackRegistered(g_damage.hook, polyhook::CallbackType::Pre, DamageHook) &&
               polyhook::IsCallbackRegistered(g_damage.hook, polyhook::CallbackType::Post, DamageHook);
    } catch (...) {
        return false;
    }
}

} // namespace

bool EnsureDamageAvailable() {
    return EnsureDamageReady();
}

bool EnsureEntityDamageAvailable() {
    return EnsureDamageCommonReady();
}

#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
bool ProbeNativeDamagePaths(std::int32_t pawnHandle,
                            std::int32_t victimHandle,
                            std::int32_t weaponHandle,
                            std::int32_t nonWeaponHandle) noexcept {
    try {
        if (!EnsureEngine() || !ResolveNavPhysics() || pawnHandle == -1 || victimHandle == -1 || weaponHandle == -1 ||
            nonWeaponHandle == -1) {
            return false;
        }
        void* pawn = EntityFromHandle(pawnHandle);
        void* victim = EntityFromHandle(victimHandle);
        void* weapon = nullptr;
        void* nonWeapon = nullptr;
        return pawn && HandleFromEntity(pawn) == pawnHandle && IsPlayerPawn(pawnHandle) && victim &&
               HandleFromEntity(victim) == victimHandle && TraceVictimHead(pawnHandle, pawn, victim) &&
               ResolveWeaponHandle(weaponHandle, weapon) && weapon &&
               !ResolveWeaponHandle(nonWeaponHandle, nonWeapon) &&
               s2sdk::NativeFixtureNavTraceCallCount() == 1 &&
               s2sdk::NativeFixtureLastEyePositionHandle() == pawnHandle &&
               s2sdk::NativeFixtureLastEyeAngleHandle() == pawnHandle;
    } catch (...) {
        return false;
    }
}

bool ProbeDamageRegistrationRollback() noexcept {
    try {
        if (!__polyhook_HookDetour || !__polyhook_FindDetour || !__polyhook_AddCallback ||
            !__polyhook_IsCallbackRegistered || !__polyhook_RemoveCallback || !__polyhook_UnhookDetour ||
            DamageFrameStack::ActiveCalls() != 0) {
            return false;
        }

        auto* const target = reinterpret_cast<void*>(&DamageRegistrationProbeTarget);
        if (polyhook::FindDetour(target)) {
            return false;
        }
        constexpr std::array argumentKinds{polyhook::DataType::Pointer,
                                           polyhook::DataType::Pointer,
                                           polyhook::DataType::Pointer};
        const plg::vector<polyhook::DataType> argumentTypes(argumentKinds.begin(), argumentKinds.end());
        const auto hook = polyhook::HookDetour(target,
                                               polyhook::DataType::Int64,
                                               argumentTypes,
                                               -1,
                                               "FunRounds::DamageRegistrationProbe");
        if (!hook) {
            return false;
        }

        g_damage.takeDamageAddress = target;
        g_damage.hook = hook;
        g_damage.owner = nullptr;
        g_damage.callback = nullptr;
        g_damage.entityOwner = nullptr;
        g_damage.entityCallback = nullptr;
        g_damage.postInstalled = false;
        g_damage.commonReady = false;
        g_damage.traceReady = false;
        g_damage.ready = false;
        g_probeDamageTargetCalls = 0;
        g_probeDamageTargetChecksum = 0;
        g_probeFailDamagePostRegistration = true;
        const bool installed = RegisterDamageCallbacks(hook, DamageRegistrationProbeCallback, nullptr);
        const bool preRegistered = polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Pre, DamageHook);
        const bool ownerRetained = !installed && preRegistered && g_damage.owner == DamageRegistrationProbeCallback &&
                                   g_damage.callback == nullptr && !g_damage.entityOwner && !g_damage.entityCallback &&
                                   !g_damage.postInstalled;
        const auto throughPartialHook = reinterpret_cast<std::int64_t (*)(void*, void*, void*)>(target)(nullptr,
                                                                                                          nullptr,
                                                                                                          nullptr);
        const bool frameBalanced = DamageFrameStack::ActiveCalls() == 0;
        const bool detached = DetachDamageHook();
        if (detached) {
            ClearDamageSubscriptions();
        }
        const auto afterDetach = reinterpret_cast<std::int64_t (*)(void*, void*, void*)>(target)(nullptr,
                                                                                                 nullptr,
                                                                                                 nullptr);
        return ownerRetained && throughPartialHook == 42 && frameBalanced && detached &&
               afterDetach == 43 && DamageFrameStack::ActiveCalls() == 0 && polyhook::FindDetour(target) == nullptr;
    } catch (...) {
        return false;
    }
}

bool ProbeDamageDetachFailureRetry() noexcept {
    try {
        if (!__polyhook_HookDetour || !__polyhook_FindDetour || !__polyhook_AddCallback ||
            !__polyhook_IsCallbackRegistered || !__polyhook_RemoveCallback || !__polyhook_UnhookDetour ||
            g_damage.owner || g_damage.entityOwner || g_damage.entityCallback || DamageFrameStack::ActiveCalls() != 0) {
            return false;
        }

        auto* const target = reinterpret_cast<void*>(&DamageRegistrationProbeTarget);
        if (polyhook::FindDetour(target)) {
            return false;
        }
        constexpr std::array argumentKinds{polyhook::DataType::Pointer,
                                           polyhook::DataType::Pointer,
                                           polyhook::DataType::Pointer};
        const plg::vector<polyhook::DataType> argumentTypes(argumentKinds.begin(), argumentKinds.end());
        const auto hook = polyhook::HookDetour(target,
                                               polyhook::DataType::Int64,
                                               argumentTypes,
                                               -1,
                                               "FunRounds::DamageDetachProbe");
        if (!hook) {
            return false;
        }

        g_damage.takeDamageAddress = target;
        g_damage.hook = hook;
        g_damage.ready = false;
        g_probeDamageTargetCalls = 0;
        if (!RegisterDamageCallbacks(hook, DamageRegistrationProbeCallback, nullptr)) {
            if (DetachDamageHook()) {
                ClearDamageSubscriptions();
            }
            return false;
        }

        bool partialDetach = false;
        std::int64_t throughPostOnly = 0;
        bool stackBalanced = false;
        {
            ScopedDamageRemoveFailure failPostRemoval;
            g_probeFailDamagePostRemoval = true;
            const bool firstDetach = DetachDamageHook();
            partialDetach = !firstDetach && polyhook::FindDetour(target) == hook &&
                            !polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Pre, DamageHook) &&
                            polyhook::IsCallbackRegistered(hook, polyhook::CallbackType::Post, DamageHook) &&
                            !g_damage.postInstalled && g_damage.owner == DamageRegistrationProbeCallback;
            throughPostOnly = reinterpret_cast<std::int64_t (*)(void*, void*, void*)>(target)(nullptr,
                                                                                              nullptr,
                                                                                              nullptr);
            stackBalanced = DamageFrameStack::ActiveCalls() == 0;
        }

        const bool retryDetached = DetachDamageHook();
        if (retryDetached) {
            ClearDamageSubscriptions();
        }
        return partialDetach && throughPostOnly == 42 && stackBalanced && retryDetached && !g_damage.owner &&
               !g_damage.entityOwner && !g_damage.entityCallback &&
               DamageFrameStack::ActiveCalls() == 0 && polyhook::FindDetour(target) == nullptr;
    } catch (...) {
        return false;
    }
}
#endif

bool HookDamage(DamagePreCallback callback) {
    if (!callback) {
        return false;
    }
    if (g_damage.owner == callback && g_damage.callback == callback && DamageHookIsActive()) {
        return true;
    }
    if (g_damage.owner && g_damage.owner != callback) {
        return false;
    }
    if (g_damage.owner == callback) {
        if (g_damage.entityOwner) {
            return false;
        }
        if (!DetachDamageHook()) {
            return false;
        }
        ClearDamageSubscriptions();
    } else if (g_damage.hook) {
        if (DamageHookIsActive()) {
            if (!EnsureDamageReady()) {
                return false;
            }
            g_damage.owner = callback;
            g_damage.callback = callback;
            RefreshDamageFrameTracking();
            return true;
        }
        if (g_damage.entityOwner || !DetachDamageHook()) {
            return false;
        }
        ClearDamageSubscriptions();
    }
    return EnsureDamageReady() && InstallDamageHook(callback, nullptr);
}

bool UnhookDamage(DamagePreCallback callback) {
    if (!callback || callback != g_damage.owner) {
        return false;
    }
    g_damage.callback = nullptr;
    RefreshDamageFrameTracking();
    if (g_damage.entityCallback) {
        g_damage.owner = nullptr;
        return true;
    }
    if (!DetachDamageHook()) {
        return false;
    }
    ClearDamageSubscriptions();
    return true;
}

bool HookEntityDamage(EntityDamagePreCallback callback) {
    if (!callback) {
        return false;
    }
    if (g_damage.entityOwner == callback && g_damage.entityCallback == callback && DamageHookIsActive()) {
        return true;
    }
    if (g_damage.entityOwner && g_damage.entityOwner != callback) {
        return false;
    }
    if (g_damage.entityOwner == callback) {
        if (g_damage.owner) {
            return false;
        }
        if (!DetachDamageHook()) {
            return false;
        }
        ClearDamageSubscriptions();
    } else if (g_damage.hook) {
        if (DamageHookIsActive()) {
            if (!EnsureDamageCommonReady()) {
                return false;
            }
            g_damage.entityOwner = callback;
            g_damage.entityCallback = callback;
            RefreshDamageFrameTracking();
            return true;
        }
        if (g_damage.owner || !DetachDamageHook()) {
            return false;
        }
        ClearDamageSubscriptions();
    }
    return EnsureDamageCommonReady() && InstallDamageHook(nullptr, callback);
}

bool UnhookEntityDamage(EntityDamagePreCallback callback) {
    if (!callback) {
        return false;
    }
    if (callback != g_damage.entityOwner) {
        return g_damage.entityCallback != callback;
    }
    g_damage.entityCallback = nullptr;
    RefreshDamageFrameTracking();
    if (g_damage.callback) {
        g_damage.entityOwner = nullptr;
        return true;
    }
    if (!DetachDamageHook()) {
        return false;
    }
    ClearDamageSubscriptions();
    return true;
}

bool ShutdownDamage() noexcept {
    g_damage.callback = nullptr;
    g_damage.entityCallback = nullptr;
    RefreshDamageFrameTracking();
    try {
        const bool detached = DetachDamageHook();
        if (detached) {
            ClearDamageSubscriptions();
        }
        return detached;
    } catch (...) {
        return false;
    }
}

std::uint64_t CurrentDamageFrameToken() noexcept {
    return DamageFrameStack::Current();
}

std::int32_t ApplyDamage(std::int32_t victimHandle,
                         std::int32_t inflictorHandle,
                         std::int32_t attackerHandle,
                         const float forceValues[3],
                         const float positionValues[3],
                         float damage,
                         std::int32_t damageType,
                         std::int32_t weaponHandle,
                         bool skipPluginRules) {
    try {
        if (!forceValues || !positionValues || !EnsureDamageReady() || !g_damage.damageConstructorAddress ||
            !g_damage.takeDamageAddress || !g_damage.infoClass || victimHandle == -1 || inflictorHandle == -1 ||
            attackerHandle == -1) {
            return -1;
        }

        void* victim = EntityFromHandle(victimHandle);
        void* inflictor = EntityFromHandle(inflictorHandle);
        void* attacker = EntityFromHandle(attackerHandle);
        void* weapon = nullptr;
        if (!ResolveWeaponHandle(weaponHandle, weapon)) {
            return -1;
        }
        if (!victim || !inflictor || !attacker || HandleFromEntity(victim) != victimHandle ||
            HandleFromEntity(inflictor) != inflictorHandle || HandleFromEntity(attacker) != attackerHandle) {
            return -1;
        }

        const Vector force{forceValues[0], forceValues[1], forceValues[2]};
        const Vector position{positionValues[0], positionValues[1], positionValues[2]};
        NativeDamageInfoStorage info(g_damage.infoClass);
        auto constructor = reinterpret_cast<NativeDamageConstructor>(g_damage.damageConstructorAddress);
        if (!info.Construct(constructor, inflictor, attacker, weapon, force, position, damage, damageType)) {
            return -1;
        }

        auto* damageInfo = static_cast<CTakeDamageInfo*>(info.Get());
        WriteField(damageInfo, g_damage.info.abilityHandle.offset,
                   weapon ? ToNativeHandle(weaponHandle) : kInvalidEntityHandle);
        CTakeDamageResult damageResult(damage);
        damageResult.CopyFrom(damageInfo, damage);
        DamageCallScope<DamagePreCallback> playerScope(damageInfo, g_damage.callback, skipPluginRules);
        DamageCallScope<EntityDamagePreCallback> entityScope(damageInfo, g_damage.entityCallback, skipPluginRules);
        (void)reinterpret_cast<TakeDamageFunction>(g_damage.takeDamageAddress)(
            static_cast<CBaseEntity*>(victim), damageInfo, &damageResult);
        const auto applied = ReadField<std::int32_t>(&damageResult, g_damage.result.healthLost.offset);
        (void)info.Destruct();
        return applied < 0 ? -1 : applied;
    } catch (...) {
        return -1;
    }
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API bool HasPlayerTakeDamagePre() {
    try {
        return funrounds::native::detail::EnsureDamageAvailable();
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool HasEntityTakeDamagePre() {
    try {
        return funrounds::native::detail::EnsureEntityDamageAvailable();
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool HookPlayerTakeDamagePre(funrounds::native::detail::DamagePreCallback callback) {
    try {
        return funrounds::native::detail::HookDamage(callback);
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool UnhookPlayerTakeDamagePre(funrounds::native::detail::DamagePreCallback callback) {
    try {
        return funrounds::native::detail::UnhookDamage(callback);
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool HookEntityTakeDamagePre(
    funrounds::native::detail::EntityDamagePreCallback callback) {
    try {
        return funrounds::native::detail::HookEntityDamage(callback);
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API bool UnhookEntityTakeDamagePre(
    funrounds::native::detail::EntityDamagePreCallback callback) {
    try {
        return funrounds::native::detail::UnhookEntityDamage(callback);
    } catch (...) {
        return false;
    }
}

extern "C" PLUGIN_API std::uint64_t GetCurrentDamageFrameToken() {
    return funrounds::native::detail::CurrentDamageFrameToken();
}

extern "C" PLUGIN_API std::int32_t TakeEntityDamageWithWeapon(std::int32_t victimHandle,
                                                                 std::int32_t inflictorHandle,
                                                                 std::int32_t attackerHandle,
                                                                 const plg::vec3& force,
                                                                 const plg::vec3& position,
                                                                 float damage,
                                                                 std::int32_t damageType,
                                                                 std::int32_t weaponHandle,
                                                                 bool skipPluginRules) {
    const float forceValues[]{force.x, force.y, force.z};
    const float positionValues[]{position.x, position.y, position.z};
    return funrounds::native::detail::ApplyDamage(victimHandle,
                                                   inflictorHandle,
                                                   attackerHandle,
                                                   forceValues,
                                                   positionValues,
                                                   damage,
                                                   damageType,
                                                   weaponHandle,
                                                   skipPluginRules);
}
