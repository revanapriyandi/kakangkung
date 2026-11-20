#include "application.h"

#include "view.h"

#include <algorithm>

namespace kakangkung::ladybird {

Session::Session(std::string profile_dir)
    : m_profile_dir(std::move(profile_dir))
{
}

Session::~Session() = default;

View* Session::create_view(std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks)
{
    auto view = std::make_unique<View>(*this, parent_hwnd, callbacks);
    View* raw = view.get();
    m_views.push_back(std::move(view));
    return raw;
}

void Session::destroy_view(View* view)
{
    m_views.erase(std::remove_if(m_views.begin(), m_views.end(), [view](auto const& p) { return p.get() == view; }), m_views.end());
}

int Session::pump()
{
    // Stub: nothing to pump. With HAVE_LADYBIRD this runs Core::EventLoop::pump().
    return 0;
}

} // namespace kakangkung::ladybird
