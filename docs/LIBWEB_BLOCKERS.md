# LibWeb upstream blockers (no stubs)

Kakangkung is Ladybird-only. These Chrome surfaces stay **documented blockers** until
LibWeb exposes a real API. We will not ship fake pickers, mock service workers, or
pretend PDF print pipelines.

| ID | Chrome surface | LibWeb status | Kakangkung today |
|----|----------------|---------------|------------------|
| **KB-23 / B1** | Presentation API / Chromecast / `RemotePlayback` | No IDL / headers for casting (in-tree `*Presentation*` = GPU frames) | Docs only; no cast protocol |
| **KB-24 / B2** | MV3 extension **background service workers** | Content-script `chrome.*` only; no extension SW process | `storage.local` + messaging demux; SW APIs (`getBackgroundPage` / `connect` / `getContexts`) reject with clear errors |
| **KB-20+ / B3** | Paginated print / `print_to_pdf` | No `print_to_pdf` on LibWebView | Screenshot preview + `window.print()` confirm |
| **B4** | Process-per-site isolation | LibWeb already swaps WebContent cross-site | Task Manager surfaces ProcessManager sites |
| **B5** | Linux / macOS product packages | App chrome is Win32/Direct2D today | Linux: host+Lagom runtime package; macOS: not yet |
| **B6** | Safe Browsing + cloud sync | SmartScreen was WebView2-only | Ladybird URL threat checks + folder/Drive sync foundation |

**Boat:** sandbox `bx_cnfw3mch` verifies real `engine=ladybird` + Lagom. Revisit each row when upstream lands the Web API.
