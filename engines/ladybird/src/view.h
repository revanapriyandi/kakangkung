#pragma once

#include "kakangkung_ladybird.h"

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace kakangkung::ladybird {

class Session;

/// Create LibWeb's Application (event loop, RequestServer, ImageDecoder, content blocker…).
/// Called when the session starts, before the host reports `ready`: the first commands
/// (adblock, permissions) need it, and before it there is no event loop to pump.
/// Only defined with HAVE_LADYBIRD.
bool initialize_libweb();

/// Page view. With HAVE_LADYBIRD this wraps LibWeb (`HeadlessWebView` + native surface paint).
/// Without it, stub_page.cpp paints an explanatory surface (build-only fallback, not product).
class View {
public:
    View(Session& session, std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks);
    ~View();

    View(View const&) = delete;
    View& operator=(View const&) = delete;

    Session& session() { return m_session; }

    void navigate(std::string url);
    void stop();
    void reload();
    void set_bounds(int x, int y, int w, int h);
    void set_visible(bool visible);
    bool can_go_back() const;
    bool can_go_forward() const;
    void go_back();
    void go_forward();

    /// Real LibWeb ops (return false when host is not linked / unavailable).
    bool screenshot_png(std::string const& path);
    bool print_page();
    /// Page zoom. `relative` null → reset to 1.0; else multiply and clamp 0.25…5.0.
    /// When `absolute` is set, that value wins (also clamped). Writes resulting factor.
    bool set_zoom(double const* relative, double const* absolute, double& out_factor);
    bool a11y_json(std::string& out_json);
    bool dom_json(std::string& out_json);
    bool console_json(std::string& out_json, bool clear);
    bool eval_js(std::string const& js, std::string& out_result);
    bool capabilities_json(std::string& out_json);
    bool ext_reply(std::string const& extension_id, std::string const& request_id, std::string const& response_json);
    /// Drain window.__kk_ext_queue into on_ext_message (console IPC is unreliable on HeadlessWebView).
    void drain_ext_queue();
    /// Find-in-page (LibWeb ViewImplementation::find_in_page). end ends search.
    /// forward=false → previous match when a search is already active with the same query.
    bool find_in_page(std::string const& query, bool match_case, bool forward, bool end, int& active_match,
        int& match_count);

private:
    Session& m_session;

#if defined(HAVE_LADYBIRD)
    struct LibWeb;
    std::unique_ptr<LibWeb> m_libweb;
    double m_zoom { 1.0 };
    std::string m_find_query;
#else
    void emit_load(bool done);
    void emit_title(std::string const& title);
    void emit_url(std::string const& url);
    void paint_label();

    std::uintptr_t m_parent {};
    kk_lb_callbacks m_cb {};
    std::string m_url { "about:blank" };
    std::vector<std::string> m_back;
    std::vector<std::string> m_forward;
    int m_x {}, m_y {}, m_w { 800 }, m_h { 600 };
    bool m_visible { true };
    double m_zoom { 1.0 };
#    if defined(_WIN32)
    void* m_hwnd { nullptr };
#    endif
#endif
};

} // namespace kakangkung::ladybird
