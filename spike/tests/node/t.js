// SPDX-License-Identifier: MIT OR Apache-2.0
// Node probes for M0.
//   node t.js now      -> {"now": <Date.now()/1000>}
//   node t.js nohang   -> one JSON line per wait primitive (elapsed via performance.now)
//   node t.js timer    -> arms setTimeout(5000) and reports when it actually fired
const { performance } = require('perf_hooks');
const { Worker } = require('worker_threads');
const mode = process.argv[2];

function classify(ms) {
  if (ms < 900) return 'EARLY';
  if (ms > 2500) return 'SLOW';
  return 'OK';
}

async function nohang() {
  const tests = {
    setTimeout_1s: () => new Promise((r) => setTimeout(r, 1000)),
    setInterval_1s: () => new Promise((r) => { const i = setInterval(() => { clearInterval(i); r(); }, 1000); }),
    atomics_wait_1s: () => new Promise((r) => {
      // Atomics.wait blocks the thread; run it in a worker.
      const w = new Worker(
        "const sab=new SharedArrayBuffer(4);Atomics.wait(new Int32Array(sab),0,0,1000);require('worker_threads').parentPort.postMessage(1)",
        { eval: true });
      w.on('message', () => { w.terminate(); r(); });
    }),
    worker_setTimeout_1s: () => new Promise((r) => {
      const w = new Worker("setTimeout(()=>require('worker_threads').parentPort.postMessage(1),1000)", { eval: true });
      w.on('message', () => { w.terminate(); r(); });
    }),
  };
  for (const [name, fn] of Object.entries(tests)) {
    const t0 = performance.now();
    const res = await Promise.race([fn().then(() => 'done'), new Promise((r) => setTimeout(() => r('HANG'), 6000).unref())]);
    const ms = performance.now() - t0;
    console.log(JSON.stringify({ test: name, elapsed_ms: Math.round(ms), status: res === 'HANG' ? 'HANG' : classify(ms) }));
  }
  process.exit(0);
}

if (mode === 'now') {
  console.log(JSON.stringify({ now: Date.now() / 1000, new_date: new Date().getTime() / 1000 }));
} else if (mode === 'nohang') {
  nohang();
} else if (mode === 'timer') {
  const t0 = performance.now();
  console.log(JSON.stringify({ armed_at_virtual: Date.now() / 1000 }));
  setTimeout(() => {
    console.log(JSON.stringify({ fired_after_ms: Math.round(performance.now() - t0), fired_at_virtual: Date.now() / 1000 }));
  }, 5000);
}
