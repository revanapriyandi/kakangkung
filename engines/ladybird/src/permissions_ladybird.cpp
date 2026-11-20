// KB-19: LibWeb PermissionManager defaults + list/reset (no WebView2 stubs).

#if !defined(HAVE_LADYBIRD)
#    error "permissions_ladybird.cpp requires -DHAVE_LADYBIRD=1"
#endif

#include "kakangkung_ladybird.h"

#include <AK/ByteString.h>
#include <AK/String.h>
#include <AK/StringView.h>
#include <LibURL/Parser.h>
#include <LibWebView/Application.h>
#include <LibWebView/PermissionManager.h>

#include <cstdlib>
#include <cstring>
#include <string>

namespace {

using Web::Permissions::EffectiveState;
using Web::Permissions::PermissionName;
using WebView::PermissionDecision;
using WebView::PermissionManager;

PermissionManager* manager()
{
    auto& session = WebView::Application::the().default_session();
    return session.permission_manager.ptr();
}

Optional<PermissionName> name_from_key(char const* key)
{
    if (!key)
        return {};
    StringView k { key, std::strlen(key) };
    if (k == "geolocation"sv || k == "location"sv)
        return PermissionName::Geolocation;
    if (k == "microphone"sv)
        return PermissionName::Microphone;
    if (k == "notifications"sv)
        return PermissionName::Notifications;
    return {};
}

char const* key_for(PermissionName name)
{
    switch (name) {
    case PermissionName::Geolocation:
        return "geolocation";
    case PermissionName::Microphone:
        return "microphone";
    case PermissionName::Notifications:
        return "notifications";
    }
    return "unknown";
}

Optional<URL::Origin> origin_from_utf8(char const* origin_utf8)
{
    if (!origin_utf8 || !*origin_utf8)
        return {};
    auto url = URL::Parser::basic_parse(StringView { origin_utf8, std::strlen(origin_utf8) });
    if (!url.has_value())
        return {};
    auto origin = url->origin();
    if (origin.is_opaque())
        return {};
    return origin;
}

} // namespace

extern "C" {

int kk_lb_permissions_set_defaults(int block_geolocation, int block_microphone, int block_notifications)
{
    auto* pm = manager();
    if (!pm)
        return 0;
    auto to_state = [](int block) {
        return block ? EffectiveState::Denied : EffectiveState::Prompt;
    };
    pm->set_global_default(PermissionName::Geolocation, to_state(block_geolocation));
    pm->set_global_default(PermissionName::Microphone, to_state(block_microphone));
    pm->set_global_default(PermissionName::Notifications, to_state(block_notifications));
    return 1;
}

int kk_lb_permissions_list_json(char** out_utf8)
{
    if (!out_utf8)
        return 0;
    *out_utf8 = nullptr;
    auto* pm = manager();
    if (!pm)
        return 0;

    std::string json = "[";
    bool first = true;
    for (auto name : { PermissionName::Geolocation, PermissionName::Microphone, PermissionName::Notifications }) {
        for (auto const& site : pm->site_decisions(name)) {
            if (!first)
                json += ',';
            first = false;
            auto origin = site.origin.serialize().to_byte_string();
            bool allowed = site.decision == PermissionDecision::Allow
                || site.decision == PermissionDecision::AllowOnce;
            json += "{\"kind\":\"";
            json += key_for(name);
            json += "\",\"origin\":\"";
            for (char c : std::string { origin.characters() }) {
                if (c == '\\' || c == '"')
                    json += '\\';
                json += c;
            }
            json += "\",\"allowed\":";
            json += allowed ? "true" : "false";
            json += '}';
        }
    }
    json += ']';

    *out_utf8 = static_cast<char*>(std::malloc(json.size() + 1));
    if (!*out_utf8)
        return 0;
    std::memcpy(*out_utf8, json.c_str(), json.size() + 1);
    return 1;
}

int kk_lb_permissions_reset(char const* origin_utf8, char const* kind_utf8)
{
    auto* pm = manager();
    if (!pm)
        return 0;
    auto origin = origin_from_utf8(origin_utf8);
    if (!origin.has_value())
        return 0;
    if (kind_utf8 && *kind_utf8) {
        auto name = name_from_key(kind_utf8);
        if (!name.has_value())
            return 0;
        pm->set_decision(*name, *origin, {});
        return 1;
    }
    pm->reset_origin(*origin);
    return 1;
}

} // extern "C"
