#include "extensions.h"

#include <filesystem>
#include <fstream>
#include <iomanip>
#include <sstream>

namespace kakangkung::ladybird {
namespace {

namespace fs = std::filesystem;

std::string read_file(fs::path const& path)
{
    std::ifstream in(path, std::ios::binary);
    if (!in)
        return {};
    std::ostringstream ss;
    ss << in.rdbuf();
    return ss.str();
}

// Tiny helpers to pull JSON string arrays / string fields without a full parser.
std::string json_string_at(std::string const& text, size_t& i)
{
    while (i < text.size() && (text[i] == ' ' || text[i] == '\n' || text[i] == '\r' || text[i] == '\t'))
        ++i;
    if (i >= text.size() || text[i] != '"')
        return {};
    ++i;
    std::string out;
    while (i < text.size() && text[i] != '"') {
        if (text[i] == '\\' && i + 1 < text.size()) {
            out += text[i + 1];
            i += 2;
            continue;
        }
        out += text[i++];
    }
    if (i < text.size() && text[i] == '"')
        ++i;
    return out;
}

std::vector<std::string> json_string_array_after(std::string const& text, char const* key)
{
    auto needle = std::string("\"") + key + "\"";
    auto pos = text.find(needle);
    if (pos == std::string::npos)
        return {};
    pos = text.find('[', pos);
    if (pos == std::string::npos)
        return {};
    size_t i = pos + 1;
    std::vector<std::string> out;
    while (i < text.size() && text[i] != ']') {
        if (text[i] == '"') {
            out.push_back(json_string_at(text, i));
            continue;
        }
        ++i;
    }
    return out;
}

std::string json_string_field(std::string const& text, char const* key)
{
    auto needle = std::string("\"") + key + "\"";
    auto pos = text.find(needle);
    if (pos == std::string::npos)
        return {};
    pos = text.find(':', pos);
    if (pos == std::string::npos)
        return {};
    size_t i = pos + 1;
    return json_string_at(text, i);
}

bool glob_match(std::string const& text, std::string const& pattern)
{
    size_t ti = 0, pi = 0, star = std::string::npos, restart = 0;
    while (ti < text.size()) {
        if (pi < pattern.size() && (pattern[pi] == '?' || pattern[pi] == text[ti])) {
            ++ti;
            ++pi;
        } else if (pi < pattern.size() && pattern[pi] == '*') {
            star = pi++;
            restart = ti;
        } else if (star != std::string::npos) {
            pi = star + 1;
            ti = ++restart;
        } else {
            return false;
        }
    }
    while (pi < pattern.size() && pattern[pi] == '*')
        ++pi;
    return pi == pattern.size();
}

} // namespace

bool url_matches_pattern(std::string const& url, std::string const& pattern)
{
    if (pattern == "<all_urls>")
        return url.find("://") != std::string::npos;
    // scheme://host/path — Chrome match patterns
    auto scheme_end = pattern.find("://");
    if (scheme_end == std::string::npos)
        return false;
    auto host_start = scheme_end + 3;
    auto path_start = pattern.find('/', host_start);
    std::string scheme = pattern.substr(0, scheme_end);
    std::string host = path_start == std::string::npos ? pattern.substr(host_start) : pattern.substr(host_start, path_start - host_start);
    std::string path = path_start == std::string::npos ? "/*" : pattern.substr(path_start);

    auto u_scheme_end = url.find("://");
    if (u_scheme_end == std::string::npos)
        return false;
    auto u_host_start = u_scheme_end + 3;
    auto u_path_start = url.find('/', u_host_start);
    std::string u_scheme = url.substr(0, u_scheme_end);
    std::string u_host = u_path_start == std::string::npos ? url.substr(u_host_start) : url.substr(u_host_start, u_path_start - u_host_start);
    // strip port
    if (auto colon = u_host.find(':'); colon != std::string::npos)
        u_host = u_host.substr(0, colon);
    std::string u_path = u_path_start == std::string::npos ? "/" : url.substr(u_path_start);

    if (scheme != "*" && scheme != u_scheme)
        return false;
    if (host == "*") {
        // ok
    } else if (host.size() >= 2 && host[0] == '*' && host[1] == '.') {
        auto suffix = host.substr(1); // ".example.com"
        if (u_host != host.substr(2) && (u_host.size() < suffix.size() || u_host.compare(u_host.size() - suffix.size(), suffix.size(), suffix) != 0))
            return false;
    } else if (host != u_host) {
        return false;
    }
    return glob_match(u_path, path);
}

std::vector<ContentScript> load_content_scripts(std::string const& profile_dir)
{
    std::vector<ContentScript> out;
    fs::path root = fs::path(profile_dir) / "extensions";
    std::error_code ec;
    if (!fs::is_directory(root, ec))
        return out;

    for (auto const& entry : fs::directory_iterator(root, ec)) {
        if (!entry.is_directory())
            continue;
        if (fs::exists(entry.path() / ".disabled"))
            continue;
        auto manifest_path = entry.path() / "manifest.json";
        auto text = read_file(manifest_path);
        if (text.empty())
            continue;
        auto id = entry.path().filename().string();

        auto cs_key = text.find("\"content_scripts\"");
        if (cs_key == std::string::npos)
            continue;
        auto arr = text.find('[', cs_key);
        if (arr == std::string::npos)
            continue;
        // Walk objects inside the array by locating '{' … '}'.
        size_t i = arr + 1;
        while (i < text.size()) {
            while (i < text.size() && text[i] != '{' && text[i] != ']')
                ++i;
            if (i >= text.size() || text[i] == ']')
                break;
            size_t start = i;
            int depth = 0;
            do {
                if (text[i] == '{')
                    ++depth;
                else if (text[i] == '}')
                    --depth;
                ++i;
            } while (i < text.size() && depth > 0);
            auto block = text.substr(start, i - start);
            ContentScript cs;
            cs.extension_id = id;
            cs.matches = json_string_array_after(block, "matches");
            cs.run_at = json_string_field(block, "run_at");
            if (cs.run_at.empty())
                cs.run_at = "document_idle";
            auto js_files = json_string_array_after(block, "js");
            for (auto const& rel : js_files) {
                auto path = entry.path() / rel;
                auto src = read_file(path);
                if (!src.empty())
                    cs.js_sources.push_back(std::move(src));
            }
            if (!cs.matches.empty() && !cs.js_sources.empty())
                out.push_back(std::move(cs));
        }
    }
    return out;
}

std::string chrome_runtime_bootstrap(std::string const& extension_id)
{
    // Real MV3 surface in the page: chrome.runtime + messaging bridge to the host via console.
    std::ostringstream js;
    js << "(function(){\n"
       << "if (window.chrome && window.chrome.runtime && window.chrome.runtime.__kk) return;\n"
       << "const id = " << std::quoted(extension_id) << ";\n"
       << "const listeners = [];\n"
       << "const chromeObj = window.chrome || (window.chrome = {});\n"
       << "chromeObj.runtime = {\n"
       << "  __kk: true,\n"
       << "  id,\n"
       << "  getManifest() { return { manifest_version: 3, name: id }; },\n"
       << "  getURL(path) { return 'kk-extension://' + id + '/' + String(path||'').replace(/^\\/+/, ''); },\n"
       << "  sendMessage(message, response) {\n"
       << "    const req = 'kk-ext:' + JSON.stringify({ id, type: 'sendMessage', message });\n"
       << "    console.log(req);\n"
       << "    if (typeof response === 'function') setTimeout(() => response(undefined), 0);\n"
       << "  },\n"
       << "  onMessage: { addListener(fn) { listeners.push(fn); }, removeListener(fn) {\n"
       << "    const i = listeners.indexOf(fn); if (i >= 0) listeners.splice(i, 1); } },\n"
       << "  __deliver(message, sender) { for (const fn of listeners.slice()) try { fn(message, sender || { id }, ()=>{}); } catch(e) {} }\n"
       << "};\n"
       << "})();\n";
    return js.str();
}

std::vector<std::string> scripts_for_url(std::vector<ContentScript> const& all, std::string const& url)
{
    std::vector<std::string> out;
    for (auto const& cs : all) {
        bool match = false;
        for (auto const& p : cs.matches) {
            if (url_matches_pattern(url, p)) {
                match = true;
                break;
            }
        }
        if (!match)
            continue;
        out.push_back(chrome_runtime_bootstrap(cs.extension_id));
        for (auto const& src : cs.js_sources)
            out.push_back(src);
    }
    return out;
}

} // namespace kakangkung::ladybird
