// SPDX-License-Identifier: MIT OR Apache-2.0
// B1: cost of one Date.now() call in Node.js.
//
//   node clockbench.js <iterations>
//
// Times the loop with process.hrtime.bigint() (CLOCK_MONOTONIC; a preload that
// only adds a constant offset doesn't change differences). Date.now() can't be
// optimised away because every result feeds a running sum. Prints one JSON line.
'use strict';

const n = Number(process.argv[2] || 10_000_000);
let sink = 0;
for (let i = 0; i < n / 10; i++) sink += Date.now() & 1; // warm-up / JIT
const t0 = process.hrtime.bigint();
for (let i = 0; i < n; i++) sink += Date.now() & 1;
const t1 = process.hrtime.bigint();
const ns = Number(t1 - t0) / n;
console.log(JSON.stringify({ ns_per_call: ns, iterations: n, wall_clock: Date.now() / 1000, sink: sink % 2 }));
