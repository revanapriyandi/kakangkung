#include "application.h"

#include "view.h"

#include <algorithm>

#if defined(HAVE_LADYBIRD)
#    include <LibCore/EventLoop.h>
#endif

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
#if defined(HAVE_LADYBIRD)
    // Drain Ladybird's Core::EventLoop so WebContent IPC and paints advance.
    Core::EventLoop::current().pump(Core::EventLoop::WaitMode::PollForEvents);
#endif
    return 0;
}

} // namespace kakangkung::ladybird
