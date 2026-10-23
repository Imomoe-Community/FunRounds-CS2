#include <plg/plugin.hpp>

#include <plugin_export.h>

#include "damage.hpp"
#include "engine.hpp"
#include "suicide.hpp"
#include "touch.hpp"

namespace {

class FunRoundsNativePlugin final : public plg::Plugin {
public:
    plg::PluginResult OnPluginStart() override {
        return {};
    }

    plg::PluginResult OnPluginEnd() override {
        try {
            const bool damageClean = funrounds::native::detail::ShutdownDamage();
            const bool suicideClean = funrounds::native::detail::ShutdownSuicide();
            const bool touchClean = funrounds::native::detail::ShutdownTouch();
            if (!damageClean || !suicideClean || !touchClean) {
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
