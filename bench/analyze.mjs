#!/usr/bin/env node
// Diff two bench/replay.js result JSONs.
//
// Usage:
//   node bench/analyze.mjs bench/out/before.json
//   node bench/analyze.mjs bench/out/before.json bench/out/after.json

import { readFileSync } from 'node:fs';

const START_MARKER = 'BENCH_RESULT_JSON_START';
const END_MARKER = 'BENCH_RESULT_JSON_END';

// Tolerate pasting the raw devtools console pane instead of just the JSON:
// strips "[Log] " prefixes, "(file.ts, line N)" source-location suffixes
// devtools appends to each log line, and REPL echo lines (e.g. the
// "< Promise {...}" devtools prints for the pasted async IIFE's return
// value). Also slices out just the BENCH_RESULT_JSON_START/END span if
// present, so noise before/after (like the "copied to clipboard" log) is
// ignored automatically.
function extractJson(raw) {
  const startIdx = raw.indexOf(START_MARKER);
  const endIdx = raw.indexOf(END_MARKER);
  const body =
    startIdx !== -1 && endIdx !== -1 && endIdx > startIdx
      ? raw.slice(startIdx + START_MARKER.length, endIdx)
      : raw;

  return body
    .split('\n')
    .map((line) =>
      line
        .replace(/^\s*<.*$/, '')
        .replace(/^\[(?:Log|Info|Debug|Warn|Error)\]\s?/, '')
        .replace(/\s*\([^()]*,\s*line\s*\d+\)\s*$/i, ''),
    )
    .join('\n')
    .trim();
}

function load(path) {
  const raw = readFileSync(path, 'utf8');
  const jsonText = extractJson(raw);
  try {
    return JSON.parse(jsonText);
  } catch (err) {
    throw new Error(
      `could not parse ${path} as JSON, even after stripping devtools console noise (${err.message})`,
    );
  }
}

function pct(a, b) {
  if (a === 0) return 'n/a';
  return `${(((b - a) / a) * 100).toFixed(1)}%`.replace(/^(?!-)/, '+');
}

function reportSingle(path, r) {
  console.log(`=== ${path}`);
  console.log(`  user agent: ${r.userAgent}`);
  console.log(`  total wall-clock: ${r.totalMs.toFixed(1)}ms`);
  console.log(`  frames: ${r.frames.frameCount}, avg fps: ${r.frames.avgFps.toFixed(1)}`);
  console.log(
    `  dropped frames (>16.7ms): ${r.frames.droppedFrameCount}, total ${r.frames.droppedFrameTimeMs.toFixed(1)}ms, worst ${r.frames.worstFrameMs.toFixed(1)}ms`,
  );
  console.log();
}

function reportDiff(pathA, a, pathB, b) {
  reportSingle(pathA, a);
  reportSingle(pathB, b);
  console.log('=== diff (before -> after)');
  const rows = [
    ['total wall-clock (ms)', a.totalMs, b.totalMs],
    ['avg fps', a.frames.avgFps, b.frames.avgFps],
    ['dropped-frame count', a.frames.droppedFrameCount, b.frames.droppedFrameCount],
    ['dropped-frame time (ms)', a.frames.droppedFrameTimeMs, b.frames.droppedFrameTimeMs],
    ['worst frame (ms)', a.frames.worstFrameMs, b.frames.worstFrameMs],
  ];
  for (const [label, av, bv] of rows) {
    console.log(`  ${label}: ${av.toFixed(1)} -> ${bv.toFixed(1)}  (${pct(av, bv)})`);
  }
}

const [, , pathA, pathB] = process.argv;
if (!pathA) {
  console.log('Usage: node bench/analyze.mjs <result.json> [other-result.json]');
  process.exit(1);
}

if (!pathB) {
  reportSingle(pathA, load(pathA));
} else {
  reportDiff(pathA, load(pathA), pathB, load(pathB));
}
