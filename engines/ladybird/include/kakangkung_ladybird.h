/* C ABI between Kakangkung (Rust) and the Ladybird UI-port host.
 *
 * When built with HAVE_LADYBIRD=1 this wraps LibWebView. Otherwise it drives a stub page so the
 * protocol and process lifecycle can be tested without compiling Ladybird.
 */
#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#if defined(_WIN32)
#    ifdef KAKANGKUNG_LADYBIRD_BUILD
#        define KK_LB_API __declspec(dllexport)
#    else
#        define KK_LB_API __declspec(dllimport)
#    endif
#else
#    define KK_LB_API __attribute__((visibility("default")))
#endif

typedef struct kk_lb_session kk_lb_session;
typedef struct kk_lb_view kk_lb_view;

typedef void (*kk_lb_title_fn)(void* user, char const* title_utf8);
typedef void (*kk_lb_url_fn)(void* user, char const* url_utf8);
typedef void (*kk_lb_load_fn)(void* user, int done /* 0 loading, 1 finished */);
/** MV3 chrome.runtime.sendMessage bridge (kk-ext:…). message_json is raw JSON. */
typedef void (*kk_lb_ext_fn)(void* user, char const* extension_id_utf8, char const* request_id_utf8,
    char const* message_json);
/** Page → host IPC (password manager kk:pw:…, media, …). */
typedef void (*kk_lb_page_ipc_fn)(void* user, char const* message_utf8);

typedef struct kk_lb_callbacks {
    void* user;
    kk_lb_title_fn on_title;
    kk_lb_url_fn on_url;
    kk_lb_load_fn on_load;
    kk_lb_ext_fn on_ext_message; /* may be null */
    kk_lb_page_ipc_fn on_page_ipc; /* may be null */
} kk_lb_callbacks;

/** "ladybird" when LibWeb is linked, "stub" otherwise. */
KK_LB_API char const* kk_lb_engine_id(void);

KK_LB_API kk_lb_session* kk_lb_session_create(char const* profile_dir_utf8);
KK_LB_API void kk_lb_session_destroy(kk_lb_session* session);

/**
 * Create a view. On Windows, parent_hwnd is an HWND the host should parent into (may be 0 for
 * the host's own top-level window when running as a separate process).
 */
KK_LB_API kk_lb_view* kk_lb_view_create(kk_lb_session* session, uintptr_t parent_hwnd, kk_lb_callbacks callbacks);
KK_LB_API void kk_lb_view_destroy(kk_lb_view* view);

KK_LB_API void kk_lb_view_navigate(kk_lb_view* view, char const* url_utf8);
KK_LB_API void kk_lb_view_stop(kk_lb_view* view);
KK_LB_API void kk_lb_view_reload(kk_lb_view* view);
KK_LB_API void kk_lb_view_set_bounds(kk_lb_view* view, int x, int y, int w, int h);
KK_LB_API void kk_lb_view_set_visible(kk_lb_view* view, int visible);
KK_LB_API int kk_lb_view_can_go_back(kk_lb_view* view);
KK_LB_API int kk_lb_view_can_go_forward(kk_lb_view* view);
KK_LB_API void kk_lb_view_go_back(kk_lb_view* view);
KK_LB_API void kk_lb_view_go_forward(kk_lb_view* view);

/**
 * Capture the current LibWeb frame to a PNG path. Returns 1 on success.
 * With HAVE_LADYBIRD this uses take_frame_snapshot; without it, returns 0.
 */
KK_LB_API int kk_lb_view_screenshot(kk_lb_view* view, char const* path_utf8);

/**
 * Request print of the current document (window.print / LibWeb print path).
 * Returns 1 when the request was handed to LibWeb.
 */
KK_LB_API int kk_lb_view_print(kk_lb_view* view);

/**
 * Page zoom (KB-8). relative_factor null → reset to 1.0; else multiply (clamped 0.25…5).
 * absolute_factor non-null sets that factor instead. Writes resulting factor to out_factor.
 * Returns 1 when applied in LibWeb.
 */
KK_LB_API int kk_lb_view_zoom(kk_lb_view* view, double const* relative_factor, double const* absolute_factor, double* out_factor);

/**
 * Write a JSON accessibility tree snapshot into out_utf8 (caller frees with kk_lb_string_free).
 * Returns 1 on success.
 */
KK_LB_API int kk_lb_view_a11y_json(kk_lb_view* view, char** out_utf8);

/**
 * Write a JSON DOM tree snapshot (LibWeb inspect_dom_tree) into out_utf8.
 * Caller frees with kk_lb_string_free. Returns 1 on success.
 */
KK_LB_API int kk_lb_view_dom_json(kk_lb_view* view, char** out_utf8);

/**
 * Write a JSON array of buffered console messages into out_utf8.
 * When clear_after is non-zero, the buffer is drained. Caller frees. Returns 1 on success.
 */
KK_LB_API int kk_lb_view_console_json(kk_lb_view* view, char** out_utf8, int clear_after);

/**
 * Run JavaScript in the page. With HAVE_LADYBIRD uses run_javascript.
 * out_utf8 receives a JSON string result when available (caller frees).
 * Returns 1 when the script was submitted to LibWeb.
 */
KK_LB_API int kk_lb_view_eval(kk_lb_view* view, char const* js_utf8, char** out_utf8);

/**
 * Probe LibWeb feature support in the page (CSS/JS/WASM/WebGL/WebGPU/media).
 * Writes a JSON object into out_utf8 (caller frees). Returns 1 on success.
 */
KK_LB_API int kk_lb_view_capabilities(kk_lb_view* view, char** out_utf8);

/**
 * Deliver a chrome.runtime.sendMessage response into the page (LibWeb).
 * response_json may be a JSON value or empty for undefined. Returns 1 on submit.
 */
KK_LB_API int kk_lb_view_ext_reply(kk_lb_view* view, char const* extension_id_utf8, char const* request_id_utf8,
    char const* response_json);

/** Drain pending chrome.runtime.sendMessage entries from the page into on_ext_message. */
KK_LB_API void kk_lb_view_drain_ext(kk_lb_view* view);

/** KB-6: LibWeb FileDownloader → protocol lines (one JSON object per call). */
KK_LB_API void kk_lb_set_download_emit(void (*fn)(char const* json_line));
KK_LB_API void kk_lb_downloads_ensure_observer(void);
KK_LB_API int kk_lb_download_start(char const* url_utf8, char const* path_utf8);
KK_LB_API int kk_lb_download_action(uint64_t id, char const* action_utf8 /* pause|resume|cancel */);

/**
 * KB-7: configure LibWeb ContentFilter/ContentBlocker from Kakangkung domain lists.
 * enabled=0 clears patterns. allowlist_utf8 is newline-separated hosts (Rust enforces per-site).
 */
KK_LB_API int kk_lb_adblock_configure(int enabled, char const* allowlist_utf8);
KK_LB_API int kk_lb_adblock_enabled(void);

/**
 * KB-11: clear LibWeb cookies/site storage and/or HTTP cache accessed since since_ms
 * (Unix ms; 0 = all time). Returns 1 when LibWeb finished the clear, 0 on failure.
 */
KK_LB_API int kk_lb_clear_data(int cookies, int cache, int64_t since_ms);

KK_LB_API void kk_lb_string_free(char* s);

/** Pump Ladybird / stub events; return non-zero if the session should quit. */
KK_LB_API int kk_lb_session_pump(kk_lb_session* session);

#ifdef __cplusplus
}
#endif
