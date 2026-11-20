#pragma once

#include "kakangkung_ladybird.h"

#include <memory>
#include <string>
#include <vector>

namespace kakangkung::ladybird {

class View;

/// Session state shared by every view. With HAVE_LADYBIRD this wraps WebView::Application;
/// otherwise it only tracks stub views for the host process.
class Session {
public:
    explicit Session(std::string profile_dir);
    ~Session();

    View* create_view(std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks);
    void destroy_view(View* view);
    int pump();

    std::string const& profile_dir() const { return m_profile_dir; }

private:
    std::string m_profile_dir;
    std::vector<std::unique_ptr<View>> m_views;
};

} // namespace kakangkung::ladybird
