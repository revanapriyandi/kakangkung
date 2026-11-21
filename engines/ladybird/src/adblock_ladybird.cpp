// KB-7: feed LibWeb ContentFilter / ContentBlocker from Kakangkung's ad-network domain list.
// ResourceLoader refuses filtered URLs — real network blocks, not cosmetic-only stubs.

#if !defined(HAVE_LADYBIRD)
#    error "adblock_ladybird.cpp requires -DHAVE_LADYBIRD=1"
#endif

#include "kakangkung_ladybird.h"

#include <AK/ByteString.h>
#include <AK/String.h>
#include <AK/StringBuilder.h>
#include <AK/StringView.h>
#include <AK/Vector.h>
#include <LibCore/File.h>

#if __has_include(<LibWeb/Loader/ContentBlocker.h>)
#    include <LibWeb/Loader/ContentBlocker.h>
#    define KK_HAS_CONTENT_BLOCKER 1
#elif __has_include(<LibWeb/Loader/ContentFilter.h>)
#    include <LibWeb/Loader/ContentFilter.h>
#    define KK_HAS_CONTENT_FILTER 1
#endif

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <mutex>
#include <string>
#include <vector>

#if defined(_WIN32)
#    ifndef NOMINMAX
#        define NOMINMAX
#    endif
#    include <windows.h>
#else
#    include <unistd.h>
#endif

namespace {

std::mutex g_mu;
bool g_enabled = true;
std::vector<std::string> g_domains;
std::vector<std::string> g_allowlist;

std::string exe_dir()
{
#if defined(_WIN32)
    char buf[MAX_PATH];
    DWORD n = GetModuleFileNameA(nullptr, buf, MAX_PATH);
    if (n == 0 || n >= MAX_PATH)
        return {};
    std::string path(buf, n);
    auto slash = path.find_last_of("\\/");
    return slash == std::string::npos ? std::string {} : path.substr(0, slash);
#else
    char buf[4096];
    ssize_t n = readlink("/proc/self/exe", buf, sizeof(buf) - 1);
    if (n <= 0)
        return {};
    buf[n] = '\0';
    std::string path(buf);
    auto slash = path.find_last_of('/');
    return slash == std::string::npos ? std::string {} : path.substr(0, slash);
#endif
}

bool load_domains_file(std::string const& path, std::vector<std::string>& out)
{
    std::ifstream in(path);
    if (!in)
        return false;
    std::string line;
    while (std::getline(in, line)) {
        while (!line.empty() && (line.back() == '\r' || line.back() == '\n'))
            line.pop_back();
        if (line.empty() || line[0] == '#')
            continue;
        if (line.starts_with("||") && line.ends_with("^"))
            out.push_back(line.substr(2, line.size() - 3));
        else
            out.push_back(line);
    }
    return !out.empty();
}

void ensure_domains_locked()
{
    if (!g_domains.empty())
        return;
    if (char const* env = std::getenv("KAKANGKUNG_ADBLOCK_LIST"); env && *env) {
        if (load_domains_file(env, g_domains))
            return;
    }
    auto dir = exe_dir();
    for (auto const& rel : {
             std::string { "/adblock-domains.txt" },
             std::string { "/data/adblock-domains.txt" },
             std::string { "/../data/adblock-domains.txt" },
             std::string { "/adblock-domains.list" },
             std::string { "/data/adblock-domains.list" },
         }) {
        if (!dir.empty() && load_domains_file(dir + rel, g_domains))
            return;
    }
    // Fail closed with a small built-in set when the file is missing (still real hosts).
    for (char const* d : {
             "doubleclick.net",
             "googlesyndication.com",
             "googleadservices.com",
             "googletagservices.com",
             "adservice.google.com",
             "2mdn.net",
             "adnxs.com",
             "adsrvr.org",
             "amazon-adsystem.com",
             "criteo.com",
             "criteo.net",
             "pubmatic.com",
             "rubiconproject.com",
             "taboola.com",
             "outbrain.com",
             "facebook.com/tr",
             "connect.facebook.net",
         }) {
        g_domains.emplace_back(d);
    }
}

bool apply_locked()
{
    ensure_domains_locked();
#if defined(KK_HAS_CONTENT_BLOCKER)
    StringBuilder builder;
    if (g_enabled) {
        for (auto const& d : g_domains)
            builder.appendff("||{}^\n", StringView { d.data(), d.size() });
    }
    auto rules = builder.to_byte_string();
    auto result = Web::ContentBlocker::the().set_rules_from_bytes(rules.bytes());
    if (result.is_error()) {
        warnln("Kakangkung adblock: ContentBlocker set_rules_from_bytes failed: {}", result.error());
        return false;
    }
    return true;
#elif defined(KK_HAS_CONTENT_FILTER)
    Vector<String> patterns;
    if (g_enabled) {
        for (auto const& d : g_domains)
            patterns.append(MUST(String::from_utf8(StringView { d.data(), d.size() })));
    }
    auto result = Web::ContentFilter::the().set_patterns(patterns);
    if (result.is_error()) {
        warnln("Kakangkung adblock: ContentFilter set_patterns failed: {}", result.error());
        return false;
    }
    return true;
#else
    warnln("Kakangkung adblock: LibWeb ContentFilter/ContentBlocker headers missing");
    return false;
#endif
}

} // namespace

extern "C" {

int kk_lb_adblock_configure(int enabled, char const* allowlist_utf8)
{
    std::lock_guard lock(g_mu);
    g_enabled = enabled != 0;
    g_allowlist.clear();
    if (allowlist_utf8) {
        std::string blob = allowlist_utf8;
        std::string line;
        for (char c : blob) {
            if (c == '\n' || c == '\r') {
                while (!line.empty() && (line.back() == ' ' || line.back() == '\t'))
                    line.pop_back();
                if (!line.empty())
                    g_allowlist.push_back(line);
                line.clear();
            } else {
                line.push_back(c);
            }
        }
        if (!line.empty())
            g_allowlist.push_back(line);
    }
    // Per-site allowlist is enforced on navigate in Rust; empty patterns when globally off.
    return apply_locked() ? 1 : 0;
}

int kk_lb_adblock_enabled(void)
{
    std::lock_guard lock(g_mu);
    return g_enabled ? 1 : 0;
}

} // extern "C"
