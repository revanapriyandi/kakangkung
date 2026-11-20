#pragma once

#include "kakangkung_ladybird.h"

#include <cstdint>
#include <string>
#include <vector>

namespace kakangkung::ladybird {

class Session;

#if defined(HAVE_LADYBIRD)
// Real UI port: subclass WebView::ViewImplementation (see Ladybird Documentation/Porting.md).
// The implementation lives in view_ladybird.cpp when HAVE_LADYBIRD is set.
class View;
#else
/// Stub view used when Ladybird sources are not linked. Speaks the same C ABI.
/// On Windows, paints an explanatory child HWND into Kakangkung's chrome parent.
class View {
public:
    View(Session& session, std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks);
    ~View();

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
    void emit_load(bool done);
    void emit_title(std::string const& title);
    void emit_url(std::string const& url);
    void paint_label();

    Session& m_session;
    std::uintptr_t m_parent {};
    kk_lb_callbacks m_cb {};
    std::string m_url { "about:blank" };
    std::vector<std::string> m_back;
    std::vector<std::string> m_forward;
    int m_x {}, m_y {}, m_w { 800 }, m_h { 600 };
    bool m_visible { true };
#    if defined(_WIN32)
    void* m_hwnd { nullptr }; // HWND
#    endif
};
#endif

} // namespace kakangkung::ladybird
