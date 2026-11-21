// KB-22: LibWeb ProcessManager snapshots — real WebContent / helper PIDs with site URLs.
// Ladybird already swaps WebContent processes on cross-site navigation; we surface that
// tree in Kakangkung's task manager instead of inventing per-tab PIDs.

#if !defined(HAVE_LADYBIRD)
#    error "processes_ladybird.cpp requires -DHAVE_LADYBIRD=1"
#endif

#include "kakangkung_ladybird.h"

#include <AK/JsonArray.h>
#include <AK/JsonObject.h>
#include <AK/String.h>
#include <LibWebView/Application.h>
#include <LibWebView/ProcessManager.h>
#include <LibWebView/ProcessSnapshot.h>
#include <LibWebView/ProcessType.h>

#include <cstdlib>
#include <cstring>

namespace {

char const* type_name(WebView::ProcessType type)
{
    using WebView::ProcessType;
    switch (type) {
    case ProcessType::Browser:
        return "Ladybird";
    case ProcessType::UI:
        return "Ladybird UI";
    case ProcessType::Hub:
        return "Ladybird Hub";
    case ProcessType::Compositor:
        return "Compositor";
    case ProcessType::WebContent:
        return "WebContent";
    case ProcessType::RequestServer:
        return "RequestServer";
    case ProcessType::ImageDecoder:
        return "ImageDecoder";
    case ProcessType::MediaServer:
        return "MediaServer";
    case ProcessType::AudioServer:
        return "AudioServer";
    case ProcessType::VideoDecoderServer:
        return "VideoDecoder";
    case ProcessType::WebRTCClient:
        return "WebRTC";
    }
    return "Ladybird";
}

} // namespace

extern "C" {

int kk_lb_processes_json(char** out_utf8)
{
    if (!out_utf8)
        return 0;
    *out_utf8 = nullptr;

    auto snapshots = WebView::process_snapshots(WebView::Application::process_manager());
    JsonArray arr;
    for (auto const& snap : snapshots) {
        JsonObject o;
        o.set("pid"sv, JsonValue { static_cast<u64>(snap.pid) });
        o.set("child_id"sv, JsonValue { snap.child_id });
        auto const* kind = type_name(snap.type);
        o.set("kind"sv, JsonValue { MUST(String::from_utf8(StringView { kind, std::strlen(kind) })) });
        o.set("ws"sv, JsonValue { snap.memory_usage_bytes });
        o.set("cpu"sv, JsonValue { snap.cpu_percent });
        if (snap.title.has_value())
            o.set("title"sv, JsonValue { snap.title->to_well_formed_utf8() });
        if (snap.site_url.has_value())
            o.set("site"sv, JsonValue { snap.site_url->serialize() });
        else if (snap.page.has_value())
            o.set("site"sv, JsonValue { snap.page->url.serialize() });
        else if (!snap.tabs.is_empty())
            o.set("site"sv, JsonValue { snap.tabs[0].page.url.serialize() });
        arr.must_append(move(o));
    }
    auto json = arr.serialized();
    auto bytes = json.bytes();
    *out_utf8 = static_cast<char*>(std::malloc(bytes.size() + 1));
    if (!*out_utf8)
        return 0;
    std::memcpy(*out_utf8, bytes.data(), bytes.size());
    (*out_utf8)[bytes.size()] = '\0';
    return 1;
}

} // extern "C"
