// KB-11: clear cookies / site storage / HTTP cache via LibWeb Application::clear_browsing_data.
// Fail closed when LibWeb is not linked (see abi.cpp stubs).

#if !defined(HAVE_LADYBIRD)
#    error "clear_data_ladybird.cpp requires -DHAVE_LADYBIRD=1"
#endif

#include "kakangkung_ladybird.h"

#include <AK/Time.h>
#include <LibCore/EventLoop.h>
#include <LibCore/Promise.h>
#include <LibWebView/Application.h>

#include <chrono>

extern "C" {

int kk_lb_clear_data(int cookies, int cache, int64_t since_ms)
{
    if (!cookies && !cache)
        return 1;

    WebView::Application::ClearBrowsingDataOptions options;
    options.since = since_ms > 0 ? UnixDateTime::from_milliseconds_since_epoch(since_ms) : UnixDateTime::earliest();
    options.delete_site_data = cookies ? WebView::Application::ClearBrowsingDataOptions::Delete::Yes
                                       : WebView::Application::ClearBrowsingDataOptions::Delete::No;
    options.delete_cached_files = cache ? WebView::Application::ClearBrowsingDataOptions::Delete::Yes
                                        : WebView::Application::ClearBrowsingDataOptions::Delete::No;
    // History / downloads stay in Kakangkung SQLite — not LibWeb's crash-report history.

    auto promise = WebView::Application::the().clear_browsing_data(options);
    bool done = false;
    bool ok = false;
    promise->when_resolved([&](Empty) {
        ok = true;
        done = true;
    });
    promise->when_rejected([&](Error&) {
        ok = false;
        done = true;
    });

    auto const deadline = std::chrono::steady_clock::now() + std::chrono::seconds(15);
    while (!done) {
        Core::EventLoop::current().pump(Core::EventLoop::WaitMode::PollForEvents);
        if (std::chrono::steady_clock::now() >= deadline)
            return 0;
    }
    return ok ? 1 : 0;
}

} // extern "C"
