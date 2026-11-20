/* kakangkung-ladybird-host — process that owns Ladybird (or stub) views for Kakangkung.
 *
 * Protocol: one JSON object per line on stdin; one JSON object per line on stdout.
 * See protocol.md. Exit when stdin closes or {"cmd":"quit"} is received.
 */

#include "kakangkung_ladybird.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <unordered_map>

namespace {

struct HostView {
    kk_lb_view* view {};
    int id {};
};

struct Host {
    kk_lb_session* session {};
    std::unordered_map<int, HostView> views;
    int next_id { 1 };
};

void emit(std::string const& json)
{
    std::cout << json << '\n';
    std::cout.flush();
}

void on_title(void* user, char const* title)
{
    auto* hv = static_cast<HostView*>(user);
    emit(std::string("{\"event\":\"title\",\"id\":") + std::to_string(hv->id) + ",\"title\":\"" + (title ? title : "") + "\"}");
}

void on_url(void* user, char const* url)
{
    auto* hv = static_cast<HostView*>(user);
    emit(std::string("{\"event\":\"url\",\"id\":") + std::to_string(hv->id) + ",\"url\":\"" + (url ? url : "") + "\"}");
}

void emit_history(HostView* hv)
{
    bool back = kk_lb_view_can_go_back(hv->view) != 0;
    bool forward = kk_lb_view_can_go_forward(hv->view) != 0;
    emit(std::string("{\"event\":\"history\",\"id\":") + std::to_string(hv->id)
        + ",\"back\":" + (back ? "true" : "false")
        + ",\"forward\":" + (forward ? "true" : "false") + "}");
}

void on_load(void* user, int done)
{
    auto* hv = static_cast<HostView*>(user);
    if (done) {
        emit_history(hv);
        // Content scripts may have queued chrome.runtime.sendMessage during inject.
        kk_lb_view_drain_ext(hv->view);
    }
    emit(std::string("{\"event\":\"load\",\"id\":") + std::to_string(hv->id) + ",\"done\":" + (done ? "true" : "false") + "}");
}

// Tiny helpers — enough for the line protocol without a JSON library.
bool field_int(std::string const& line, char const* key, int& out)
{
    auto needle = std::string("\"") + key + "\":";
    auto pos = line.find(needle);
    if (pos == std::string::npos)
        return false;
    pos += needle.size();
    out = std::atoi(line.c_str() + pos);
    return true;
}

bool field_double(std::string const& line, char const* key, double& out)
{
    auto needle = std::string("\"") + key + "\":";
    auto pos = line.find(needle);
    if (pos == std::string::npos)
        return false;
    pos += needle.size();
    out = std::atof(line.c_str() + pos);
    return true;
}

bool field_str(std::string const& line, char const* key, std::string& out)
{
    auto needle = std::string("\"") + key + "\":\"";
    auto pos = line.find(needle);
    if (pos == std::string::npos)
        return false;
    pos += needle.size();
    auto end = line.find('"', pos);
    if (end == std::string::npos)
        return false;
    out = line.substr(pos, end - pos);
    return true;
}

bool has_cmd(std::string const& line, char const* cmd)
{
    auto needle = std::string("\"cmd\":\"") + cmd + "\"";
    return line.find(needle) != std::string::npos;
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
        case '\r':
            out += "\\r";
            break;
        case '\t':
            out += "\\t";
            break;
        default:
            out += c;
            break;
        }
    }
    return out;
}

void on_ext_message(void* user, char const* extension_id, char const* request_id, char const* message_json)
{
    auto* hv = static_cast<HostView*>(user);
    emit(std::string("{\"event\":\"ext_message\",\"id\":") + std::to_string(hv->id)
        + ",\"extension\":\"" + json_escape(extension_id ? extension_id : "") + "\""
        + ",\"requestId\":\"" + json_escape(request_id ? request_id : "") + "\""
        + ",\"message\":" + (message_json && message_json[0] ? message_json : "null") + "}");
}

void on_page_ipc(void* user, char const* message)
{
    auto* hv = static_cast<HostView*>(user);
    emit(std::string("{\"event\":\"page_ipc\",\"id\":") + std::to_string(hv->id)
        + ",\"message\":\"" + json_escape(message ? message : "") + "\"}");
}

std::string extract_raw_json_field(std::string const& line, char const* key)
{
    auto needle = std::string("\"") + key + "\":";
    auto pos = line.find(needle);
    if (pos == std::string::npos)
        return {};
    pos += needle.size();
    while (pos < line.size() && (line[pos] == ' ' || line[pos] == '\t'))
        ++pos;
    if (pos >= line.size())
        return {};
    if (line[pos] == '"') {
        size_t i = pos + 1;
        std::string out = "\"";
        while (i < line.size()) {
            if (line[i] == '\\' && i + 1 < line.size()) {
                out += line[i];
                out += line[i + 1];
                i += 2;
                continue;
            }
            out += line[i];
            if (line[i] == '"')
                break;
            ++i;
        }
        return out;
    }
    if (line[pos] == '{' || line[pos] == '[') {
        char open = line[pos];
        char close = open == '{' ? '}' : ']';
        int depth = 0;
        size_t i = pos;
        do {
            if (line[i] == open)
                ++depth;
            else if (line[i] == close)
                --depth;
            ++i;
        } while (i < line.size() && depth > 0);
        return line.substr(pos, i - pos);
    }
    size_t end = pos;
    while (end < line.size() && line[end] != ',' && line[end] != '}')
        ++end;
    return line.substr(pos, end - pos);
}

} // namespace

int main(int argc, char** argv)
{
    std::string profile;
    uintptr_t parent = 0;
    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], "--profile") == 0 && i + 1 < argc)
            profile = argv[++i];
        else if (std::strcmp(argv[i], "--parent") == 0 && i + 1 < argc)
            parent = static_cast<uintptr_t>(std::strtoull(argv[++i], nullptr, 0));
        else if (std::strcmp(argv[i], "--version") == 0) {
            std::printf("kakangkung-ladybird-host engine=%s\n", kk_lb_engine_id());
            return 0;
        }
    }

    Host host;
    host.session = kk_lb_session_create(profile.c_str());
    kk_lb_set_download_emit([](char const* json_line) {
        if (json_line)
            emit(json_line);
    });
    emit(std::string("{\"event\":\"ready\",\"engine\":\"") + kk_lb_engine_id() + "\"}");

    std::string line;
    while (std::getline(std::cin, line)) {
        if (line.empty())
            continue;
        if (has_cmd(line, "quit"))
            break;
        if (has_cmd(line, "create")) {
            int id = host.next_id++;
            auto& stored = host.views[id];
            stored.id = id;
            kk_lb_callbacks cb {};
            cb.user = &stored;
            cb.on_title = on_title;
            cb.on_url = on_url;
            cb.on_load = on_load;
            cb.on_ext_message = on_ext_message;
            cb.on_page_ipc = on_page_ipc;
            int x = 0, y = 0, w = 800, h = 600;
            field_int(line, "x", x);
            field_int(line, "y", y);
            field_int(line, "w", w);
            field_int(line, "h", h);
            stored.view = kk_lb_view_create(host.session, parent, cb);
            kk_lb_view_set_bounds(stored.view, x, y, w, h);
            emit(std::string("{\"event\":\"created\",\"id\":") + std::to_string(stored.id) + "}");
            std::string url;
            if (field_str(line, "url", url))
                kk_lb_view_navigate(stored.view, url.c_str());
            continue;
        }
        if (has_cmd(line, "navigate")) {
            int id = 0;
            std::string url;
            if (field_int(line, "id", id) && field_str(line, "url", url)) {
                if (auto it = host.views.find(id); it != host.views.end())
                    kk_lb_view_navigate(it->second.view, url.c_str());
            }
            continue;
        }
        if (has_cmd(line, "destroy")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                if (auto it = host.views.find(id); it != host.views.end()) {
                    kk_lb_view_destroy(it->second.view);
                    host.views.erase(it);
                }
            }
            continue;
        }
        if (has_cmd(line, "bounds")) {
            int id = 0, x = 0, y = 0, w = 0, h = 0;
            if (field_int(line, "id", id)) {
                field_int(line, "x", x);
                field_int(line, "y", y);
                field_int(line, "w", w);
                field_int(line, "h", h);
                if (auto it = host.views.find(id); it != host.views.end())
                    kk_lb_view_set_bounds(it->second.view, x, y, w, h);
            }
            continue;
        }
        if (has_cmd(line, "reload")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                if (auto it = host.views.find(id); it != host.views.end())
                    kk_lb_view_reload(it->second.view);
            }
            continue;
        }
        if (has_cmd(line, "stop")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                if (auto it = host.views.find(id); it != host.views.end())
                    kk_lb_view_stop(it->second.view);
            }
            continue;
        }
        if (has_cmd(line, "back")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                if (auto it = host.views.find(id); it != host.views.end())
                    kk_lb_view_go_back(it->second.view);
            }
            continue;
        }
        if (has_cmd(line, "forward")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                if (auto it = host.views.find(id); it != host.views.end())
                    kk_lb_view_go_forward(it->second.view);
            }
            continue;
        }
        if (has_cmd(line, "visible")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                bool on = line.find("\"visible\":true") != std::string::npos
                    || line.find("\"visible\":1") != std::string::npos;
                if (auto it = host.views.find(id); it != host.views.end())
                    kk_lb_view_set_visible(it->second.view, on ? 1 : 0);
            }
            continue;
        }
        if (has_cmd(line, "screenshot")) {
            int id = 0;
            std::string path;
            if (field_int(line, "id", id)) {
                field_str(line, "path", path);
                bool ok = false;
                if (auto it = host.views.find(id); it != host.views.end())
                    ok = kk_lb_view_screenshot(it->second.view, path.c_str()) != 0;
                emit(std::string("{\"event\":\"screenshot\",\"id\":") + std::to_string(id)
                    + ",\"path\":\"" + json_escape(path) + "\",\"ok\":" + (ok ? "true" : "false")
                    + (ok ? "" : ",\"reason\":\"screenshot failed\"") + "}");
            }
            continue;
        }
        if (has_cmd(line, "print")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                bool ok = false;
                if (auto it = host.views.find(id); it != host.views.end())
                    ok = kk_lb_view_print(it->second.view) != 0;
                emit(std::string("{\"event\":\"print\",\"id\":") + std::to_string(id)
                    + ",\"ok\":" + (ok ? "true" : "false")
                    + (ok ? "" : ",\"reason\":\"print unavailable\"") + "}");
            }
            continue;
        }
        if (has_cmd(line, "adblock")) {
            bool enabled = line.find("\"enabled\":false") == std::string::npos
                && line.find("\"enabled\":0") == std::string::npos;
            std::string allowlist;
            field_str(line, "allowlist", allowlist);
            // allowlist may contain escaped newlines as \n in JSON string — accept both.
            for (size_t i = 0; i + 1 < allowlist.size(); ++i) {
                if (allowlist[i] == '\\' && allowlist[i + 1] == 'n') {
                    allowlist.replace(i, 2, "\n");
                }
            }
            bool ok = kk_lb_adblock_configure(enabled ? 1 : 0, allowlist.c_str()) != 0;
            emit(std::string("{\"event\":\"adblock\",\"ok\":") + (ok ? "true" : "false")
                + ",\"enabled\":" + (enabled ? "true" : "false")
                + (ok ? "" : ",\"reason\":\"content filter unavailable\"") + "}");
            continue;
        }
        if (has_cmd(line, "zoom")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                double relative = 0.0;
                double absolute = 0.0;
                bool has_relative = field_double(line, "factor", relative);
                bool has_absolute = field_double(line, "absolute", absolute);
                double out = 1.0;
                bool ok = false;
                if (auto it = host.views.find(id); it != host.views.end()) {
                    ok = kk_lb_view_zoom(it->second.view, has_relative ? &relative : nullptr,
                            has_absolute ? &absolute : nullptr, &out)
                        != 0;
                }
                char factor_buf[64];
                std::snprintf(factor_buf, sizeof(factor_buf), "%.6g", out);
                emit(std::string("{\"event\":\"zoom\",\"id\":") + std::to_string(id)
                    + ",\"ok\":" + (ok ? "true" : "false") + ",\"factor\":" + factor_buf
                    + (ok ? "" : ",\"reason\":\"zoom unavailable\"") + "}");
            }
            continue;
        }
        if (has_cmd(line, "a11y")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                char* tree = nullptr;
                bool ok = false;
                if (auto it = host.views.find(id); it != host.views.end())
                    ok = kk_lb_view_a11y_json(it->second.view, &tree) != 0;
                if (ok && tree) {
                    emit(std::string("{\"event\":\"a11y\",\"id\":") + std::to_string(id)
                        + ",\"tree\":" + tree + "}");
                    kk_lb_string_free(tree);
                } else {
                    emit(std::string("{\"event\":\"a11y\",\"id\":") + std::to_string(id)
                        + ",\"tree\":null,\"ok\":false,\"reason\":\"a11y unavailable\"}");
                }
            }
            continue;
        }
        if (has_cmd(line, "dom")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                char* tree = nullptr;
                bool ok = false;
                if (auto it = host.views.find(id); it != host.views.end())
                    ok = kk_lb_view_dom_json(it->second.view, &tree) != 0;
                if (ok && tree) {
                    emit(std::string("{\"event\":\"dom\",\"id\":") + std::to_string(id)
                        + ",\"ok\":true,\"tree\":" + tree + "}");
                    kk_lb_string_free(tree);
                } else {
                    emit(std::string("{\"event\":\"dom\",\"id\":") + std::to_string(id)
                        + ",\"tree\":null,\"ok\":false,\"reason\":\"dom unavailable\"}");
                }
            }
            continue;
        }
        if (has_cmd(line, "console")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                bool clear = line.find("\"clear\":true") != std::string::npos
                    || line.find("\"clear\":1") != std::string::npos;
                char* messages = nullptr;
                bool ok = false;
                if (auto it = host.views.find(id); it != host.views.end())
                    ok = kk_lb_view_console_json(it->second.view, &messages, clear ? 1 : 0) != 0;
                if (ok && messages) {
                    emit(std::string("{\"event\":\"console\",\"id\":") + std::to_string(id)
                        + ",\"ok\":true,\"messages\":" + messages + "}");
                    kk_lb_string_free(messages);
                } else {
                    emit(std::string("{\"event\":\"console\",\"id\":") + std::to_string(id)
                        + ",\"messages\":null,\"ok\":false,\"reason\":\"console unavailable\"}");
                }
            }
            continue;
        }
        if (has_cmd(line, "eval")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                // Prefer raw JSON string field so escaped quotes / newlines in scripts survive.
                std::string js = extract_raw_json_field(line, "js");
                if (js.size() >= 2 && js.front() == '"' && js.back() == '"') {
                    js = js.substr(1, js.size() - 2);
                    std::string unesc;
                    for (size_t i = 0; i < js.size(); ++i) {
                        if (js[i] == '\\' && i + 1 < js.size()) {
                            char n = js[++i];
                            if (n == 'n')
                                unesc += '\n';
                            else if (n == 'r')
                                unesc += '\r';
                            else if (n == 't')
                                unesc += '\t';
                            else
                                unesc += n;
                        } else {
                            unesc += js[i];
                        }
                    }
                    js = std::move(unesc);
                } else if (!field_str(line, "js", js)) {
                    js.clear();
                }
                char* result = nullptr;
                bool ok = false;
                if (auto it = host.views.find(id); it != host.views.end())
                    ok = kk_lb_view_eval(it->second.view, js.c_str(), &result) != 0;
                emit(std::string("{\"event\":\"eval\",\"id\":") + std::to_string(id)
                    + ",\"ok\":" + (ok ? "true" : "false")
                    + ",\"result\":\"" + json_escape(result ? result : "") + "\"}");
                kk_lb_string_free(result);
            }
            continue;
        }
        if (has_cmd(line, "capabilities")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                char* caps = nullptr;
                bool ok = false;
                if (auto it = host.views.find(id); it != host.views.end())
                    ok = kk_lb_view_capabilities(it->second.view, &caps) != 0;
                if (ok && caps) {
                    emit(std::string("{\"event\":\"capabilities\",\"id\":") + std::to_string(id)
                        + ",\"ok\":true,\"caps\":" + caps + "}");
                    kk_lb_string_free(caps);
                } else {
                    emit(std::string("{\"event\":\"capabilities\",\"id\":") + std::to_string(id)
                        + ",\"ok\":false,\"caps\":null,\"reason\":\"capabilities unavailable\"}");
                }
            }
            continue;
        }
        if (has_cmd(line, "ext_reply")) {
            int id = 0;
            std::string extension;
            std::string request_id;
            if (field_int(line, "id", id) && field_str(line, "extension", extension) && field_str(line, "requestId", request_id)) {
                auto response = extract_raw_json_field(line, "response");
                bool ok = false;
                if (auto it = host.views.find(id); it != host.views.end())
                    ok = kk_lb_view_ext_reply(it->second.view, extension.c_str(), request_id.c_str(), response.c_str()) != 0;
                emit(std::string("{\"event\":\"ext_reply\",\"id\":") + std::to_string(id)
                    + ",\"ok\":" + (ok ? "true" : "false") + "}");
            }
            continue;
        }
        if (has_cmd(line, "ext_drain")) {
            int id = 0;
            if (field_int(line, "id", id)) {
                if (auto it = host.views.find(id); it != host.views.end())
                    kk_lb_view_drain_ext(it->second.view);
            }
            continue;
        }
        if (has_cmd(line, "download")) {
            kk_lb_downloads_ensure_observer();
            std::string url;
            std::string path;
            field_str(line, "url", url);
            field_str(line, "path", path);
            bool ok = !url.empty() && !path.empty() && kk_lb_download_start(url.c_str(), path.c_str()) != 0;
            emit(std::string("{\"event\":\"download_started\",\"ok\":") + (ok ? "true" : "false")
                + ",\"url\":\"" + json_escape(url) + "\",\"path\":\"" + json_escape(path) + "\"}");
            continue;
        }
        if (has_cmd(line, "download_action")) {
            int id = 0;
            std::string action;
            field_int(line, "downloadId", id);
            if (id == 0)
                field_int(line, "id", id);
            field_str(line, "action", action);
            bool ok = id > 0 && !action.empty()
                && kk_lb_download_action(static_cast<uint64_t>(id), action.c_str()) != 0;
            emit(std::string("{\"event\":\"download_action\",\"downloadId\":") + std::to_string(id)
                + ",\"action\":\"" + json_escape(action) + "\",\"ok\":" + (ok ? "true" : "false") + "}");
            continue;
        }
        kk_lb_session_pump(host.session);
    }

    for (auto& [_, hv] : host.views)
        kk_lb_view_destroy(hv.view);
    host.views.clear();
    kk_lb_session_destroy(host.session);
    return 0;
}
