# WebView2 API map

Checked against the **pinned sources** (`wry 0.57.1`, `webview2-com 0.39.1` / `webview2-com-sys 0.39.1`,
`windows 0.62.2`) by reading `~/.cargo/registry/src/.../<crate>-<version>/src`.

"Interface" is the COM interface in `webview2-com-sys/src/bindings.rs` that declares the method
(found by locating the `impl <Interface>` block containing it).

"Min runtime" is the first WebView2 Runtime that ships the interface. Values marked *(docs)* come from
Microsoft's WebView2 release notes and were **not** re-verified here; the installed runtime is
157.0.4341.42, newer than all of them. The ground truth on a given machine is the runtime
`QueryInterface` (`cast::<ICoreWebView2_N>()`): `--self-test` probes every interface below and records
which ones the installed runtime actually supports.

| Feature | wry 0.57.1 | Raw COM needed | Interface (webview2-com-sys 0.39.1) | Min runtime |
|---|---|---|---|---|
| Suspend / resume | not exposed | yes | `ICoreWebView2_3::TrySuspend` (`TrySuspendCompletedHandler`), `Resume`, `IsSuspended` | 89 *(docs)* |
| Memory target level | `WebViewExtWindows::set_memory_usage_level(MemoryUsageLevel::{Normal,Low})` | no | `ICoreWebView2_19::SetMemoryUsageTargetLevel` | 114.0.1823.32 (stated in wry's doc comment) |
| Audio detection | not exposed | yes | `ICoreWebView2_8::IsDocumentPlayingAudio`, `IsMuted`, `add_IsDocumentPlayingAudioChanged` | 95 *(docs)* |
| Favicon | not exposed | yes | `ICoreWebView2_15::add_FaviconChanged`, `GetFavicon` | 104 *(docs)* |
| Permissions | `with_permission_handler(Fn(PermissionKind) -> PermissionResponse)` — synchronous, no origin passed to the closure | yes, for per-site decisions (origin is on the event args) | `ICoreWebView2::add_PermissionRequested` (wry also adds one for clipboard if `with_clipboard(true)`) | base |
| Downloads | `with_download_started_handler(FnMut(url, &mut PathBuf) -> bool)`, `with_download_completed_handler` — start/finish only, **no progress, no cancel** | yes, for progress/cancel (`ICoreWebView2DownloadOperation`) | `ICoreWebView2_4::add_DownloadStarting` | 92 *(docs)* |
| Shortcuts while page has focus | `with_browser_accelerator_keys(bool)` only toggles browser keys | yes | `ICoreWebView2Controller::add_AcceleratorKeyPressed` (`AcceleratorKeyPressedEventHandler`) | base |
| Process list / memory | not exposed | yes | `ICoreWebView2Environment8::GetProcessInfos`, `add_ProcessInfosChanged`; `ICoreWebView2Environment13::GetProcessExtendedInfos` (adds frame → process mapping, used for per-tab attribution) | 99 / 120 *(docs)* |
| Process kinds | — | — | `COREWEBVIEW2_PROCESS_KIND_{BROWSER,RENDERER,UTILITY,SANDBOX_HELPER,GPU,PPAPI_PLUGIN,PPAPI_BROKER}` | — |
| Private mode | `with_incognito(true)` → `ICoreWebView2ControllerOptions::SetIsInPrivateModeEnabled` on controller creation | no | `ICoreWebView2Environment10` controller options | 101 *(docs)* |
| New window / popup | `with_new_window_req_handler(Fn(url, NewWindowFeatures) -> NewWindowResponse::{Allow, Create{webview}, Deny})` | no (Create lets us host it in a new tab) | `ICoreWebView2::add_NewWindowRequested` | base |
| Screenshot of page | not exposed | yes | `ICoreWebView2::CapturePreview` (PNG into an `IStream`) | base |
| Custom protocol (new-tab page) | `with_custom_protocol(name, handler)`; served as `http://<name>.<path>` (or `https://` with `with_https_scheme(true)`) | no | `AddWebResourceRequestedFilter` | base |
| Document title | `with_document_title_changed_handler` | no | — | base |
| Page load | `with_on_page_load_handler(PageLoadEvent::{Started,Finished}, url)` | no | — | base |

## Findings that differ from first assumptions

1. **`WebContext` does not share a WebView2 environment.** In wry 0.57.1 `InnerWebView::new_in_hwnd`
   calls `create_environment` for every webview unless `WebViewBuilderExtWindows::with_environment(env)`
   is given. `WebContext` only supplies the data directory. Webviews with the same user-data folder and
   the same options attach to the same browser process anyway, but to make sharing explicit the app
   creates the first webview, takes `WebViewExtWindows::environment()`, and passes it to every later
   normal tab via `with_environment`. Private tabs get a separate environment (separate data dir).
2. **wry passes default browser arguments**: `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`
   (and `--autoplay-policy=no-user-gesture-required` if autoplay is on). None are in the forbidden list
   (`--no-sandbox`, `--single-process`, `--disable-web-security`). They must be listed in the README with
   their measured effect (Phase 6). Disabling SmartScreen protection is a security reduction worth
   reviewing; overriding it requires `with_additional_browser_args`, which replaces the whole default string.
3. **Handler cleanup on drop**: wry's `Drop for InnerWebView` calls `controller.Close()` and destroys the
   child HWND, but does not `remove_*` the event tokens it registered. Handlers the app adds through raw
   COM keep their `EventRegistrationToken` and are removed explicitly before the tab's `WebView` is dropped.
4. **wry permission handler has no origin**: per-site remembered decisions need the raw
   `PermissionRequested` args (`Uri`), so Phase 5 registers its own handler instead of wry's.
5. **wry download handler has no progress/cancel**: Phase 5 uses `ICoreWebView2_4::add_DownloadStarting`
   directly to get the `ICoreWebView2DownloadOperation`.

## Suspend vs memory level

Both mechanisms are reachable: `TrySuspend` via raw `ICoreWebView2_3`, the level via wry's
`set_memory_usage_level`. The `tabs` crate's state machine decides which one is used; `platform`
only executes the resulting command, so mixing them is impossible by construction and tested without
Windows.
