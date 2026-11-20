#if !defined(HAVE_LADYBIRD)
#    error "downloads_ladybird.cpp requires -DHAVE_LADYBIRD=1"
#endif

#include "kakangkung_ladybird.h"

#include <AK/ByteString.h>
#include <AK/LexicalPath.h>
#include <LibURL/URL.h>
#include <LibWebView/Application.h>
#include <LibWebView/FileDownloader.h>

#include <cstdio>
#include <mutex>
#include <string>

namespace {

using WebView::FileDownloader;
using WebView::FileDownloaderObserver;
using WebView::IsPrivate;

std::mutex g_emit_mutex;
void (*g_emit)(char const* json_line) = nullptr;
FileDownloaderObserver* g_observer = nullptr;

char const* status_name(FileDownloader::DownloadStatus status)
{
    switch (status) {
    case FileDownloader::DownloadStatus::InProgress:
        return "in_progress";
    case FileDownloader::DownloadStatus::Paused:
        return "paused";
    case FileDownloader::DownloadStatus::Completed:
        return "done";
    case FileDownloader::DownloadStatus::Canceled:
        return "stopped";
    case FileDownloader::DownloadStatus::Failed:
        return "stopped";
    }
    return "in_progress";
}

std::string json_escape(std::string const& in)
{
    std::string out;
    out.reserve(in.size() + 8);
    for (char c : in) {
        switch (c) {
        case '\\':
            out += "\\\\";
            break;
        case '"':
            out += "\\\"";
            break;
        case '\n':
            out += "\\n";
            break;
        default:
            out += c;
            break;
        }
    }
    return out;
}

void emit_download(FileDownloader::Download const& d)
{
    std::lock_guard lock(g_emit_mutex);
    if (!g_emit)
        return;
    auto url = d.url.to_byte_string();
    auto path = d.destination.string();
    long long total = d.total_size.has_value() ? static_cast<long long>(*d.total_size) : -1;
    char buf[2048];
    std::snprintf(buf, sizeof(buf),
        "{\"event\":\"download\",\"downloadId\":%llu,\"url\":\"%s\",\"path\":\"%s\",\"received\":%llu,\"total\":%lld,\"state\":\"%s\",\"can_resume\":%s}",
        static_cast<unsigned long long>(d.id),
        json_escape(std::string { url.characters() }).c_str(),
        json_escape(std::string { path.characters() }).c_str(),
        static_cast<unsigned long long>(d.downloaded_size),
        total,
        status_name(d.status),
        d.can_resume ? "true" : "false");
    g_emit(buf);
}

class KakDownloadObserver final : public FileDownloaderObserver {
public:
    AK_ALLOC_WITH_KMALLOC;

    void download_added(FileDownloader::Download const& download) override { emit_download(download); }
    void download_updated(FileDownloader::Download const& download) override { emit_download(download); }
    void download_removed(u64 id, IsPrivate) override
    {
        std::lock_guard lock(g_emit_mutex);
        if (!g_emit)
            return;
        char buf[128];
        std::snprintf(buf, sizeof(buf), "{\"event\":\"download_removed\",\"downloadId\":%llu}",
            static_cast<unsigned long long>(id));
        g_emit(buf);
    }
};

void ensure_observer()
{
    if (g_observer)
        return;
    if (!WebView::Application::is_initialized())
        return;
    // Owns itself for process lifetime; FileDownloaderObserver registers in its ctor.
    g_observer = new KakDownloadObserver();
}

} // namespace

extern "C" {

void kk_lb_set_download_emit(void (*fn)(char const* json_line))
{
    std::lock_guard lock(g_emit_mutex);
    g_emit = fn;
}

int kk_lb_download_start(char const* url_utf8, char const* path_utf8)
{
    if (!url_utf8 || !path_utf8 || !*url_utf8 || !*path_utf8)
        return 0;
    if (!WebView::Application::is_initialized())
        return 0;
    ensure_observer();
    auto url = URL::create_with_url_or_path(ByteString { url_utf8 });
    if (!url.has_value())
        return 0;
    LexicalPath dest { ByteString { path_utf8 } };
    auto id = WebView::Application::the().file_downloader().download_file(
        IsPrivate::No, url.release_value(), dest, FileDownloader::MayReplaceDestination::Yes);
    return id > 0 ? 1 : 0;
}

int kk_lb_download_action(uint64_t id, char const* action_utf8)
{
    if (!action_utf8 || !WebView::Application::is_initialized())
        return 0;
    ensure_observer();
    auto& dl = WebView::Application::the().file_downloader();
    std::string action { action_utf8 };
    if (action == "pause")
        dl.pause_download(id);
    else if (action == "resume")
        dl.resume_download(id);
    else if (action == "cancel")
        dl.cancel_download(id);
    else
        return 0;
    return 1;
}

void kk_lb_downloads_ensure_observer(void)
{
    ensure_observer();
}

} // extern "C"
