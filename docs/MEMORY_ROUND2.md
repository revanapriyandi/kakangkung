# Memory and tab-preview investigation — 9 November 2026

The requested approximately 100 MiB total for YouTube, GitHub, Instagram and
Facebook has not been achieved. Measurements include the application and all
identified WebView2 runtime processes. A tab's JavaScript heap is a subset of
that total and is never substituted for whole-browser RAM in these comparisons.

## Observed memory behavior

The new optional `KAKANGKUNG_MEMORY_TRACE` JSONL diagnostic reads native
visibility, suspension and memory-target properties without evaluating scripts
in sleeping documents. It records hostnames, state/protection flags, process
private working set and private commit. It is disabled unless an output path is
explicitly provided.

The four-site Maximum-mode run recorded YouTube active and the other three
views natively suspended by approximately 30 seconds. Their native visibility
was false and target level Low; YouTube remained visible, running and Normal.
Thus the remaining footprint cannot be attributed merely to a missing suspend
command. Historical startup-selection labels have also been corrected in
`bench/memory-probe-findings.md`: startup selects the first URL.

| Diagnostic run, same four URLs, 45 s | Private working set | Private commit |
|---|---:|---:|
| Ordinary suspension, GC experiment off | 699.06 MiB | 831.51 MiB |
| Idle-only heap collection before suspension | 711.79 MiB | 832.86 MiB |

These are single-run observations with changing public content, not causal
benchmarks. Three hidden-tab garbage collections completed in 69–108 ms. The
experiment did not establish a reduction, so it is **not enabled by default**.
`KAKANGKUNG_MEMORY_COMPACT=1` is an explicit diagnostic experiment, not a memory
ceiling or a recommended normal configuration.

Raw evidence is in `app/bench/results/memory-probe-20261109-002419` and
`memory-probe-20261109-002545`, with native-state traces under the parent
workspace's `checks/memory-round2/gc-off.jsonl` and `gc-on.jsonl`.

Global CDP pressure notifications were rejected: the protocol broadcasts to all
processes, including active playback. Leak-detection preparation was also rejected
because it terminates workers and drops caches. No sandbox removal, forced
working-set trimming, active-video quality reduction or automatic destructive
tab discard was introduced.

## Playback protection

Previously the page helper reported camera/microphone capture, while ordinary
playback relied on the native audible-audio signal. Muted video therefore lacked
its own playback protection. The helper now observes media/PiP lifecycle events,
including muted playback, and combines them with capture tracks.

Each document has its own bounded identifier. Native aggregation prevents one
iframe's pause from clearing another frame's playback. Page-hide/page-show
signals handle removal and BFCache restoration. Direct stop listeners also
release detached media elements; a WeakSet avoids retaining stopped elements.
Playback adds no polling timer. The existing capture-track timer runs only while
capture tracks are live.

## Hover cards and measurement limits

One nonactivating native window draws the card, with no additional WebView.
At most eight 320 × 180 BGRA snapshots are cached: 1,843,200 bytes maximum for
decoded thumbnails, plus the currently displayed bitmap/window resources.
Snapshots are captured in memory while a page is visible and are not persisted.
Native screenshot capture can temporarily allocate viewport-sized resources.
Navigation and closing a tab invalidate its snapshot; sleeping-tab hovers use
the prior snapshot and do not resume the document.

A native crash was observed in the installed WebView2 157 runtime during initial
hover testing. Microsoft's open [frame-iterator regression report](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5793)
describes a matching API sequence in runtime 156. The precise local crash cause
was not symbolically established, so runtime 156 and newer unverified versions
avoid that native frame-materialization path. The card instead uses
`Runtime.getHeapUsage` and labels the value **JS heap only**, with an exclusion
note for images, video and GPU memory. Cached values are marked. This is an
isolate-level JS measurement, not the tab's complete renderer footprint.

Older supported frame-attribution paths identify renderer processes by native
frame IDs/ancestry and deduplicate PIDs; these totals are conservatively labelled
shared. No total is divided by the number of tabs or inferred from matching URLs.

## Final release verification

`checks/memory-round2/final/20261109-025833-245/selftest.json` in the parent
workspace records **35 passing native checks and zero failures**. It includes
the normal native mouse-hover/timer route, real in-memory thumbnail decoding,
heap measurement, unchanged WebView count, sleeping-tab hover without resume,
and retention of a live JS object and unsaved form through suspend/resume.
The synthetic hover fixture suppresses OS mouse-leave tracking so it does not
move the physical pointer. The self-test also isolates external focus loss;
normal browser windows retain their existing close-on-focus-loss behavior.

The public signed-out workload opened YouTube, GitHub, Instagram and Facebook
with four WebViews. Prior HTML fixture views were closed first. YouTube's
1280 × 720 video advanced to **68.32 seconds**, with **seven background samples**
while GitHub was active for approximately 35 seconds. It remained natively
unsuspended, reported no media error and no pause; the probe attempted playback
only once and did not auto-resume a paused video. The decoder reported 28 dropped
frames out of 2,597; rendering naturally throttled while the tab was hidden.
This is one public-video smoke test, not a guarantee about all sites or streams.
Signed-in social feeds were not exercised.

Total private working set at the final video sample was **631.56 MiB**, with
**965.41 MiB private commit**. Across samples it ranged from 607.34 to 1,000.91 MiB.
This was a functional video/preview test with changing network contents, not a
matched before/after memory benchmark. It does not establish an additional RAM
improvement or meet the requested approximately 100 MiB total. The card's roughly
66.5 MiB JS heap value is a different measurement and must not be presented as
whole-browser memory. Native traces are in `checks/memory-round2/four-sites-final.jsonl`.

The final workspace suite passed **129 Rust tests**, with one network-dependent
test ignored by default; **26 JavaScript** and **10 Python** tests passed.
Clippy with warnings denied, the release build and diff whitespace checks passed.
Snapshots include `shots/tab-hover.png` and `shots/youtube-tab-hover.png` beside
the native report. The JavaScript tests include detached media cleanup and
independent frame protection; the real video probe independently verifies the
muted-background playback path.

## Primary references

- [WebView2 visibility](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller#get_isvisible)
- [Suspend/resume](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3#trysuspend)
- [Memory target and paging caveats](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_19)
- [CDP Runtime heap measurement](https://github.com/ChromeDevTools/devtools-protocol/blob/master/json/js_protocol.json)
- [CDP global memory-pressure and leak-detection scope](https://github.com/ChromeDevTools/devtools-protocol/blob/master/pdl/domains/Memory.pdl)
