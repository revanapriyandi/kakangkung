// Real Ladybird UI port — HAVE_LADYBIRD=1.
// WebView::HeadlessWebView (LibWeb) + CPU frame blit into native surface
// (Win32 HWND parented into Kakangkung, or X11 window on Linux/Boat).

#if !defined(HAVE_LADYBIRD)
#    error "view_ladybird.cpp requires -DHAVE_LADYBIRD=1"
#endif

#include "view.h"
#include "application.h"
#include "extensions.h"
#if !defined(_WIN32)
#    include "x11_surface.h"
#endif

#include <AK/ByteString.h>
#include <AK/JsonObject.h>
#include <AK/String.h>
#include <LibCore/EventLoop.h>
#include <LibCore/File.h>
#include <LibGfx/Bitmap.h>
#include <LibGfx/ImageFormats/PNGWriter.h>
#include <LibGfx/ShareableBitmap.h>
#include <LibGfx/SystemTheme.h>
#include <LibURL/URL.h>
#include <LibWebCommon/WebView/ConsoleOutput.h>
#include <LibWebCommon/WebView/Debugger.h>
#include <LibWebView/Application.h>
#include <LibWebView/HeadlessWebView.h>
#include <LibWebView/PageBackingStores.h>
#include <LibWebView/URL.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <utility>

#if defined(_WIN32)
#    ifndef NOMINMAX
#        define NOMINMAX
#    endif
#    include <windows.h>
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
        std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks, std::string profile_dir)
    {
        auto view = adopt_own(*new LibWebSurface(move(theme), size, parent_hwnd, callbacks, move(profile_dir)));
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
        kk_x11_surface_set_bounds(m_x11, m_x, m_y, m_w, m_h);
#endif
        reset_viewport_size({ m_w, m_h });
    }

    void set_native_visible(bool on)
    {
#if defined(_WIN32)
        if (m_hwnd)
            ShowWindow(m_hwnd, on ? SW_SHOW : SW_HIDE);
#else
        kk_x11_surface_set_visible(m_x11, on ? 1 : 0);
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
        kk_x11_surface_destroy(m_x11);
        m_x11 = nullptr;
#endif
    }

private:
    LibWebSurface(Core::AnonymousBuffer theme, Web::DevicePixelSize size, std::uintptr_t parent_hwnd, kk_lb_callbacks callbacks,
        std::string profile_dir)
        : HeadlessWebView(move(theme), size)
        , m_cb(callbacks)
        , m_parent(parent_hwnd)
        , m_profile_dir(move(profile_dir))
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
            m_url = text.characters();
            if (m_cb.on_url)
                m_cb.on_url(m_cb.user, text.characters());
        };
        on_load_start = [this] {
            if (m_cb.on_load)
                m_cb.on_load(m_cb.user, 0);
        };
        on_load_finish = [this](auto const&) {
            inject_content_scripts();
            if (m_cb.on_load)
                m_cb.on_load(m_cb.user, 1);
        };
        on_console_message = [this](WebView::ConsoleOutput const& output) {
            // chrome.runtime.sendMessage bridge: content scripts log kk-ext:{...}
            output.output.visit(
                [this](WebView::ConsoleLog const& log) {
                    for (auto const& arg : log.arguments) {
                        if (!arg.is_string())
                            continue;
                        auto const& msg = arg.as_string();
                        if (!msg.starts_with_bytes("kk-ext:"sv))
                            continue;
                        // Host-side extension message bus can pick this up later (MCP / DevTools).
                        (void)this;
                        (void)msg;
                    }
                },
                [](auto const&) {});
        };
    }

    void inject_content_scripts()
    {
        if (m_url.empty() || m_profile_dir.empty())
            return;
        auto scripts = scripts_for_url(load_content_scripts(m_profile_dir), m_url);
        for (auto const& src : scripts) {
            auto script = MUST(String::from_utf8(StringView { src.data(), src.size() }));
            run_javascript(script);
        }
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
        m_x11 = kk_x11_surface_create(m_x, m_y, m_w, m_h);
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
        kk_x11_surface_blit(m_x11, bw, bh, pixels.data());
#endif
    }

    kk_lb_callbacks m_cb {};
    std::uintptr_t m_parent {};
    std::string m_profile_dir;
    std::string m_url;
    int m_x {}, m_y {}, m_w { 800 }, m_h { 600 };
#if defined(_WIN32)
    HWND m_hwnd { nullptr };
#else
    KkX11Surface* m_x11 { nullptr };
#endif
};

static OwnPtr<KakangkungApplication> g_app;

static ErrorOr<void> ensure_application()
{
    if (WebView::Application::is_initialized())
        return {};
    // Chrome-parity engine knobs (KB-5): experimental IDL (WebGPU etc.), media autoplay,
    // and CPU painting so HeadlessWebView frames reach our HWND/X11 blit path.
    static char arg0[] = "kakangkung-ladybird-host";
    static char arg_exp[] = "--expose-experimental-interfaces";
    static char arg_auto[] = "--enable-autoplay";
    static char arg_cpu[] = "--force-cpu-painting";
    static char* argv[] = { arg0, arg_exp, arg_auto, arg_cpu, nullptr };
    static StringView string_storage[] = {
        StringView { arg0, sizeof(arg0) - 1 },
        StringView { arg_exp, sizeof(arg_exp) - 1 },
        StringView { arg_auto, sizeof(arg_auto) - 1 },
        StringView { arg_cpu, sizeof(arg_cpu) - 1 },
    };
    Main::Arguments arguments {
        .argc = 4,
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
    m_libweb->surface = LibWebSurface::create(theme_or_error.release_value(), { 800, 600 }, parent_hwnd, callbacks, session.profile_dir());
    if (callbacks.on_load)
        callbacks.on_load(callbacks.user, 1);
}

View::~View() = default;

void View::navigate(std::string url)
{
    if (!m_libweb || !m_libweb->surface)
        return;
    auto view = StringView { url.data(), url.size() };
    if (auto parsed = WebView::sanitize_url(view); parsed.has_value()) {
        m_libweb->surface->load(*parsed);
        return;
    }
    if (auto fallback = URL::create_with_url_or_path(ByteString { url.c_str(), url.size() }); fallback.has_value())
        m_libweb->surface->load(*fallback);
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

namespace {

bool pump_until(bool const& done, std::chrono::milliseconds budget)
{
    auto const deadline = std::chrono::steady_clock::now() + budget;
    while (!done) {
        Core::EventLoop::current().pump(Core::EventLoop::WaitMode::PollForEvents);
        if (std::chrono::steady_clock::now() >= deadline)
            return false;
    }
    return true;
}

std::string debugger_value_to_string(WebView::DebuggerValue const& value)
{
    using WebView::DebuggerValueType;
    switch (value.type) {
    case DebuggerValueType::Undefined:
        return "undefined";
    case DebuggerValueType::Null:
        return "null";
    case DebuggerValueType::Boolean:
        return value.boolean_value ? "true" : "false";
    case DebuggerValueType::Number: {
        char buf[64];
        std::snprintf(buf, sizeof(buf), "%g", value.number_value);
        return buf;
    }
    case DebuggerValueType::String:
    case DebuggerValueType::BigInt:
        return std::string { value.text.to_byte_string().characters() };
    case DebuggerValueType::Object: {
        auto klass = value.object_class.to_byte_string();
        return std::string("[object ") + klass.characters() + "]";
    }
    case DebuggerValueType::Uninitialized:
        return "uninitialized";
    }
    return {};
}

} // namespace

bool View::screenshot_png(std::string const& path)
{
    if (!m_libweb || !m_libweb->surface || path.empty())
        return false;

    auto write_bitmap = [&](Gfx::Bitmap const& bitmap) -> bool {
        auto encoded = Gfx::PNGWriter::encode(bitmap);
        if (encoded.is_error())
            return false;
        auto file_or_error = Core::File::open(StringView { path.data(), path.size() }, Core::File::OpenMode::Write);
        if (file_or_error.is_error())
            return false;
        return !file_or_error.value()->write_until_depleted(encoded.value()).is_error();
    };

    // Prefer a compositor snapshot; fall back to the CPU-presented frame we already blit.
    bool done = false;
    bool ok = false;
    m_libweb->surface->take_frame_snapshot([&](Optional<Gfx::ShareableBitmap> snapshot) {
        if (snapshot.has_value() && snapshot->bitmap())
            ok = write_bitmap(*snapshot->bitmap());
        done = true;
    });
    if (pump_until(done, std::chrono::seconds(5)) && ok)
        return true;

    if (auto frame = m_libweb->surface->backing_stores().displayed_frame(); frame.has_value()) {
        if (auto bitmap = frame->shared_image_buffer ? frame->shared_image_buffer->bitmap_if_present() : nullptr)
            return write_bitmap(*bitmap);
    }
    return false;
}

bool View::print_page()
{
    if (!m_libweb || !m_libweb->surface)
        return false;
    // LibWeb has no separate host print IPC yet; window.print() is the real document print path.
    m_libweb->surface->run_javascript("window.print()"_string);
    return true;
}

bool View::a11y_json(std::string& out_json)
{
    if (!m_libweb || !m_libweb->surface)
        return false;

    bool done = false;
    bool ok = false;
    m_libweb->surface->on_received_accessibility_tree = [&](JsonObject tree) {
        out_json = std::string { tree.serialized().to_byte_string().characters() };
        ok = true;
        done = true;
        m_libweb->surface->on_received_accessibility_tree = nullptr;
    };
    m_libweb->surface->inspect_accessibility_tree();
    if (!pump_until(done, std::chrono::seconds(10))) {
        m_libweb->surface->on_received_accessibility_tree = nullptr;
        return false;
    }
    return ok;
}

bool View::eval_js(std::string const& js, std::string& out_result)
{
    if (!m_libweb || !m_libweb->surface || js.empty())
        return false;

    bool done = false;
    bool ok = false;
    m_libweb->surface->on_received_js_console_result = [&](WebView::DebuggerValue result) {
        out_result = debugger_value_to_string(result);
        ok = true;
        done = true;
        m_libweb->surface->on_received_js_console_result = nullptr;
    };
    auto script = MUST(String::from_utf8(StringView { js.data(), js.size() }));
    m_libweb->surface->js_console_input(script);
    if (!pump_until(done, std::chrono::seconds(10))) {
        m_libweb->surface->on_received_js_console_result = nullptr;
        return false;
    }
    return ok;
}

bool View::capabilities_json(std::string& out_json)
{
    // Live LibWeb feature probe — not a static stub table.
    static char const* probe = R"JS((() => {
  const can = (fn) => { try { return !!fn(); } catch (e) { return false; } };
  const css = (q) => can(() => CSS.supports(q));
  return JSON.stringify({
    engine: "ladybird",
    html: typeof document !== "undefined",
    css_grid: css("display:grid"),
    css_flex: css("display:flex"),
    css_container: css("container-type:inline-size"),
    es_modules: can(() => typeof HTMLScriptElement !== "undefined"),
    wasm: typeof WebAssembly !== "undefined",
    webgl: can(() => {
      const c = document.createElement("canvas");
      return !!(c.getContext("webgl") || c.getContext("experimental-webgl"));
    }),
    webgl2: can(() => !!document.createElement("canvas").getContext("webgl2")),
    webgpu: typeof navigator !== "undefined" && "gpu" in navigator,
    webrtc: typeof RTCPeerConnection !== "undefined",
    service_worker: typeof navigator !== "undefined" && "serviceWorker" in navigator,
    media_source: typeof MediaSource !== "undefined",
    audio: typeof Audio !== "undefined",
    video: typeof HTMLVideoElement !== "undefined",
    codecs: {
      av1: can(() => MediaSource && MediaSource.isTypeSupported('video/mp4; codecs="av01.0.01M.08"')),
      vp9: can(() => MediaSource && MediaSource.isTypeSupported('video/webm; codecs="vp9"')),
      opus: can(() => MediaSource && MediaSource.isTypeSupported('audio/webm; codecs="opus"')),
      mp3: can(() => {
        const a = document.createElement("audio");
        return a.canPlayType('audio/mpeg') !== "";
      })
    }
  });
})())JS";
    std::string result;
    if (!eval_js(probe, result))
        return false;
    // Console may wrap string results in quotes — strip one layer if present.
    if (result.size() >= 2 && result.front() == '"' && result.back() == '"') {
        out_json = result.substr(1, result.size() - 2);
        // Unescape minimal JSON string escapes produced by DebuggerValue stringification.
        std::string unescaped;
        for (size_t i = 0; i < out_json.size(); ++i) {
            if (out_json[i] == '\\' && i + 1 < out_json.size()) {
                char n = out_json[++i];
                if (n == '"' || n == '\\' || n == '/')
                    unescaped += n;
                else if (n == 'n')
                    unescaped += '\n';
                else
                    unescaped += n;
            } else {
                unescaped += out_json[i];
            }
        }
        out_json = std::move(unescaped);
    } else {
        out_json = std::move(result);
    }
    return !out_json.empty() && out_json.front() == '{';
}

} // namespace kakangkung::ladybird
