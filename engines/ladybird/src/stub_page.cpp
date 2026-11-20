#include "view.h"

#include "application.h"

#include <utility>

namespace kakangkung::ladybird {

namespace {

std::string stub_html(std::string const& url)
{
    // Minimal explanatory document so a stub host still paints something recognizable.
    std::string body = "<!DOCTYPE html><html><head><meta charset=utf-8><title>Kakangkung · Ladybird</title>"
                       "<style>body{font:16px/1.45 system-ui,sans-serif;margin:2rem;background:#0f1419;color:#e7ecf1}"
                       "code{background:#1c2430;padding:.1rem .35rem;border-radius:4px}"
                       "a{color:#7cb7ff}</style></head><body>"
                       "<h1>Ladybird UI port (stub)</h1>"
                       "<p>Kakangkung asked this host to open <code>";
    body += url;
    body += "</code>.</p>"
            "<p>This binary was built <strong>without</strong> Ladybird LibWeb linked in. "
            "Rebuild with <code>HAVE_LADYBIRD=1</code> and a Ladybird source tree "
            "(see <code>engines/ladybird/README.md</code>) to render with LibWeb.</p>"
            "<p><a href=\"https://ladybird.org\">ladybird.org</a></p>"
            "</body></html>";
    return body;
}

} // namespace

View::View(Session& session, std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks)
    : m_session(session)
    , m_parent(parent_hwnd)
    , m_cb(callbacks)
{
    (void)m_session;
    (void)m_parent;
}

View::~View() = default;

void View::emit_load(bool done)
{
    if (m_cb.on_load)
        m_cb.on_load(m_cb.user, done ? 1 : 0);
}

void View::emit_title(std::string const& title)
{
    if (m_cb.on_title)
        m_cb.on_title(m_cb.user, title.c_str());
}

void View::emit_url(std::string const& url)
{
    if (m_cb.on_url)
        m_cb.on_url(m_cb.user, url.c_str());
}

void View::navigate(std::string url)
{
    if (url.empty())
        url = "about:blank";
    if (m_url != url) {
        m_back.push_back(m_url);
        m_forward.clear();
    }
    m_url = std::move(url);
    emit_load(false);
    emit_url(m_url);
    emit_title(m_url == "about:blank" ? "New Tab" : m_url);
    // Stub does not paint HTML into an HWND yet; the host process prints the document for debugging.
    auto html = stub_html(m_url);
    (void)html;
    emit_load(true);
}

void View::stop() { emit_load(true); }

void View::reload() { navigate(m_url); }

void View::set_bounds(int x, int y, int w, int h)
{
    m_x = x;
    m_y = y;
    m_w = w;
    m_h = h;
}

void View::set_visible(bool visible) { m_visible = visible; }

bool View::can_go_back() const { return !m_back.empty(); }

bool View::can_go_forward() const { return !m_forward.empty(); }

void View::go_back()
{
    if (m_back.empty())
        return;
    m_forward.push_back(m_url);
    m_url = m_back.back();
    m_back.pop_back();
    emit_url(m_url);
    emit_title(m_url);
    emit_load(true);
}

void View::go_forward()
{
    if (m_forward.empty())
        return;
    m_back.push_back(m_url);
    m_url = m_forward.back();
    m_forward.pop_back();
    emit_url(m_url);
    emit_title(m_url);
    emit_load(true);
}

} // namespace kakangkung::ladybird
