#include "application.h"

#include "view.h"

#include <algorithm>

#if defined(HAVE_LADYBIRD)
#    include <LibCore/EventLoop.h>
#    include <LibWebView/Application.h>
#endif

namespace kakangkung::ladybird {

Session::Session(std::string profile_dir)
    : m_profile_dir(std::move(profile_dir))
{
#if defined(HAVE_LADYBIRD)
    // Before `ready`: the host used to crash on the first command (adblock) because LibWeb
    // only came up with the first tab.
    initialize_libweb();
#endif
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
    // Drain Ladybird's Core::EventLoop so WebContent IPC and paints advance. No Application
    // (start-up failed) means no event loop: nothing to pump.
    if (WebView::Application::is_initialized())
        Core::EventLoop::current().pump(Core::EventLoop::WaitMode::PollForEvents);
#endif
    return 0;
}

} // namespace kakangkung::ladybird
