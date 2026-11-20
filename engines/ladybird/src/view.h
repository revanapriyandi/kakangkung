#pragma once

#include "kakangkung_ladybird.h"

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace kakangkung::ladybird {

class Session;

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

private:
    Session& m_session;

#if defined(HAVE_LADYBIRD)
    struct LibWeb;
    std::unique_ptr<LibWeb> m_libweb;
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
#    if defined(_WIN32)
    void* m_hwnd { nullptr };
#    endif
#endif
};

} // namespace kakangkung::ladybird
