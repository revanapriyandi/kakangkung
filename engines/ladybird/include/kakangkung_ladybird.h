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

typedef struct kk_lb_callbacks {
    void* user;
    kk_lb_title_fn on_title;
    kk_lb_url_fn on_url;
    kk_lb_load_fn on_load;
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

KK_LB_API void kk_lb_string_free(char* s);

/** Pump Ladybird / stub events; return non-zero if the session should quit. */
KK_LB_API int kk_lb_session_pump(kk_lb_session* session);

#ifdef __cplusplus
}
#endif
