#pragma once

#include <string>
#include <vector>

namespace kakangkung::ladybird {

struct ContentScript {
    std::string extension_id;
    std::vector<std::string> matches;
    std::vector<std::string> js_sources; // absolute paths or already-read source
    std::string run_at;                 // document_start / document_end / document_idle
};

/// Load enabled MV3 content_scripts from `<profile>/extensions/*/manifest.json`.
std::vector<ContentScript> load_content_scripts(std::string const& profile_dir);

/// True when `url` matches a Chrome match pattern (supports `*://` / `<all_urls>` / host wildcards).
bool url_matches_pattern(std::string const& url, std::string const& pattern);

/// Bootstrap + matched content scripts to inject after a navigation (real JS sources from disk).
std::string chrome_runtime_bootstrap(std::string const& extension_id);
std::vector<std::string> scripts_for_url(std::vector<ContentScript> const& all, std::string const& url);

} // namespace kakangkung::ladybird
