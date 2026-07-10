# UI perf benchmark

Deterministic replay for comparing UI smoothness before/after a change, instead of
hand-timed manual testing (which is too noisy to draw conclusions from).

## Usage

1. Run the build you want to measure (`npm start`, or a packaged build) and get to the
   library view with at least one image loaded.
2. Open devtools (right-click → Inspect) and switch to the Console tab.
3. Paste the contents of `bench/replay.js` and press Enter.
4. It scrolls the library, opens the first image, and drags two sliders, then prints a
   JSON result between `BENCH_RESULT_JSON_START`/`END` markers (and copies it to your
   clipboard if the console supports `copy()`).
5. Save the result as `bench/out/<name>.json` (gitignored, doesn't need to be committed).
6. Compare two runs:
   ```
   node bench/analyze.mjs bench/out/before.json bench/out/after.json
   ```

## Why it's built this way

- **Self-measuring, not devtools-export-dependent.** `replay.js` times itself with
  `requestAnimationFrame`/`performance.now()` (standard web APIs) rather than relying on
  a devtools Timeline recording. Tauri uses a different webview per OS — WebKitGTK on
  Linux, WKWebView on macOS, WebView2 (Chromium) on Windows — and each one's devtools
  Timeline/Performance export uses a different JSON format. Self-measurement sidesteps
  that entirely, so the same script and analyzer work on all three platforms.
- **Fixed synthetic input.** All interaction timing is scripted (`setTimeout` steps at a
  fixed cadence, fixed pixel deltas), so two runs get identical input instead of
  whatever pacing a human happened to use. Don't hand-drive the interaction and try to
  compare the numbers to a scripted run — only compare scripted run to scripted run.

## Known limitations

- Numbers are only comparable **on the same machine** (same window size, same display
  scaling). A slider's pixel-delta → value-delta depends on its on-screen width, so
  results aren't meaningful across different maintainers' machines — only before/after
  on one machine.
- The thumbnail selector prefers `[data-bench-id="thumbnail"]` (see
  `src/components/panel/library/LibraryItems.tsx`) with a Tailwind-class fallback. If
  you add new interaction steps, prefer adding a `data-bench-id` hook over relying on
  utility classes, which shift on any restyle.
- This is a general smoothness/regression check, not a profiler. It won't tell you
  *why* something is slow — use devtools Timeline/Performance recording by hand for
  root-causing, and this script for confirming a fix actually helped.
