#include "kakangkung_ladybird.h"

#include "application.h"
#include "view.h"

#include <cstdlib>
#include <cstring>
#include <string>

using kakangkung::ladybird::Session;
using kakangkung::ladybird::View;

extern "C" {

char const* kk_lb_engine_id(void)
{
#if defined(HAVE_LADYBIRD)
    return "ladybird";
#else
    return "stub";
#endif
}

kk_lb_session* kk_lb_session_create(char const* profile_dir_utf8)
{
    return reinterpret_cast<kk_lb_session*>(new Session(profile_dir_utf8 ? profile_dir_utf8 : ""));
}

void kk_lb_session_destroy(kk_lb_session* session)
{
    delete reinterpret_cast<Session*>(session);
}

kk_lb_view* kk_lb_view_create(kk_lb_session* session, uintptr_t parent_hwnd, kk_lb_callbacks callbacks)
{
    if (!session)
        return nullptr;
    return reinterpret_cast<kk_lb_view*>(reinterpret_cast<Session*>(session)->create_view(parent_hwnd, callbacks));
}

void kk_lb_view_destroy(kk_lb_view* view)
{
    if (!view)
        return;
    auto* v = reinterpret_cast<View*>(view);
    v->session().destroy_view(v);
}

void kk_lb_view_navigate(kk_lb_view* view, char const* url_utf8)
{
    if (!view)
        return;
    reinterpret_cast<View*>(view)->navigate(url_utf8 ? url_utf8 : "about:blank");
}

void kk_lb_view_stop(kk_lb_view* view)
{
    if (view)
        reinterpret_cast<View*>(view)->stop();
}

void kk_lb_view_reload(kk_lb_view* view)
{
    if (view)
        reinterpret_cast<View*>(view)->reload();
}

void kk_lb_view_set_bounds(kk_lb_view* view, int x, int y, int w, int h)
{
    if (view)
        reinterpret_cast<View*>(view)->set_bounds(x, y, w, h);
}

void kk_lb_view_set_visible(kk_lb_view* view, int visible)
{
    if (view)
        reinterpret_cast<View*>(view)->set_visible(visible != 0);
}

int kk_lb_view_can_go_back(kk_lb_view* view)
{
    return view && reinterpret_cast<View*>(view)->can_go_back() ? 1 : 0;
}

int kk_lb_view_can_go_forward(kk_lb_view* view)
{
    return view && reinterpret_cast<View*>(view)->can_go_forward() ? 1 : 0;
}

void kk_lb_view_go_back(kk_lb_view* view)
{
    if (view)
        reinterpret_cast<View*>(view)->go_back();
}

void kk_lb_view_go_forward(kk_lb_view* view)
{
    if (view)
        reinterpret_cast<View*>(view)->go_forward();
}

int kk_lb_view_screenshot(kk_lb_view* view, char const* path_utf8)
{
    if (!view || !path_utf8)
        return 0;
    return reinterpret_cast<View*>(view)->screenshot_png(path_utf8) ? 1 : 0;
}

int kk_lb_view_print(kk_lb_view* view)
{
    return view && reinterpret_cast<View*>(view)->print_page() ? 1 : 0;
}

int kk_lb_view_zoom(kk_lb_view* view, double const* relative_factor, double const* absolute_factor, double* out_factor)
{
    if (!view || !out_factor)
        return 0;
    return reinterpret_cast<View*>(view)->set_zoom(relative_factor, absolute_factor, *out_factor) ? 1 : 0;
}

int kk_lb_view_a11y_json(kk_lb_view* view, char** out_utf8)
{
    if (!view || !out_utf8)
        return 0;
    std::string json;
    if (!reinterpret_cast<View*>(view)->a11y_json(json))
        return 0;
    *out_utf8 = static_cast<char*>(std::malloc(json.size() + 1));
    if (!*out_utf8)
        return 0;
    std::memcpy(*out_utf8, json.c_str(), json.size() + 1);
    return 1;
}

int kk_lb_view_dom_json(kk_lb_view* view, char** out_utf8)
{
    if (!view || !out_utf8)
        return 0;
    std::string json;
    if (!reinterpret_cast<View*>(view)->dom_json(json))
        return 0;
    *out_utf8 = static_cast<char*>(std::malloc(json.size() + 1));
    if (!*out_utf8)
        return 0;
    std::memcpy(*out_utf8, json.c_str(), json.size() + 1);
    return 1;
}

int kk_lb_view_console_json(kk_lb_view* view, char** out_utf8, int clear_after)
{
    if (!view || !out_utf8)
        return 0;
    std::string json;
    if (!reinterpret_cast<View*>(view)->console_json(json, clear_after != 0))
        return 0;
    *out_utf8 = static_cast<char*>(std::malloc(json.size() + 1));
    if (!*out_utf8)
        return 0;
    std::memcpy(*out_utf8, json.c_str(), json.size() + 1);
    return 1;
}

int kk_lb_view_eval(kk_lb_view* view, char const* js_utf8, char** out_utf8)
{
    if (!view || !js_utf8)
        return 0;
    std::string result;
    if (!reinterpret_cast<View*>(view)->eval_js(js_utf8, result))
        return 0;
    if (out_utf8) {
        *out_utf8 = static_cast<char*>(std::malloc(result.size() + 1));
        if (!*out_utf8)
            return 0;
        std::memcpy(*out_utf8, result.c_str(), result.size() + 1);
    }
    return 1;
}

int kk_lb_view_capabilities(kk_lb_view* view, char** out_utf8)
{
    if (!view || !out_utf8)
        return 0;
    std::string json;
    if (!reinterpret_cast<View*>(view)->capabilities_json(json))
        return 0;
    *out_utf8 = static_cast<char*>(std::malloc(json.size() + 1));
    if (!*out_utf8)
        return 0;
    std::memcpy(*out_utf8, json.c_str(), json.size() + 1);
    return 1;
}

int kk_lb_view_ext_reply(kk_lb_view* view, char const* extension_id_utf8, char const* request_id_utf8,
    char const* response_json)
{
    if (!view || !extension_id_utf8 || !request_id_utf8)
        return 0;
    return reinterpret_cast<View*>(view)->ext_reply(extension_id_utf8, request_id_utf8,
               response_json ? response_json : "")
            ? 1
            : 0;
}

void kk_lb_view_drain_ext(kk_lb_view* view)
{
    if (view)
        reinterpret_cast<View*>(view)->drain_ext_queue();
}

#if !defined(HAVE_LADYBIRD)
void kk_lb_set_download_emit(void (*)(char const*)) { }
void kk_lb_downloads_ensure_observer(void) { }
int kk_lb_download_start(char const*, char const*) { return 0; }
int kk_lb_download_action(uint64_t, char const*) { return 0; }
int kk_lb_adblock_configure(int, char const*) { return 0; }
int kk_lb_adblock_enabled(void) { return 0; }
int kk_lb_clear_data(int, int, int64_t) { return 0; }
int kk_lb_permissions_set_defaults(int, int, int) { return 0; }
int kk_lb_permissions_list_json(char** out)
{
    if (out)
        *out = nullptr;
    return 0;
}
int kk_lb_permissions_reset(char const*, char const*) { return 0; }
#endif

int kk_lb_view_find(kk_lb_view* view, char const* query_utf8, int match_case, int forward, int end,
    int* active_match, int* match_count)
{
    if (!view)
        return 0;
    int active = 0;
    int count = 0;
    bool ok = reinterpret_cast<View*>(view)->find_in_page(query_utf8 ? query_utf8 : "", match_case != 0,
        forward != 0, end != 0, active, count);
    if (active_match)
        *active_match = active;
    if (match_count)
        *match_count = count;
    return ok ? 1 : 0;
}

void kk_lb_string_free(char* s)
{
    std::free(s);
}

int kk_lb_session_pump(kk_lb_session* session)
{
    if (!session)
        return 1;
    return reinterpret_cast<Session*>(session)->pump();
}

} // extern "C"
