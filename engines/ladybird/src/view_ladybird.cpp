// Real Ladybird UI port — compiled only with -DHAVE_LADYBIRD=1 and LADYBIRD_* paths.
// Subclass WebView::ViewImplementation per Ladybird Documentation/Porting.md.
// Parent the view's native surface into Kakangkung's HWND (passed as parent_hwnd).
//
// Build on Boat (https://boat.dev/dashboard), sandbox kakangkung-ladybird / bx_cnfw3mch:
//   ./engines/ladybird/build.sh --ladybird
//
// Until this file is wired into CMakeLists when HAVE_LADYBIRD=ON and linked against
// LibWebView, the stub in stub_page.cpp paints an explanatory child HWND instead.

#if defined(HAVE_LADYBIRD)

#    include "view.h"

#    include "application.h"

// Placeholder: include Ladybird headers once the Boat tree is present, e.g.:
//   #include <LibWebView/ViewImplementation.h>
//   #include <LibWebView/Application.h>
//
// class kakangkung::ladybird::View final : public WebView::ViewImplementation { ... };

#    error "view_ladybird.cpp: implement ViewImplementation against a Ladybird checkout (Boat). See engines/ladybird/README.md."

#endif
