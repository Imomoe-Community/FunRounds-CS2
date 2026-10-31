// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include <plg/plugin.hpp>

#include <plugin_export.h>

#include "damage.hpp"
#include "engine.hpp"
#include "ghost_radar.hpp"
#include "ghost_visibility.hpp"
#include "ground_reverse.hpp"
#include "interactions.hpp"
#include "suicide.hpp"
#include "touch.hpp"
#include "third_person.hpp"

#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
#include "lifecycle_probe.hpp"
#endif

namespace {

class FunRoundsNativePlugin final : public plg::Plugin {
public:
    plg::PluginResult OnPluginStart() override {
#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
        try {
            const auto fail = [](const char* message) {
                (void)funrounds::native::detail::ShutdownDamage();
                (void)funrounds::native::detail::ShutdownTouch();
                (void)funrounds::native::detail::StopLifecycleProbe();
                return plg::PluginResult{message};
            };
            if (!funrounds::native::detail::StartLifecycleProbe()) {
                return fail(funrounds::native::detail::LifecycleProbeFailure());
            }
            if (!funrounds::native::detail::ProbeNativeDamagePaths(17, 21, 18, 19)) {
                return fail("Native damage path probe failed");
            }
            if (!funrounds::native::detail::ProbeDamageRegistrationRollback()) {
                return fail("Native damage registration rollback probe failed");
            }
            if (!funrounds::native::detail::ProbeDamageDetachFailureRetry()) {
                return fail("Native damage detach retry probe failed");
            }
            if (!funrounds::native::detail::ProbeTouchUnhookContract(17)) {
                return fail("Native Touch unhook contract probe failed");
            }
            return {};
        } catch (...) {
            (void)funrounds::native::detail::ShutdownDamage();
            (void)funrounds::native::detail::ShutdownTouch();
            (void)funrounds::native::detail::StopLifecycleProbe();
            return plg::PluginResult{"Native lifecycle probe threw"};
        }
#else
        return {};
#endif
    }

    plg::PluginResult OnPluginEnd() override {
        try {
            const bool damageClean = funrounds::native::detail::ShutdownDamage();
            const bool suicideClean = funrounds::native::detail::ShutdownSuicide();
            const bool touchClean = funrounds::native::detail::ShutdownTouch();
            const bool groundReverseClean = funrounds::native::detail::ShutdownGroundReverse();
            const bool ghostRadarClean = funrounds::native::detail::ShutdownGhostRadar();
            const bool ghostVisibilityClean = funrounds::native::detail::ShutdownGhostVisibility();
            const bool interactionsClean = funrounds::native::detail::ShutdownInteractions();
            const bool thirdPersonClean = funrounds::native::detail::ShutdownThirdPersonEye();
#if defined(FUNROUNDS_NATIVE_LIFECYCLE_PROBE)
            const bool probeClean = funrounds::native::detail::StopLifecycleProbe();
#else
            constexpr bool probeClean = true;
#endif
            if (!damageClean || !suicideClean || !touchClean || !groundReverseClean || !ghostRadarClean ||
                !ghostVisibilityClean ||
                !interactionsClean || !thirdPersonClean || !probeClean) {
                return plg::PluginResult{"Native hook cleanup could not be verified"};
            }
            funrounds::native::detail::ShutdownEngine();
            return {};
        } catch (...) {
            return plg::PluginResult{"Native hook cleanup threw"};
        }
    }
};

FunRoundsNativePlugin g_plugin;

} // namespace

PLUGIFY_PLUGIN(PLUGIN_API, &g_plugin)
