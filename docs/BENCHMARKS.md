# Benchmarks

Every number on this page is written by [`bench/run.ps1`](../bench/run.ps1); none is typed or
estimated by hand. Raw per-process rows are in `bench/results/`.

## Method

- **What is measured:** the sum of the private working set (Task Manager's "Memory" column,
  `Win32_PerfFormattedData_PerfProc_Process.WorkingSetPrivate`) over the browser's whole process
  tree: for Kakangkung that is `kakangkung.exe` plus every WebView2 process it starts. Also the
  commit charge (`Win32_Process.PrivatePageCount`): memory reserved even when paged out, which is
  what runs a PC out of memory with many frozen tabs.
- **Procedure:** fresh profile per run (`--user-data-dir`, `--no-first-run`), the URLs given on the
  command line as tabs (1, 10, 25, 50, 100), wait 30 s + 0.6 s per tab, measure, close. 3 runs;
  the median and range are reported.
- **Pages:** what people keep open, all usable without signing in. Tab *i* opens URL *i* mod 14,
  so 25+ tabs open some sites twice: YouTube video, Kompas, Google Maps, Tokopedia,
  Reddit r/indonesia, GitHub, Detik, Stack Overflow, Amazon, BBC News, Excalidraw, Google
  Translate, CNN, Wikipedia (exact URLs in `bench/run.ps1`).
- **State of the tabs:** every tab loaded, one visible, the rest hidden. The wait is shorter than
  any browser's own tab-freezing timer. Each browser keeps its defaults for a fresh profile:
  Kakangkung's Memory Saver "Balanced" (freezes after 5 min) and built-in ad blocker on;
  Chrome's and Edge's own memory-saving features at their defaults, no extensions.
- **`kakangkung-saver`** is the "after suspension" row: Memory Saver set to Maximum (hidden tabs
  freeze after 30 s), measured 90 s + 0.6 s per tab after start. A frozen renderer's private
  working set drops to a few MB because Windows trims it; its commit charge stays.
- **Not yet measured** (task §7): after discard, memory saver explicitly on
  vs off in Chrome and Edge, cold start to first paint, tab-switch latency, leak and 30-minute
  growth checks.

## Environment

Windows 11 Pro build 26300, Intel Core i5-13420H, 16 GB RAM. Chrome 157.0.8103.38,
Edge 157.0.4341.42, WebView2 Runtime 157.0.4341.42.

## Results

<!-- results -->
Run 20261108-191357; 1 run(s) each, measured 30 s + 0.6 s per tab after start (90 s + 0.6 s per tab for kakangkung-saver); raw data: `bench/results/20261108-191357-processes.csv`.

| Browser | Tabs | Processes (median) | Private working set, median (MB) | Range (MB) | Commit, median (MB) |
|---|---:|---:|---:|---|---:|
| chrome | 50 | 93 | 5,059 | 5,059 – 5,059 | 6,726 |
| edge | 50 | 68 | 3,092 | 3,092 – 3,092 | 4,214 |
| kakangkung | 50 | 58 | 4,942 | 4,942 – 4,942 | 6,257 |
| kakangkung-saver | 50 | 73 | 4,705 | 4,705 – 4,705 | 8,323 |
<!-- /results -->

## Reading the numbers

WebView2 is the same Chromium engine as Edge, so one renderer costs about the same in every
browser here. Differences come from the processes around the pages (browser, GPU, utility,
services) and from what pages are allowed to load (the ad blocker).

With many tabs and nothing frozen yet, Kakangkung's GPU process and WebView2 browser process are
larger than Chrome's (in one 12-tab look with an earlier page set: GPU 174 vs 115 MB, browser 117 vs 76 MB), because every
tab is its own WebView2 control with its own window and surface; the renderers cost the same.
Kakangkung's saving comes from freezing hidden tabs, compare the `kakangkung` and
`kakangkung-saver` rows.
