// Deterministic UI perf benchmark. Paste into the browser devtools Console
// of a running RapidRAW build (right-click -> Inspect) and press Enter.
//
// Drives the same scroll/open/slider-drag interaction with fixed synthetic
// timing every run, and measures its own frame timing via
// requestAnimationFrame + performance.now() -- standard web APIs, so this
// works the same under WebKitGTK (Linux), WKWebView (macOS), and WebView2
// (Windows). It does NOT depend on any devtools-specific recording/export
// format, unlike an earlier version of this tool that only worked with
// WebKit Web Inspector's Timeline export.
//
// Output: a JSON blob printed between BENCH_RESULT_JSON_START/END markers,
// and copied to the clipboard if the console supports copy(). Save it to
// bench/out/<name>.json (gitignored) and diff two runs with analyze.mjs.
//
// Requirements: library open with at least one image, at the default
// Adjustments panel (so two sliders are present to drag).

(async function bench() {
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const mark = (phases, name) => {
    phases[name] = performance.now();
  };
  const dispatchMouse = (target, type, x, y, opts = {}) =>
    target.dispatchEvent(
      new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, ...opts }),
    );

  const THUMBNAIL_SELECTOR = '[data-bench-id="thumbnail"], .aspect-square.bg-surface.rounded-md.overflow-hidden.cursor-pointer';
  const SCROLL_CONTAINER_SELECTOR = '.custom-scrollbar';
  const SLIDER_SELECTOR = '.slider-input';

  // --- frame-timing measurement -------------------------------------------------
  let measuring = false;
  let rafHandle = null;
  const frameTimestamps = [];

  function frameTick(ts) {
    if (!measuring) return;
    frameTimestamps.push(ts);
    rafHandle = requestAnimationFrame(frameTick);
  }

  function startMeasuring() {
    frameTimestamps.length = 0;
    measuring = true;
    rafHandle = requestAnimationFrame(frameTick);
  }

  function stopMeasuring() {
    measuring = false;
    if (rafHandle !== null) cancelAnimationFrame(rafHandle);
  }

  function summarizeFrames() {
    const durations = [];
    for (let i = 1; i < frameTimestamps.length; i++) {
      durations.push(frameTimestamps[i] - frameTimestamps[i - 1]);
    }
    const DROPPED_THRESHOLD_MS = 1000 / 60 + 2; // small tolerance over one vsync
    const dropped = durations.filter((d) => d > DROPPED_THRESHOLD_MS);
    const totalMs = durations.reduce((a, b) => a + b, 0);
    return {
      frameCount: frameTimestamps.length,
      avgFps: durations.length ? 1000 / (totalMs / durations.length) : 0,
      worstFrameMs: durations.length ? Math.max(...durations) : 0,
      droppedFrameCount: dropped.length,
      droppedFrameTimeMs: dropped.reduce((a, b) => a + b, 0),
    };
  }

  // --- interaction steps ---------------------------------------------------------
  async function scrollLibrary(phases) {
    const container = document.querySelector(SCROLL_CONTAINER_SELECTOR);
    if (!container) throw new Error(`bench: scroll container not found (${SCROLL_CONTAINER_SELECTOR})`);
    mark(phases, 'scroll:start');
    const steps = 30;
    for (let i = 0; i < steps; i++) {
      container.scrollTop += 40;
      container.dispatchEvent(new Event('scroll', { bubbles: true }));
      await wait(16);
    }
    mark(phases, 'scroll:end');
  }

  async function openFirstImage(phases) {
    const thumb = document.querySelector(THUMBNAIL_SELECTOR);
    if (!thumb) throw new Error(`bench: no thumbnail found (${THUMBNAIL_SELECTOR})`);
    mark(phases, 'open:start');
    const rect = thumb.getBoundingClientRect();
    dispatchMouse(thumb, 'dblclick', rect.left + rect.width / 2, rect.top + rect.height / 2);

    const timeoutMs = 8000;
    const pollMs = 50;
    let waited = 0;
    while (!document.querySelector(SLIDER_SELECTOR) && waited < timeoutMs) {
      await wait(pollMs);
      waited += pollMs;
    }
    if (waited >= timeoutMs) {
      throw new Error('bench: editor did not open (no sliders appeared within 8s)');
    }
    mark(phases, 'open:end');
  }

  async function dragSlider(slider, totalDeltaPx) {
    const rect = slider.getBoundingClientRect();
    const startX = rect.left + rect.width / 2;
    const y = rect.top + rect.height / 2;

    dispatchMouse(slider, 'mousedown', startX, y);
    await wait(16);

    const steps = 30;
    for (let i = 1; i <= steps; i++) {
      const x = startX + (totalDeltaPx * i) / steps;
      dispatchMouse(window, 'mousemove', x, y);
      await wait(16);
    }

    dispatchMouse(window, 'mouseup', startX + totalDeltaPx, y);
    await wait(16);
  }

  async function editTwoSliders(phases) {
    const sliders = document.querySelectorAll(SLIDER_SELECTOR);
    if (sliders.length < 2) throw new Error(`bench: fewer than 2 sliders found (${sliders.length})`);
    mark(phases, 'edit:start');
    await dragSlider(sliders[0], 80);
    await wait(200);
    await dragSlider(sliders[1], -60);
    mark(phases, 'edit:end');
  }

  // --- run -------------------------------------------------------------------
  const phases = {};
  try {
    mark(phases, 'full:start');
    startMeasuring();
    await scrollLibrary(phases);
    await wait(300);
    await openFirstImage(phases);
    await wait(300);
    await editTwoSliders(phases);
    stopMeasuring();
    mark(phases, 'full:end');
  } catch (err) {
    stopMeasuring();
    console.error('bench: failed —', err.message);
    return;
  }

  const result = {
    userAgent: navigator.userAgent,
    recordedAt: new Date().toISOString(),
    totalMs: phases['full:end'] - phases['full:start'],
    phases,
    frames: summarizeFrames(),
  };

  const json = JSON.stringify(result, null, 2);
  console.log('BENCH_RESULT_JSON_START');
  console.log(json);
  console.log('BENCH_RESULT_JSON_END');
  try {
    copy(json); // eslint-disable-line no-undef -- devtools console global
    console.log('bench: result copied to clipboard. Paste into bench/out/<name>.json');
  } catch {
    console.log('bench: clipboard copy() unavailable in this console, copy the JSON above manually');
  }
})();
