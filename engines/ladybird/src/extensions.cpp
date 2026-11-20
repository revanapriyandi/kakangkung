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

std::string js_string_literal(std::string const& s)
{
    // std::quoted does not escape newlines/controls — those break JS string literals.
    std::ostringstream out;
    out << '"';
    for (unsigned char c : s) {
        switch (c) {
        case '\\':
            out << "\\\\";
            break;
        case '"':
            out << "\\\"";
            break;
        case '\n':
            out << "\\n";
            break;
        case '\r':
            out << "\\r";
            break;
        case '\t':
            out << "\\t";
            break;
        default:
            if (c < 0x20) {
                out << "\\u" << std::hex << std::setw(4) << std::setfill('0') << static_cast<int>(c) << std::dec;
            } else {
                out << static_cast<char>(c);
            }
            break;
        }
    }
    out << '"';
    return out.str();
}

/// Compact manifest JSON for embedding (strip newlines to keep the bootstrap one line-safe).
std::string compact_json(std::string text)
{
    std::string out;
    out.reserve(text.size());
    bool in_string = false;
    for (size_t i = 0; i < text.size(); ++i) {
        char c = text[i];
        if (in_string) {
            if (c == '\\' && i + 1 < text.size()) {
                out += c;
                out += text[++i];
                continue;
            }
            if (c == '"')
                in_string = false;
            out += c;
            continue;
        }
        if (c == '"') {
            in_string = true;
            out += c;
            continue;
        }
        if (c == ' ' || c == '\n' || c == '\r' || c == '\t')
            continue;
        out += c;
    }
    return out.empty() ? "{}" : out;
}

std::string extract_json_object_field(std::string const& text, char const* key)
{
    auto needle = std::string("\"") + key + "\":";
    auto pos = text.find(needle);
    if (pos == std::string::npos)
        return {};
    pos += needle.size();
    while (pos < text.size() && (text[pos] == ' ' || text[pos] == '\t'))
        ++pos;
    if (pos >= text.size() || text[pos] != '{')
        return {};
    size_t i = pos;
    int depth = 0;
    do {
        if (text[i] == '{')
            ++depth;
        else if (text[i] == '}')
            --depth;
        ++i;
    } while (i < text.size() && depth > 0);
    return text.substr(pos, i - pos);
}

bool extract_json_string_field(std::string const& obj, char const* key, std::string& out)
{
    auto needle = std::string("\"") + key + "\":";
    auto pos = obj.find(needle);
    if (pos == std::string::npos)
        return false;
    size_t i = pos + needle.size();
    out = json_string_at(obj, i);
    return true;
}

} // namespace

bool url_matches_pattern(std::string const& url, std::string const& pattern)
{
    if (pattern == "<all_urls>")
        return url.find("://") != std::string::npos;
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
    if (auto colon = u_host.find(':'); colon != std::string::npos)
        u_host = u_host.substr(0, colon);
    std::string u_path = u_path_start == std::string::npos ? "/" : url.substr(u_path_start);

    if (scheme != "*" && scheme != u_scheme)
        return false;
    if (host == "*") {
        // ok
    } else if (host.size() >= 2 && host[0] == '*' && host[1] == '.') {
        auto suffix = host.substr(1);
        if (u_host != host.substr(2) && (u_host.size() < suffix.size() || u_host.compare(u_host.size() - suffix.size(), suffix.size(), suffix) != 0))
            return false;
    } else if (host != u_host) {
        return false;
    }
    return glob_match(u_path, path);
}

bool content_script_matches_url(ContentScript const& cs, std::string const& url)
{
    bool match = false;
    for (auto const& p : cs.matches) {
        if (url_matches_pattern(url, p)) {
            match = true;
            break;
        }
    }
    if (!match)
        return false;
    for (auto const& p : cs.exclude_matches) {
        if (url_matches_pattern(url, p))
            return false;
    }
    return true;
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
        auto manifest_compact = compact_json(text);

        auto cs_key = text.find("\"content_scripts\"");
        if (cs_key == std::string::npos)
            continue;
        auto arr = text.find('[', cs_key);
        if (arr == std::string::npos)
            continue;
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
            cs.extension_dir = entry.path().string();
            cs.manifest_json = manifest_compact;
            cs.matches = json_string_array_after(block, "matches");
            cs.exclude_matches = json_string_array_after(block, "exclude_matches");
            cs.run_at = json_string_field(block, "run_at");
            if (cs.run_at.empty())
                cs.run_at = "document_idle";
            for (auto const& rel : json_string_array_after(block, "js")) {
                auto src = read_file(entry.path() / rel);
                if (!src.empty())
                    cs.js_sources.push_back(std::move(src));
            }
            for (auto const& rel : json_string_array_after(block, "css")) {
                auto src = read_file(entry.path() / rel);
                if (!src.empty())
                    cs.css_sources.push_back(std::move(src));
            }
            if (!cs.matches.empty() && (!cs.js_sources.empty() || !cs.css_sources.empty()))
                out.push_back(std::move(cs));
        }
    }
    return out;
}

std::string chrome_runtime_bootstrap(std::string const& extension_id, std::string const& manifest_json)
{
    std::ostringstream js;
    js << "(function(){\n"
       << "if (window.chrome && window.chrome.runtime && window.chrome.runtime.__kk) return;\n"
       << "const id = " << js_string_literal(extension_id) << ";\n"
       << "const manifest = " << (manifest_json.empty() ? std::string("{}") : manifest_json) << ";\n"
       << "const listeners = [];\n"
       << "const pending = new Map();\n"
       << "let nextReq = 1;\n"
       << "const chromeObj = window.chrome || (window.chrome = {});\n"
       << "chromeObj.runtime = {\n"
       << "  __kk: true,\n"
       << "  id,\n"
       << "  getManifest() { return JSON.parse(JSON.stringify(manifest)); },\n"
       << "  getURL(path) { return 'kk-extension://' + id + '/' + String(path||'').replace(/^\\/+/, ''); },\n"
       << "  sendMessage(message, response) {\n"
       << "    const requestId = String(nextReq++);\n"
       << "    if (typeof response === 'function') pending.set(requestId, response);\n"
       << "    const entry = { id, type: 'sendMessage', requestId, message };\n"
       << "    (window.__kk_ext_queue = window.__kk_ext_queue || []).push(entry);\n"
       << "    try { console.log('kk-ext:' + JSON.stringify(entry)); } catch (e) {}\n"
       << "    setTimeout(() => {\n"
       << "      if (!pending.has(requestId)) return;\n"
       << "      const cb = pending.get(requestId); pending.delete(requestId);\n"
       << "      try { cb(undefined); } catch (e) {}\n"
       << "    }, 8000);\n"
       << "  },\n"
       << "  onMessage: { addListener(fn) { listeners.push(fn); }, removeListener(fn) {\n"
       << "    const i = listeners.indexOf(fn); if (i >= 0) listeners.splice(i, 1); } },\n"
       << "  __deliver(message, sender) {\n"
       << "    for (const fn of listeners.slice()) try { fn(message, sender || { id }, ()=>{}); } catch(e) {}\n"
       << "  },\n"
       << "  __resolve(requestId, response) {\n"
       << "    const cb = pending.get(String(requestId));\n"
       << "    if (!cb) return;\n"
       << "    pending.delete(String(requestId));\n"
       << "    try { cb(response); } catch (e) {}\n"
       << "  }\n"
       << "};\n"
       << "})();\n";
    return js.str();
}

std::string css_inject_script(std::string const& css_text)
{
    std::ostringstream js;
    js << "(function(){\n"
       << "var s = document.createElement('style');\n"
       << "s.setAttribute('data-kk-ext','1');\n"
       << "s.textContent = " << js_string_literal(css_text) << ";\n"
       << "var root = document.documentElement || document.head || document.body;\n"
       << "if (root) root.appendChild(s);\n"
       << "})();\n";
    return js.str();
}

std::vector<std::string> scripts_for_url(std::vector<ContentScript> const& all, std::string const& url,
    std::string const& run_at_filter)
{
    std::vector<std::string> out;
    for (auto const& cs : all) {
        if (!run_at_filter.empty() && cs.run_at != run_at_filter)
            continue;
        if (!content_script_matches_url(cs, url))
            continue;
        out.push_back(chrome_runtime_bootstrap(cs.extension_id, cs.manifest_json));
        for (auto const& css : cs.css_sources)
            out.push_back(css_inject_script(css));
        for (auto const& src : cs.js_sources)
            out.push_back(src);
    }
    return out;
}

bool parse_kk_ext_payload(std::string const& console_text, std::string& extension_id, std::string& type,
    std::string& request_id, std::string& message_json)
{
    static constexpr char const* kPrefix = "kk-ext:";
    if (console_text.rfind(kPrefix, 0) != 0)
        return false;
    auto body = console_text.substr(7);
    if (!extract_json_string_field(body, "id", extension_id))
        return false;
    if (!extract_json_string_field(body, "type", type))
        return false;
    extract_json_string_field(body, "requestId", request_id);
    message_json = extract_json_object_field(body, "message");
    if (message_json.empty()) {
        // message may be a string / number / array — keep raw after "message":
        auto needle = std::string("\"message\"");
        auto pos = body.find(needle);
        if (pos != std::string::npos) {
            pos = body.find(':', pos);
            if (pos != std::string::npos) {
                size_t i = pos + 1;
                while (i < body.size() && (body[i] == ' ' || body[i] == '\t'))
                    ++i;
                if (i < body.size() && body[i] == '"') {
                    auto s = json_string_at(body, i);
                    message_json = js_string_literal(s);
                } else {
                    size_t end = i;
                    while (end < body.size() && body[end] != ',' && body[end] != '}')
                        ++end;
                    message_json = body.substr(i, end - i);
                }
            }
        }
    }
    if (message_json.empty())
        message_json = "null";
    return true;
}

std::string chrome_runtime_reply_script(std::string const& extension_id, std::string const& request_id,
    std::string const& response_json)
{
    std::ostringstream js;
    js << "(function(){\n"
       << "var r = window.chrome && window.chrome.runtime;\n"
       << "if (!r || r.id !== " << js_string_literal(extension_id) << " || !r.__resolve) return;\n"
       << "r.__resolve(" << js_string_literal(request_id) << ", " << (response_json.empty() ? "undefined" : response_json)
       << ");\n"
       << "})();\n";
    return js.str();
}

std::string chrome_runtime_deliver_script(std::string const& extension_id, std::string const& message_json)
{
    std::ostringstream js;
    js << "(function(){\n"
       << "var r = window.chrome && window.chrome.runtime;\n"
       << "if (!r || r.id !== " << js_string_literal(extension_id) << " || !r.__deliver) return;\n"
       << "r.__deliver(" << (message_json.empty() ? "null" : message_json) << ", { id: " << js_string_literal(extension_id)
       << " });\n"
       << "})();\n";
    return js.str();
}

} // namespace kakangkung::ladybird
