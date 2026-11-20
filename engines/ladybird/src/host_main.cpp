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

void on_load(void* user, int done)
{
    auto* hv = static_cast<HostView*>(user);
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
        kk_lb_session_pump(host.session);
    }

    for (auto& [_, hv] : host.views)
        kk_lb_view_destroy(hv.view);
    host.views.clear();
    kk_lb_session_destroy(host.session);
    return 0;
}
