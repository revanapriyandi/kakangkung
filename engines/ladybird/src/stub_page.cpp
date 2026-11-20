#include "view.h"

#include "application.h"

#include <utility>

#if defined(_WIN32)
#    ifndef NOMINMAX
#        define NOMINMAX
#    endif
#    include <windows.h>
#endif

namespace kakangkung::ladybird {

namespace {

std::string stub_label(std::string const& url)
{
    return "Kakangkung · Ladybird stub\n\n" + url
        + "\n\nLibWeb not linked. Build with HAVE_LADYBIRD=1 on Boat\n(https://boat.dev/dashboard).";
}

#if defined(_WIN32)

wchar_t const* k_class = L"KakangkungLadybirdStub";

LRESULT CALLBACK stub_wndproc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam)
{
    if (msg == WM_PAINT) {
        PAINTSTRUCT ps;
        HDC hdc = BeginPaint(hwnd, &ps);
        RECT rc;
        GetClientRect(hwnd, &rc);
        FillRect(hdc, &rc, (HBRUSH)GetStockObject(DKGRAY_BRUSH));
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, RGB(230, 236, 241));
        auto* text = reinterpret_cast<std::wstring*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
        if (text) {
            DrawTextW(hdc, text->c_str(), -1, &rc, DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX);
        }
        EndPaint(hwnd, &ps);
        return 0;
    }
    if (msg == WM_NCDESTROY) {
        auto* text = reinterpret_cast<std::wstring*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
        delete text;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wparam, lparam);
}

void ensure_class()
{
    static bool once = false;
    if (once)
        return;
    once = true;
    WNDCLASSW wc {};
    wc.lpfnWndProc = stub_wndproc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = k_class;
    wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    RegisterClassW(&wc);
}

std::wstring widen(std::string const& utf8)
{
    if (utf8.empty())
        return {};
    int n = MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(), -1, nullptr, 0);
    std::wstring out(n > 0 ? n - 1 : 0, L'\0');
    if (n > 1)
        MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(), -1, out.data(), n);
    return out;
}

#endif

} // namespace

View::View(Session& session, std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks)
    : m_session(session)
    , m_parent(parent_hwnd)
    , m_cb(callbacks)
{
    (void)m_session;
#if defined(_WIN32)
    ensure_class();
    HWND parent = reinterpret_cast<HWND>(parent_hwnd);
    if (parent) {
        m_hwnd = CreateWindowExW(
            0,
            k_class,
            L"Ladybird stub",
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            m_w,
            m_h,
            parent,
            nullptr,
            GetModuleHandleW(nullptr),
            nullptr);
        if (m_hwnd) {
            SetWindowLongPtrW(m_hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(new std::wstring(widen(stub_label(m_url)))));
        }
    }
#else
    (void)m_parent;
#endif
}

View::~View()
{
#if defined(_WIN32)
    if (m_hwnd) {
        DestroyWindow(m_hwnd);
        m_hwnd = nullptr;
    }
#endif
}

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

void View::paint_label()
{
#if defined(_WIN32)
    if (!m_hwnd)
        return;
    auto* text = reinterpret_cast<std::wstring*>(GetWindowLongPtrW(m_hwnd, GWLP_USERDATA));
    if (text) {
        *text = widen(stub_label(m_url));
        InvalidateRect(m_hwnd, nullptr, TRUE);
    }
#endif
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
    paint_label();
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
#if defined(_WIN32)
    if (m_hwnd)
        MoveWindow(m_hwnd, x, y, w, h, TRUE);
#endif
}

void View::set_visible(bool visible)
{
    m_visible = visible;
#if defined(_WIN32)
    if (m_hwnd)
        ShowWindow(m_hwnd, visible ? SW_SHOW : SW_HIDE);
#endif
}

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
    paint_label();
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
    paint_label();
    emit_load(true);
}

bool View::screenshot_png(std::string const&)
{
    return false;
}

bool View::print_page()
{
    return false;
}

bool View::a11y_json(std::string&)
{
    return false;
}

bool View::dom_json(std::string&)
{
    return false;
}

bool View::console_json(std::string&, bool)
{
    return false;
}

bool View::eval_js(std::string const&, std::string&)
{
    return false;
}

bool View::capabilities_json(std::string&)
{
    return false;
}

bool View::ext_reply(std::string const&, std::string const&, std::string const&)
{
    return false;
}

void View::drain_ext_queue()
{
}

} // namespace kakangkung::ladybird
