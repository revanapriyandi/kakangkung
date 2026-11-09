# Chrome feature investigation and implementation

Checked on 2026-11-08. Kakangkung is a Rust/Win32/Direct2D browser application using
WebView2 for websites. Chromium's browser-side services are not automatically part
of an embedded WebView2 application. Keeping the native interface and lazy tabs
avoids adding an extra web renderer for the browser interface.

## Implementation and remaining boundaries

| Area | Implemented in this change | Remaining boundary |
| --- | --- | --- |
| Password manager | Standard URL-origin parsing; migration preserving old entries and conflicting ciphertexts; malformed CSV rejection; strict UTF-8 and 20 MiB input limit; new-password field protection; source-bound, sequenced save callbacks that survive redirects; stale fill checks | Form detection is a top-frame heuristic. A disappearing form or pagehide does not prove login success. Native Windows Hello and real-site login have not been exercised in this investigation. |
| Google account | External desktop OAuth browser when Kakangkung is default; atomic account updates; account-bound mirror caches; display name/photo refresh at startup and hourly, including with data sync off; native cached avatar | Requires the application's own Desktop OAuth client and Drive API configuration. Actual Google consent, profile refresh against a personal account, token refresh and two-device delivery remain unverified. |
| Kakangkung cloud sync | Drive pagination, session-change checks, bounded device files, and preserving the prior snapshot if a new one is oversized | This is Kakangkung's own Drive appDataFolder data. It is not the Google Chrome Sync service. Passwords are not included in this cloud-sync format. |
| Import/refresh from Chrome | SQLite backup snapshots include committed WAL changes; unchanged profiles skip parsing; incomplete bookmark reads are retried; automatic refresh does not read password stores | History/bookmarks are copied into Kakangkung. Changes and deletions are not written back to Chrome. Modern app-bound Chrome passwords require an explicit CSV export/import. |
| Many tabs | Retryable lifecycle after platform errors; resume on audio/media protection; reevaluate changed protection; clean old split pairs; cap recently closed tabs at 100; lazy background/restore tabs | Suspension preserves document state but does not guarantee a fixed RAM reduction. Automatic destructive discard remains disabled pending reliable unsaved-state handling. |
| Web Store install | Unique staging, key-to-requested-ID binding, consent before promotion, restore old files on engine rejection, stale-route and double-click protection, HRESULT diagnostics | HTTPS download/extraction and native loading of public uBlock Origin Lite are tested separately. This does not prove every extension's APIs work or reproduce the unspecified original notification. CRX signatures are not independently verified like Chrome's complete installer. |
| Downloads | Toolbar button appears on download activity, even when unpinned; repeated updates do not duplicate it | Visibility is retained for the session; the pinned preference is not changed. |
| Task manager | Union of app descendants and authoritative WebView2 environment process lists, PID deduplication and process-age guards | Windows can group the shared runtime separately. A renderer can be shared, so process memory is not an exact per-tab allocation. |
| DevTools | Real WebView2 inspector adopted in a bottom child viewport; clips Chromium's client-painted title bar; browser shortcut/menu and custom page Inspect route; cleanup on close/tab switch/failure | Native Win32 hosting workaround rather than a supported WebView2 docking API. Runtime/DPI changes can prevent docking. Right-click Inspect opens the panel without automatically selecting that DOM node. |
| Ad blocking | One shared adblock-rust network engine with bundled EasyList/EasyPrivacy, exact-host pause, toolbar counters, basic ad cosmetics and targeted YouTube player-ad JSON pruning | No claim of every ad blocked. Lists update with the app/manual bundle refresh. Top-level source approximates nested frame initiators; document requests pass through. Remote scriptlets, complete cosmetic syntax, response replacement and embedded creator sponsorships are not covered. |
| Tab hover cards | Native nonactivating window, title/site, bounded cached snapshots, memory measurement, no extra WebView and no wake on sleeping-tab hover | Runtime 156+ uses an explicitly labelled JS-isolate heap subset to avoid a native frame-attribution crash path. Images, video, GPU and the rest of renderer memory are excluded from that value. |

No Chrome OAuth identity, private account token, app-bound encryption bypass, or
private Chrome service is used. The public source informed behavior and boundaries;
the fixes remain in the existing Rust/JavaScript implementation.

## Evidence and verification

- A synthetic live-WAL regression failed before the import fix (zero imported rows
  instead of one), and passed after using SQLite's online backup API.
- Regression tests exercise account switching, paginated Drive listings, callback
  freshness, legacy password origins, import retries, UTF-8/CSV fidelity and lifecycle
  state transitions. Tests use fabricated data rather than personal browser stores.
- The native self-test also exercises actual WebView2 suspension and resumption,
  simulated audio-protection signals, and 100 lazy tabs without additional WebViews.
  `bench/native-parity-check.ps1` creates synthetic Chrome/Edge profiles and isolates
  the child process's LOCALAPPDATA and Kakangkung data paths.
- Chrome 157.0.8103.38 was exercised using REA against the local password fixture.
  All seven recorded navigation/action steps completed and the final DOM contained
  the expected save-offer signal. REA then returned `cleanup_incomplete` after its
  cleanup deadline, so this remains a partial observation, not a successful complete
  REA scenario or an audit of Chrome's password vault. A subsequent OS process
  inspection found no remaining matching headless Chrome process. The raw partial
  observation is in the workspace's `checks/chrome-parity/rea-chrome-fixture.json`.
- Native decompilation was not performed: the configured REA session reported that
  its Ghidra provider was not configured. The relevant implementation is available
  in public Chromium source, so no decompiler installation was needed.

Reproduce the checks from the project directory:

The latest unit run passed 121 Rust tests; one network-dependent store test is
ignored by default and was also run separately during the investigation.
Sixteen JavaScript regressions cover passwords, Web Store interaction and the
YouTube/helper boundaries. Native runtime results and public-site memory probes
are recorded separately; fixture success is not personal-account or all-site proof.

The release build and Clippy with warnings denied succeeded. The isolated native
run at `checks/chrome-parity/20261108-165204-447/selftest.json` in the parent
workspace passed all 30 report entries, including real uBlock Origin Lite native
loading/removal, a completed blob download and automatic toolbar button, native
profile-image decoding, real child-hosted DevTools, and a public YouTube video.
The synthetic Chrome profile contains no extensions, so import-from-Chrome's
extension entry explicitly reports a skip; the separate real store-package loader
did run. The YouTube probe advanced from 1.4 to 61.5 seconds, reported no media
error and no `ad-showing` player state across 13 samples. The helper counted 42
deleted player-ad metadata fields; this is a field count, not 42 blocked ads.
It does not establish coverage of every preroll, midroll, account or video.
Screenshots are retained alongside that report under `shots/`.

Ten Python tests also passed for the bounded filter-bundle maintenance CLI,
including rollback and preservation of license metadata. It does not automatically
update filters in an installed browser.

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
node --test assets/passwords.test.js assets/webstore.test.js assets/adblock.test.js
cargo build --release --locked
pwsh -NoProfile -File bench/native-parity-check.ps1
python bench/test-update-adblock-lists.py
```

For the optional real public-video smoke test, set `KAKANGKUNG_YOUTUBE_PROBE=1`
before the native parity command. It uses an isolated signed-out profile, attempts
muted playback and records 65 seconds of samples. It depends on YouTube/network
availability and is kept out of the default fixture check.

For the optional Chrome script fixture, run `node bench/password-fixture.mjs` and
open the printed loopback address in a separate test profile. It uses synthetic
credentials and a test IPC receiver; it tests Kakangkung's page script on Chromium,
not Chrome's own save-password UI or Google Password Manager service.

`bench/memory-probe-findings.md` records fresh-profile memory observations including
the reported YouTube/GitHub/Instagram/Facebook workload. It reproduces roughly
800 MiB before the new filters. These single-run public-page observations do not
establish a 100 MiB ceiling, authenticated-feed behavior or video playback by
themselves. Maximum (30 seconds) is now the default for new/reset profiles; saved
preferences are retained and protected media stays running.

The final quiet-machine pilot measured 756.63 MiB private working set for the
preserved old Balanced build, 721.00 MiB for the new Balanced build, and
716.07 MiB for the new Maximum build with the same four public URLs and YouTube
active after 45 seconds. This does not meet the requested approximately 100 MiB
total. Maximum versus new Balanced differed by only 0.68% in this workload.
The alternate ordering (GitHub selected, despite the historical raw scenario name `four-real-active-youtube`) measured 583.75 MiB and is not a matched
before/after comparison; playback was verified by the separate video probe.
The active-tab labels were corrected on 2026-11-09 after verifying that command-line
startup restores selection to the first URL. Recorded figures are unchanged;
historical memory runs did not record native per-tab visibility or suspension.
One simple `example.com` tab measured 124.09 MiB. All figures include the app and
its runtime process tree, use fresh signed-out profiles and are single-run
observations. See the probe findings for hashes, commit, CPU and raw data.

## Primary sources

The follow-up dated 2026-11-09, including muted-background playback, hover-card
verification and the unsuccessful approximately 100 MiB target, is documented in
[MEMORY_ROUND2.md](MEMORY_ROUND2.md).

- [Chromium Password Manager](https://chromium.googlesource.com/chromium/src/+/HEAD/components/password_manager/)
- [Chromium password CSV writer](https://github.com/chromium/chromium/blob/main/components/password_manager/core/browser/export/password_csv_writer.cc)
- [Chromium API keys and sign-in/Sync restrictions](https://www.chromium.org/developers/how-tos/api-keys/)
- [Google OAuth for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app)
- [Drive application data folder](https://developers.google.com/workspace/drive/api/guides/appdata)
- [Drive files.list pagination](https://developers.google.com/workspace/drive/api/reference/rest/v3/files/list)
- [WebView2 memory target](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_19)
- [SQLite Online Backup API](https://www.sqlite.org/backup.html)
- [WebView2 native DevTools API](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#opendevtoolswindow)
- [WebView2 context menu customization](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/context-menus)
- [Brave adblock-rust](https://github.com/brave/adblock-rust)
- [uAssets current YouTube filtering mechanisms](https://github.com/uBlockOrigin/uAssets/blob/master/filters/filters.txt)
