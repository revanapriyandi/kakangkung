# Isolated memory probe — 8 November 2026

Executable SHA-256 before filter-engine integration: `718F96E52893774BBE6EC67609A44783224113C1AF2C526473325F8C97EDD8A9`.
WebView2 process command lines identify runtime `157.0.4341.42`.

These are single-run observations of the entire launched process tree. Private working set and private commit are different metrics. Values below use MiB (1048576 bytes), not decimal MB. The probe does not trim working sets, disable sandboxing, or collapse renderer processes. Each scenario uses a distinct fresh temporary profile, no imported extensions, and no signed-in personal accounts. Start-Process uses WindowStyle Hidden; the application can still manage its own window visibility. Exact public network contents and media playback are not controlled.

| Scenario | Settle seconds | Processes | Private working set MiB | Private commit MiB |
|---|---:|---:|---:|---:|
| Native new tab | 20 | 1 | 43.87 | 45.88 |
| One web tab: example.com | 20 | 7 | 143.98 | 215.45 |
| Four simple public pages, Balanced | 20 | 10 | 245.92 | 350.38 |
| Four simple public pages, Balanced | 45 | 10 | 248.89 | 339.40 |
| Four simple public pages, UltraLite | 45 | 10 | 219.34 | 311.87 |

The four simple pages are example.com, example.org, example.net, and iana.org/domains/reserved. They are not representative of YouTube playback, authenticated GitHub applications, Instagram feeds, or Facebook feeds. For equal 45-second settling, the UltraLite observation is 11.87% lower private working set and 8.11% lower private commit. One run does not establish a stable causal effect; OS pressure, GPU allocation and changing network contents can affect the result. These samples did not independently query IsSuspended for each tab.

In the one-web-tab sample the GPU process accounts for approximately 53.7 MiB private working set, WebView2 browser process 36.6 MiB, native host 24.4 MiB, and renderer 11.1 MiB. Fixed engine and GPU costs already dominate this minimal page. There is no evidence here that a complete modern web browser with this workload fits within 100 MiB.

Source-level change: new profiles and reset preferences select UltraLite, which suspends unprotected hidden tabs after 30 seconds rather than the Balanced five-minute delay. Existing saved preferences are retained. Suspension pauses background scripts but retains document/form state. Automatic destructive discard remains disabled because reliable unsaved-form and other protection signals are incomplete. This change does not reduce active page complexity or promise a total process-memory ceiling.

Raw evidence directories:

- `memory-probe-20261108-233617`: native, one web, four web at 20 seconds. The first script invocation stopped before the final saver scenario due to a seed SQL typo, so raw per-process CSVs are available but its aggregate method/summary files were not emitted. The typo was corrected before later runs.
- `memory-probe-20261108-233747`: four web UltraLite at 45 seconds, with method and summary.
- `memory-probe-20261108-233911`: matched four web Balanced at 45 seconds, with method and summary.

`bench/memory-probe.ps1` is the reproducible probe; it preserves isolated profile folders for inspection and stops only its launched process tree. `docs/BENCHMARKS.md` is unchanged.

## Public social/video workload baseline

Same pre-filter-engine binary, fresh isolated profile, Balanced, 45-second settle; URLs opened in this exact order: YouTube Big Buck Bunny (`aqz-KE-bpKQ`), `github.com/revanapriyandi`, `instagram.com`, `facebook.com` (first tab, YouTube, selected after startup; corrected on 9 November 2026).

Observed **791.51 MiB private working set, 927.50 MiB private commit, 11 processes**. This reproduces the approximate reported 800 MB magnitude. It does not establish a leak: live content, engine processes, renderers and GPU allocations all count. Authentication and actual video playback were not confirmed. Source review on 9 November 2026 shows startup restores selection to the first URL after creating the remaining tabs (`crates/app/src/main.rs:268-275`). YouTube therefore remains selected in this ordering; the old hidden-YouTube interpretation was incorrect. Selection alone does not confirm playback or visibility during the entire sample. Evidence: `memory-probe-20261108-234058`.

The public four-site baseline consists of a largest renderer at 298.2 MiB private working set, GPU process 175.4 MiB, browser process 89.1 MiB, other renderers 66.7/52.4/52.3 MiB, and native host 27.8 MiB. Utility and crash-reporting processes make up the remainder. Renderer command lines do not identify each site's URL, so these allocations cannot be assigned reliably to individual sites from this probe alone.

Validation of the default-policy change: `cargo test -p memory -p tabs --offline` passed 6 tests and doc tests. `bench/memory-probe.ps1` passes PowerShell parser validation. The full application and after-build measurements are coordinated by the parent task.

## Final release pilot after filter/default changes

Old binary: SHA-256 `718F96E52893774BBE6EC67609A44783224113C1AF2C526473325F8C97EDD8A9`, preserved under `checks/chrome-parity/memory-baseline/kakangkung.exe`.
New binary: SHA-256 `C298AE36980543FAE66B1D4D3AD850DE1C4FF65A0FDE963D9E477A6C8A88CB48`.

The parent task completed compilation and tests before these runs; no concurrent builds or parent runtime probes ran during sampling. Existing personal Kakangkung PID 29772 was preserved and remained running. Only each explicitly launched isolated process tree was stopped.

The three matched four-site scenarios use identical URL order: YouTube Big Buck Bunny, GitHub profile, Instagram home, Facebook home (YouTube selected after startup; corrected on 9 November 2026). Mode is explicitly seeded in each fresh profile to separate code/filter changes from the changed default. Samples are one run each, not statistical benchmarks.

| Binary / scenario | Mode | Settle seconds | Processes | Private WS MiB | Commit MiB | Cumulative CPU seconds |
|---|---|---:|---:|---:|---:|---:|
| Old / four real public pages | Balanced | 45 | 11 | 756.63 | 880.80 | 45.017 |
| New / four real public pages | Balanced | 45 | 11 | 721.00 | 843.14 | 32.937 |
| New / four real public pages | UltraLite | 45 | 11 | 716.07 | 839.56 | 40.156 |
| New / alternate order, GitHub selected | UltraLite | 45 | 11 | 583.75 | 704.42 | 38.967 |
| New / one example.com web tab | Balanced | 20 | 7 | 124.09 | 196.08 | 4.515 |

The alternate-order row (historical raw scenario name `four-real-active-youtube`) opens GitHub, Instagram, Facebook, then YouTube. Startup selects the first URL, GitHub; the historical scenario name was misleading. This is a different scenario and must not be presented as a direct before/after improvement. The probe does not confirm actual playback; the parent's separate 61-second playback check is separate evidence. The social pages are unauthenticated and may display login walls.

For the matched four-site workload, new Balanced is 4.71% lower private working set than old Balanced in this pilot. New UltraLite is only 0.68% below new Balanced (0.42% lower commit), and uses more cumulative CPU time in this observation. Thus this pilot does **not** establish that earlier suspension materially solves this workload's memory use. Protected audio/media can remain running; this probe does not expose per-tab protection or confirmed suspension state. Variability in fetched content, GPU state, timing and OS memory pressure remains uncontrolled.

The achieved result is approximately **716 MiB private working set for the matched four-site UltraLite case**, not 100 MiB. Even the simple one-web-tab fresh profile uses **124 MiB private working set / 196 MiB commit**. No 100 MiB total guarantee is justified. To reduce materially more memory, a separate destructive discard policy would require trustworthy protection signals and explicit reload/data-loss tradeoffs; active page and GPU allocations remain outside a simple hidden-tab suspension policy.

CPU values are cumulative user plus kernel CPU seconds for the whole sampled process tree since each process started, not instantaneous CPU percentages. Raw per-process CSV files retain command lines, runtime identity and individual CPU/memory measurements.

Final raw directories:

- `bench/results/memory-probe-20261108-235511`: preserved old Balanced reference, method/hash, summary and per-process CSV.
- `bench/results/memory-probe-20261108-235631`: new release's four scenarios, method/hash, summary, per-process CSV and stderr logs.
- `bench/results/memory-probe-20261108-235617`: an empty preliminary invocation before comma-separated scenario selection was supported; no browser was launched, and this directory contains no measurement.

The existing CSV summaries use the host Indonesian decimal comma inside quoted cells. JSON console observations and the Markdown table above use decimal points. Future probe runs explicitly set en-US culture; raw measurements from these runs were not rewritten.

## Startup-order correction — 9 November 2026

The recorded memory, commit, CPU and process counts above are unchanged. The old active-tab descriptions assumed command-line startup left the last opened tab selected. Source inspection establishes that `main.rs` saves the first tab ID, opens the additional URLs, then calls `browser.activate(first)`. `bench/memory-probe.ps1` passes URLs in the order shown above. Therefore the standard four-site measurements select YouTube, and the alternate order selects GitHub. These historical runs did not record native `IsVisible`, `IsSuspended` or memory-target values per tab; source-derived selection is not retrospective runtime confirmation of those properties. New diagnostic runs must capture those values and protection/loading state before attributing a memory difference to suspension.
