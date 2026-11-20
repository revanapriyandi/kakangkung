#pragma once

#include <string>
#include <vector>

namespace kakangkung::ladybird {

struct ContentScript {
    std::string extension_id;
    std::string extension_dir;
    std::string manifest_json; // raw manifest for chrome.runtime.getManifest()
    std::vector<std::string> matches;
    std::vector<std::string> exclude_matches;
    std::vector<std::string> js_sources;
    std::vector<std::string> css_sources;
    std::string run_at; // document_start / document_end / document_idle
};

/// Load enabled MV3 content_scripts from `<profile>/extensions/*/manifest.json`.
std::vector<ContentScript> load_content_scripts(std::string const& profile_dir);

/// True when `url` matches a Chrome match pattern (supports `*://` / `<all_urls>` / host wildcards).
bool url_matches_pattern(std::string const& url, std::string const& pattern);

/// True when any `matches` hits and no `exclude_matches` hits.
bool content_script_matches_url(ContentScript const& cs, std::string const& url);

/// Bootstrap chrome.runtime + chrome.storage.local (file-backed in the extension profile dir).
std::string chrome_runtime_bootstrap(std::string const& extension_id, std::string const& manifest_json);

/// Read/merge chrome.storage.local JSON under `<profile>/extensions/<id>/local-storage.json`.
/// `message_json` is the get keys object / null / array, or the set items object.
std::string chrome_storage_get(std::string const& profile_dir, std::string const& extension_id,
    std::string const& message_json);
std::string chrome_storage_set(std::string const& profile_dir, std::string const& extension_id,
    std::string const& message_json);
std::string chrome_storage_remove(std::string const& profile_dir, std::string const& extension_id,
    std::string const& message_json);
std::string chrome_storage_clear(std::string const& profile_dir, std::string const& extension_id);

/// CSS injector wrapping stylesheet text.
std::string css_inject_script(std::string const& css_text);

/// Matched content scripts for `url` filtered by `run_at` (empty = all timings).
std::vector<std::string> scripts_for_url(std::vector<ContentScript> const& all, std::string const& url,
    std::string const& run_at_filter = {});

/// Parse a `kk-ext:…` console payload. Returns false if not a bridge message.
bool parse_kk_ext_payload(std::string const& console_text, std::string& extension_id, std::string& type,
    std::string& request_id, std::string& message_json);

/// JS that resolves a pending sendMessage callback in the page.
std::string chrome_runtime_reply_script(std::string const& extension_id, std::string const& request_id,
    std::string const& response_json);

/// JS that delivers a message to onMessage listeners in the page.
std::string chrome_runtime_deliver_script(std::string const& extension_id, std::string const& message_json);

} // namespace kakangkung::ladybird
