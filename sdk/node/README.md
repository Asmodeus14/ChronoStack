# chronostack (Node register)

Makes a Node.js process **running directly on your machine** follow a ChronoStack stack's virtual clock. Services inside the stack don't need it; they get the virtual time from the preload library.

Use it when your app runs on the host (`npm run dev`) and only its databases run in Docker.

```sh
chronostack up --wait                         # in your project directory
node -r ./path/to/sdk/node/register.js app.js # or NODE_OPTIONS="-r …/register.js"
```

The package isn't published to npm yet (ChronoStack is pre-alpha). Use it from a clone of the repository.

## What it changes

- `Date.now()`, `new Date()` and `Date()` return virtual time. Dates built from explicit values (`new Date(0)`, `new Date('2026-01-01')`) are unchanged, and `instanceof Date` still works.
- `performance.now()`, `process.hrtime()`, `setTimeout` and `setInterval` keep real time, like every other runtime under ChronoStack. A timer set for 24 hours does not fire when you advance 24 hours. Re-arm it from `onAdvance` if needed.

## How it stays in sync

1. At startup it reads the clock once, synchronously, from the controller. It does this in a short-lived child process, because Node has no synchronous HTTP.
2. Then it long-polls the controller's `/v1/clock/wait`. An `advance`, `freeze` or `unfreeze` reaches the process within milliseconds, usually before the CLI command has returned.
3. The connection never keeps your process alive.

Measured against a real stack: the process's `Date.now()` matched the controller's virtual time to within 1 ms (`tests/e2e/run_m4.py`; the target is 50 ms).

## Finding the controller

In order:

1. `CHRONOSTACK_URL` and `CHRONOSTACK_TOKEN`;
2. `CHRONOSTACK_SESSION`, the path to a `.chronostack/session.json`;
3. the nearest `.chronostack/session.json` in the current directory or a parent.

If none is found, or the controller is unreachable, the process keeps real time and prints one warning. With `CHRONOSTACK_STRICT=1` it fails instead, which is useful in CI.

## API

```js
const cs = require('./path/to/sdk/node');

cs.onAdvance(({ virtualMs, offsetMs, frozen, generation }) => {
  // e.g. re-arm an in-process scheduler after the clock jumps
});
cs.now();        // virtual ms since the epoch
cs.active;       // false if it fell back to real time
cs.RealDate;     // the original Date
```

Types are in `index.d.ts`. Tests: `node --test test/register.test.js`. They use a fake controller and need no Docker.
