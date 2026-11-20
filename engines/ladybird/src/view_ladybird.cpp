// Real Ladybird UI port — HAVE_LADYBIRD=1.
// WebView::HeadlessWebView (LibWeb) + CPU frame blit into native surface
// (Win32 HWND parented into Kakangkung, or X11 window on Linux/Boat).

#if !defined(HAVE_LADYBIRD)
#    error "view_ladybird.cpp requires -DHAVE_LADYBIRD=1"
#endif

#include "view.h"
#include "application.h"

#include <AK/ByteString.h>
#include <LibCore/EventLoop.h>
#include <LibGfx/Bitmap.h>
#include <LibGfx/SystemTheme.h>
#include <LibURL/URL.h>
#include <LibWebView/Application.h>
#include <LibWebView/HeadlessWebView.h>
#include <LibWebView/PageBackingStores.h>
#include <LibWebView/URL.h>

#include <cstdlib>
#include <utility>

#if defined(_WIN32)
#    ifndef NOMINMAX
#        define NOMINMAX
#    endif
#    include <windows.h>
#else
#    include <X11/Xlib.h>
#    include <X11/Xutil.h>
#endif

namespace kakangkung::ladybird {

namespace {

class KakangkungApplication final : public WebView::Application {
    WEB_VIEW_APPLICATION(KakangkungApplication)
public:
    KakangkungApplication() = default;
};

class LibWebSurface final : public WebView::HeadlessWebView {
public:
    static NonnullOwnPtr<LibWebSurface> create(Core::AnonymousBuffer theme, Web::DevicePixelSize size,
        std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks)
    {
        auto view = adopt_own(*new LibWebSurface(move(theme), size, parent_hwnd, callbacks));
        view->initialize_tab(Web::HTML::VisibilityState::Visible);
        view->set_presents_from_cpu_memory(true);
        view->wire_callbacks();
        return view;
    }

    void set_native_bounds(int x, int y, int w, int h)
    {
        m_x = x;
        m_y = y;
        m_w = w > 0 ? w : 1;
        m_h = h > 0 ? h : 1;
#if defined(_WIN32)
        if (m_hwnd)
            MoveWindow(m_hwnd, m_x, m_y, m_w, m_h, TRUE);
#else
        if (m_display && m_window)
            XMoveResizeWindow(m_display, m_window, m_x, m_y, static_cast<unsigned>(m_w), static_cast<unsigned>(m_h));
#endif
        reset_viewport_size({ m_w, m_h });
    }

    void set_native_visible(bool on)
    {
#if defined(_WIN32)
        if (m_hwnd)
            ShowWindow(m_hwnd, on ? SW_SHOW : SW_HIDE);
#else
        if (m_display && m_window) {
            if (on)
                XMapWindow(m_display, m_window);
            else
                XUnmapWindow(m_display, m_window);
            XFlush(m_display);
        }
#endif
        set_system_visibility_state(on ? Web::HTML::VisibilityState::Visible : Web::HTML::VisibilityState::Hidden);
    }

    ~LibWebSurface() override
    {
#if defined(_WIN32)
        if (m_hwnd) {
            DestroyWindow(m_hwnd);
            m_hwnd = nullptr;
        }
#else
        if (m_display && m_window) {
            XDestroyWindow(m_display, m_window);
            m_window = 0;
        }
        if (m_display) {
            XCloseDisplay(m_display);
            m_display = nullptr;
        }
#endif
    }

private:
    LibWebSurface(Core::AnonymousBuffer theme, Web::DevicePixelSize size, std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks)
        : HeadlessWebView(move(theme), size)
        , m_cb(callbacks)
        , m_parent(parent_hwnd)
        , m_w(size.width().value())
        , m_h(size.height().value())
    {
        create_native_window();
    }

    void wire_callbacks()
    {
        on_title_change = [this](auto const& title) {
            if (m_cb.on_title)
                m_cb.on_title(m_cb.user, title.to_byte_string().characters());
        };
        on_url_change = [this](auto const& url) {
            auto text = url.to_byte_string();
            if (m_cb.on_url)
                m_cb.on_url(m_cb.user, text.characters());
        };
        on_load_start = [this] {
            if (m_cb.on_load)
                m_cb.on_load(m_cb.user, 0);
        };
        on_load_finish = [this](auto const&) {
            if (m_cb.on_load)
                m_cb.on_load(m_cb.user, 1);
        };
    }

    void create_native_window()
    {
#if defined(_WIN32)
        static bool registered = false;
        if (!registered) {
            WNDCLASSW wc {};
            wc.lpfnWndProc = DefWindowProcW;
            wc.hInstance = GetModuleHandleW(nullptr);
            wc.lpszClassName = L"KakangkungLibWebSurface";
            wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
            RegisterClassW(&wc);
            registered = true;
        }
        HWND parent = reinterpret_cast<HWND>(m_parent);
        DWORD style = parent ? (WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS) : (WS_OVERLAPPEDWINDOW | WS_VISIBLE);
        m_hwnd = CreateWindowExW(0, L"KakangkungLibWebSurface", L"LibWeb", style, m_x, m_y, m_w, m_h,
            parent, nullptr, GetModuleHandleW(nullptr), nullptr);
#else
        m_display = XOpenDisplay(nullptr);
        if (!m_display)
            return;
        int screen = DefaultScreen(m_display);
        Window root = RootWindow(m_display, screen);
        m_window = XCreateSimpleWindow(m_display, root, m_x, m_y, static_cast<unsigned>(m_w), static_cast<unsigned>(m_h), 0,
            BlackPixel(m_display, screen), WhitePixel(m_display, screen));
        XStoreName(m_display, m_window, "Kakangkung · LibWeb");
        XSelectInput(m_display, m_window, ExposureMask | StructureNotifyMask);
        XMapWindow(m_display, m_window);
        XFlush(m_display);
#endif
    }

    virtual void did_accept_presented_backing_store(i32, Gfx::IntRect) override
    {
        auto frame = backing_stores().displayed_frame();
        if (!frame.has_value() || !frame->shared_image_buffer)
            return;
        auto bitmap = frame->shared_image_buffer->bitmap_if_present();
        if (!bitmap)
            return;
        blit_bitmap(*bitmap);
    }

    void blit_bitmap(Gfx::Bitmap const& bitmap)
    {
        int const bw = bitmap.width();
        int const bh = bitmap.height();
        if (bw <= 0 || bh <= 0)
            return;

        Vector<u32> pixels;
        pixels.resize(static_cast<size_t>(bw) * bh);
        bool const red_low = bitmap.format() == Gfx::BitmapFormat::RGBA8888 || bitmap.format() == Gfx::BitmapFormat::RGBx8888;
        for (int y = 0; y < bh; ++y) {
            auto const* src = bitmap.scanline(y);
            for (int x = 0; x < bw; ++x) {
                u32 const p = src[x];
                u8 const a = (p >> 24) & 0xff;
                u8 const lo = p & 0xff;
                u8 const mid = (p >> 8) & 0xff;
                u8 const hi = (p >> 16) & 0xff;
                u8 const r = red_low ? lo : hi;
                u8 const b = red_low ? hi : lo;
                pixels[static_cast<size_t>(y) * bw + x] = (u32(a) << 24) | (u32(r) << 16) | (u32(mid) << 8) | u32(b);
            }
        }

#if defined(_WIN32)
        if (!m_hwnd)
            return;
        BITMAPINFO info {};
        info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
        info.bmiHeader.biWidth = bw;
        info.bmiHeader.biHeight = -bh;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        HDC hdc = GetDC(m_hwnd);
        StretchDIBits(hdc, 0, 0, m_w, m_h, 0, 0, bw, bh, pixels.data(), &info, DIB_RGB_COLORS, SRCCOPY);
        ReleaseDC(m_hwnd, hdc);
#else
        if (!m_display || !m_window)
            return;
        XImage image {};
        image.width = bw;
        image.height = bh;
        image.format = ZPixmap;
        image.data = reinterpret_cast<char*>(pixels.data());
        image.byte_order = LSBFirst;
        image.bitmap_unit = 32;
        image.bitmap_bit_order = LSBFirst;
        image.bitmap_pad = 32;
        image.depth = 24;
        image.bytes_per_line = bw * 4;
        image.bits_per_pixel = 32;
        image.red_mask = 0xff0000;
        image.green_mask = 0x00ff00;
        image.blue_mask = 0x0000ff;
        GC gc = DefaultGC(m_display, DefaultScreen(m_display));
        XPutImage(m_display, m_window, gc, &image, 0, 0, 0, 0, static_cast<unsigned>(bw), static_cast<unsigned>(bh));
        XFlush(m_display);
#endif
    }

    kk_lb_callbacks m_cb {};
    std::uintptr_t m_parent {};
    int m_x {}, m_y {}, m_w { 800 }, m_h { 600 };
#if defined(_WIN32)
    HWND m_hwnd { nullptr };
#else
    Display* m_display { nullptr };
    Window m_window { 0 };
#endif
};

static OwnPtr<KakangkungApplication> g_app;

static ErrorOr<void> ensure_application()
{
    if (WebView::Application::is_initialized())
        return {};
    static char arg0[] = "kakangkung-ladybird-host";
    static char* argv[] = { arg0, nullptr };
    static auto const arg0_view = StringView { arg0, sizeof("kakangkung-ladybird-host") - 1 };
    static StringView string_storage[] = { arg0_view };
    Main::Arguments arguments {
        .argc = 1,
        .argv = argv,
        .strings = Span<StringView> { string_storage },
    };
    g_app = TRY(KakangkungApplication::create(arguments));
    return {};
}

static ErrorOr<Core::AnonymousBuffer> load_theme()
{
    char const* src = getenv("LADYBIRD_SOURCE_DIR");
    if (src && *src) {
        auto path = ByteString::formatted("{}/Base/res/themes/Default.ini", src);
        if (auto theme = Gfx::load_system_theme(path); !theme.is_error())
            return theme.release_value();
    }
    return TRY(Core::AnonymousBuffer::create_with_size(4096));
}

} // namespace

struct View::LibWeb {
    OwnPtr<LibWebSurface> surface;
};

View::View(Session& session, std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks)
    : m_session(session)
    , m_libweb(std::make_unique<LibWeb>())
{
    if (auto result = ensure_application(); result.is_error()) {
        warnln("Kakangkung LibWeb Application failed: {}", result.error());
        return;
    }
    auto theme_or_error = load_theme();
    if (theme_or_error.is_error()) {
        warnln("Kakangkung LibWeb theme failed: {}", theme_or_error.error());
        return;
    }
    m_libweb->surface = LibWebSurface::create(theme_or_error.release_value(), { 800, 600 }, parent_hwnd, callbacks);
    if (callbacks.on_load)
        callbacks.on_load(callbacks.user, 1);
}

View::~View() = default;

void View::navigate(std::string url)
{
    if (!m_libweb || !m_libweb->surface)
        return;
    if (auto parsed = WebView::sanitize_url(url); parsed.has_value())
        m_libweb->surface->load(*parsed);
    else
        m_libweb->surface->load(URL::URL::create_with_url_or_path(url));
}

void View::stop()
{
    if (m_libweb && m_libweb->surface)
        m_libweb->surface->stop_loading();
}

void View::reload()
{
    if (m_libweb && m_libweb->surface)
        m_libweb->surface->reload();
}

void View::set_bounds(int x, int y, int w, int h)
{
    if (m_libweb && m_libweb->surface)
        m_libweb->surface->set_native_bounds(x, y, w, h);
}

void View::set_visible(bool visible)
{
    if (m_libweb && m_libweb->surface)
        m_libweb->surface->set_native_visible(visible);
}

bool View::can_go_back() const
{
    return m_libweb && m_libweb->surface && m_libweb->surface->can_navigate_back();
}

bool View::can_go_forward() const
{
    return m_libweb && m_libweb->surface && m_libweb->surface->can_navigate_forward();
}

void View::go_back()
{
    if (m_libweb && m_libweb->surface)
        m_libweb->surface->traverse_the_history_by_delta(-1);
}

void View::go_forward()
{
    if (m_libweb && m_libweb->surface)
        m_libweb->surface->traverse_the_history_by_delta(1);
}

} // namespace kakangkung::ladybird
