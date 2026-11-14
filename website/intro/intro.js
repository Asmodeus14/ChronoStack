// ChronoStack intro, black and white.
// Two Victorian hands unfold from a point, a drop of white floods the screen,
// then every sweep of the minute hand inverts the screen again (black ↔ white).
// Every shape is XOR'd with what is under it, so the hands are always the opposite colour.
// Plain WebGL2, no dependencies. ?t=<seconds> freezes the timeline (for screenshots).

const TAU = Math.PI * 2;
const $ = (s) => document.querySelector(s);
const params = new URLSearchParams(location.search);
const FREEZE = params.has('t') ? parseFloat(params.get('t')) : null;
const REDUCED = matchMedia('(prefers-reduced-motion: reduce)').matches && !params.has('motion');

const intro = $('#intro');
const canvas = $('#gl');
const status = $('#intro-status');

// ---------------------------------------------------------------- timeline (seconds)
// Phases overlap so each motion hands off to the next without a dead frame.
const GROW0 = 0.25, GROW1 = 1.0;    // the hands unfold from the pivot
const DROP0 = 0.75, DROP1 = 1.6;    // a drop of white floods the screen (starts while the hands settle)
const RUN0 = 1.35, RUN = 3.0;       // the advance: three sweeps, each one inverts the screen
const SWEEPS = 3;
const FOLD0 = 0.05, FOLD = 0.45;    // after the last sweep: the hands fold into the pivot
const IRIS0 = 0.42, IRIS = 0.95;    // then the pivot opens like an iris onto the page
const TOTAL = 14 * 1440 + 180;      // +14d 03:00 in minutes; the hour hand ends on III

const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const expoOut = (x) => (x >= 1 ? 1 : 1 - Math.pow(2, -10 * clamp(x)));
const smooth = (x) => { x = clamp(x); return x * x * (3 - 2 * x); };
const cubicOut = (x) => 1 - (1 - clamp(x)) ** 3;
const cubicInOut = (x) => { x = clamp(x); return x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2; };
const fract = (x) => ((x % 1) + 1) % 1;

// ---------------------------------------------------------------- fallback
const root = document.documentElement;
const announce = (name) => dispatchEvent(new CustomEvent(name));
function goStatic() {
  root.classList.add('no-intro');
  root.classList.remove('is-locked');
  status.textContent = 'ChronoStack loaded.';
  announce('chronostack:intro-done');
}
const gl = REDUCED || params.has('nointro') ? null : canvas.getContext('webgl2', { antialias: false, alpha: false });

// ---------------------------------------------------------------- the hands (Canvas2D silhouettes)
function cnv(w, h) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  return [c, c.getContext('2d')];
}
function ringPath(x, cx, cy, r, t) {
  x.beginPath();
  x.arc(cx, cy, r, 0, TAU);
  x.arc(cx, cy, r - t, 0, TAU, true);
  x.fill();
}

// Drawn in hand units, pivot at the origin, tip at +y.
const MINUTE = { len: 0.85, tail: 0.19, halfW: 0.056 };
const HOUR = { len: 0.56, tail: 0.17, halfW: 0.085 };

function drawHand(spec, w, draw) {
  const h = Math.round((w * (spec.len + spec.tail)) / (2 * spec.halfW));
  const [c, x] = cnv(w, h);
  const s = w / (2 * spec.halfW);
  x.setTransform(s, 0, 0, -s, w / 2, (h * spec.len) / (spec.len + spec.tail));
  x.fillStyle = '#fff';
  draw(x);
  return c;
}

function drawMinute(x) {
  x.beginPath(); x.moveTo(-0.009, 0); x.lineTo(-0.006, -0.112); x.lineTo(0.006, -0.112); x.lineTo(0.009, 0); x.fill();
  ringPath(x, 0, -0.146, 0.035, 0.011);                  // counterweight
  x.beginPath(); x.moveTo(-0.014, 0); x.lineTo(-0.0045, 0.502); x.lineTo(0.0045, 0.502); x.lineTo(0.014, 0); x.fill();
  ringPath(x, 0, 0.546, 0.046, 0.011);                   // the Breguet moon
  x.beginPath(); x.moveTo(-0.004, 0.588); x.lineTo(-0.0024, 0.74); x.lineTo(0.0024, 0.74); x.lineTo(0.004, 0.588); x.fill();
  x.beginPath();                                          // lozenge tip
  x.moveTo(0, 0.845);
  x.quadraticCurveTo(0.004, 0.792, 0.017, 0.768);
  x.quadraticCurveTo(0.004, 0.75, 0, 0.726);
  x.quadraticCurveTo(-0.004, 0.75, -0.017, 0.768);
  x.quadraticCurveTo(-0.004, 0.792, 0, 0.845);
  x.fill();
}

function drawHour(x) {
  x.beginPath(); x.moveTo(-0.011, 0); x.lineTo(-0.007, -0.1); x.lineTo(0.007, -0.1); x.lineTo(0.011, 0); x.fill();
  ringPath(x, 0, -0.132, 0.034, 0.012);
  x.beginPath(); x.moveTo(-0.02, 0); x.lineTo(-0.009, 0.302); x.lineTo(0.009, 0.302); x.lineTo(0.02, 0); x.fill();
  x.beginPath();                                          // the spade
  x.moveTo(0, 0.56);
  x.bezierCurveTo(0.012, 0.52, 0.083, 0.47, 0.075, 0.4);
  x.bezierCurveTo(0.068, 0.34, 0.03, 0.325, 0.012, 0.29);
  x.lineTo(-0.012, 0.29);
  x.bezierCurveTo(-0.03, 0.325, -0.068, 0.34, -0.075, 0.4);
  x.bezierCurveTo(-0.083, 0.47, -0.012, 0.52, 0, 0.56);
  x.fill();
  x.globalCompositeOperation = 'destination-out';       // piercings
  for (const sx of [-1, 1]) {
    x.beginPath(); x.ellipse(sx * 0.035, 0.402, 0.015, 0.03, sx * 0.45, 0, TAU); x.fill();
  }
  x.beginPath(); x.moveTo(0, 0.478); x.lineTo(0.008, 0.452); x.lineTo(0, 0.426); x.lineTo(-0.008, 0.452); x.closePath(); x.fill();
  x.globalCompositeOperation = 'source-over';
}

// ---------------------------------------------------------------- shader
const MIN_TEX_W = 192, HOUR_TEX_W = 256;

const VS = `#version 300 es
in vec2 aPos;
void main(){ gl_Position = vec4(aPos, 0., 1.); }`;

const FS = `#version 300 es
precision highp float;
out vec4 o;
uniform vec2 uRes;
uniform float uTime;
uniform sampler2D uMinTex, uHourTex;
uniform vec4 uHands;     // minute rev, hour rev, minute span this frame, hour span this frame
uniform vec4 uHandK;     // length (0..1), size in scene units, pivot radius, -
uniform vec4 uDrop;      // cx, cy, radius, wobble
uniform float uDropBlur; // how far the drop edge moved this frame
uniform vec4 uWipe;      // sweep fraction, base parity, wipe on, sweep span this frame
uniform vec2 uIris;      // iris radius, ring width (scene units)
const float TAU = 6.28318530718;
const vec3 INK = vec3(.043, .047, .055);
const vec3 PAPER = vec3(.957, .949, .925);
const vec3 AMBER = vec3(.949, .639, .227);
const float ML=${MINUTE.len}, MT=${MINUTE.tail}, MW=${MINUTE.halfW};
const float HL=${HOUR.len}, HT=${HOUR.tail}, HW=${HOUR.halfW};
const float MTEXEL = ${(2 * MINUTE.halfW / MIN_TEX_W).toFixed(6)};
const float HTEXEL = ${(2 * HOUR.halfW / HOUR_TEX_W).toFixed(6)};

float hash(vec2 p){ p=fract(p*vec2(123.34,456.21)); p+=dot(p,p+45.32); return fract(p.x*p.y); }
// soft XOR of two coverages: keeps the antialiased edges
float xr(float a, float b){ return a + b - 2.*a*b; }

float hand(sampler2D t, vec2 q, float a, float g, float L, float T, float W, float lod){
  vec2 d = vec2(sin(a), cos(a));
  vec2 uv = vec2(dot(q, vec2(d.y,-d.x))/(2.*W) + .5, (dot(q,d)/g + T)/(L+T));
  if (uv.x<0. || uv.x>1. || uv.y<0. || uv.y>1.) return 0.;
  return textureLod(t, uv, lod).a;
}
// Motion blur: evenly spaced copies along the arc swept this frame, each one softened
// (via the mip chain) just enough to close the gap to its neighbour, so it reads as a smear.
float handBlur(sampler2D t, vec2 q, float rev, float span, float g, float L, float T, float W, float texel){
  span = clamp(span, -.5, .5);
  int n = int(clamp(ceil(abs(span)*260.), 1., 40.));
  float gap = abs(span)*TAU*max(length(q), .04)/float(n);
  float lod = n > 1 ? clamp(log2(gap/texel), 0., 6.) : 0.;
  float acc = 0.;
  for (int i=0; i<40; i++){
    if (i>=n) break;
    acc += hand(t, q, (rev - span*(float(i)+.5)/float(n))*TAU, g, L, T, W, lod);
  }
  return acc/float(n);
}

void main(){
  vec2 p = (gl_FragCoord.xy - .5*uRes)/uRes.y*2.;
  float px = 2./uRes.y;
  float r = length(p);
  float v = uWipe.y;

  // the drop: a circle of the opposite colour with a liquid edge, blurred by its own speed
  if (uDrop.z > 0.){
    vec2 d = p - uDrop.xy;
    float a = atan(d.y, d.x);
    float edge = uDrop.z + uDrop.w*(.6*sin(a*5. + uTime*3.1) + .4*sin(a*9. - uTime*4.3));
    float soft = max(px, uDropBlur*.5);
    v = xr(v, 1. - smoothstep(edge - soft, edge + soft, length(d)));
  }

  // the sweep: everything the minute hand has passed this turn is inverted.
  // The leading edge is smeared over the angle it covered this frame.
  if (uWipe.z > 0.){
    float ang = fract(atan(p.x, p.y)/TAU);
    float pxRev = px/(TAU*max(r, 1e-3));
    float ew = max(abs(uWipe.w), pxRev*2.);
    float lead = clamp((uWipe.x - ang)/ew + .5, 0., 1.);
    float trail = smoothstep(-px, px, ang*TAU*r);
    v = xr(v, lead*trail);
  }

  // the hands
  if (uHandK.x > .001){
    vec2 q = p/uHandK.y;
    float hh = handBlur(uHourTex, q, uHands.y, uHands.w, uHandK.x, HL, HT, HW, HTEXEL);
    float mm = handBlur(uMinTex,  q, uHands.x, uHands.z, uHandK.x, ML, MT, MW, MTEXEL);
    v = xr(v, max(hh, mm));
  }

  // the pivot, which at the end opens like an iris (the page shows through the hole via a CSS mask)
  float hole = uIris.x > 0. ? uIris.x : uHandK.z*.38;
  float outer = uIris.x > 0. ? uIris.x + uIris.y : uHandK.z;
  float amber = 0.;
  if (outer > 0.){
    float ring = max((1. - smoothstep(outer - px, outer + px, r)) - (1. - smoothstep(hole - px, hole + px, r)), 0.);
    if (uIris.x > 0.) amber = ring;   // the opening iris is the one amber moment, like the arc in the logo
    else v = xr(v, ring);
  }

  vec3 col = mix(mix(INK, PAPER, v), AMBER, amber);
  // film grain, refreshed 24 times a second like film, not every frame
  col += (hash(gl_FragCoord.xy + floor(uTime*24.)*17.31) - .5)*.022;
  o = vec4(col, 1.);
}`;

// ---------------------------------------------------------------- GL plumbing
function shader(type, src) {
  const s = gl.createShader(type);
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
  return s;
}
function program(vs, fs) {
  const p = gl.createProgram();
  gl.attachShader(p, shader(gl.VERTEX_SHADER, vs));
  gl.attachShader(p, shader(gl.FRAGMENT_SHADER, fs));
  gl.bindAttribLocation(p, 0, 'aPos');
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  const u = {};
  const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS);
  for (let i = 0; i < n; i++) {
    const name = gl.getActiveUniform(p, i).name;
    u[name] = gl.getUniformLocation(p, name);
  }
  return { p, u };
}
function texture(source) {
  const t = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, t);
  gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
  gl.generateMipmap(gl.TEXTURE_2D);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return t;
}

// ---------------------------------------------------------------- main
async function start() {
  const prog = program(VS, FS);
  const vbo = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

  const minTex = texture(drawHand(MINUTE, MIN_TEX_W, drawMinute));
  const hourTex = texture(drawHand(HOUR, HOUR_TEX_W, drawHour));

  // ---- resolution: device pixels, scaled down automatically if frames run long
  let W = 0, H = 0, quality = 1, lastQualityChange = 0;
  const maxDpr = Math.min(window.devicePixelRatio || 1, 2);
  function resize() {
    const dpr = Math.max(0.75, maxDpr * quality);
    const w = Math.round(innerWidth * dpr), h = Math.round(innerHeight * dpr);
    if (w === W && h === H) return;
    W = canvas.width = w; H = canvas.height = h;
    gl.viewport(0, 0, W, H);
  }
  resize();
  addEventListener('resize', resize);

  // ---- state
  let S;
  function reset() {
    S = {
      T: 0, rate: 1, holding: false, p: 0, done: false, Te: 0,
      revealed: false, gone: false,
      prevMin: 0, prevHour: 0, prevRev: 0, prevDrop: 0, first: true,
      spanMin: 0, spanHour: 0, spanWipe: 0, dropBlur: 0,
      hands: [0, 0, 0, 0], handK: [0, 0.5, 0, 0], drop: [0, 0, 0, 0], wipe: [0, 0, 0, 0], iris: [0, 0],
    };
    intro.classList.remove('is-revealed', 'is-holding', 'is-running', 'is-iris');
    intro.style.removeProperty('--iris');
    root.classList.remove('intro-done', 'intro-gone');
    root.classList.add('is-locked');
    status.textContent = 'Loading ChronoStack…';
    announce('chronostack:intro-start');
  }
  reset();

  // per-frame movement, smoothed so uneven frame pacing doesn't make the blur flicker
  const ease = (prev, next) => prev + (next - prev) * 0.5;

  function step(dt) {
    S.T += dt;
    const t = S.T;
    const aspect = innerWidth / innerHeight;
    const size = Math.min(0.92, aspect * 0.8);           // hand scale, in half-screen-heights
    const cover = Math.hypot(aspect, 1) * 1.08;            // radius that covers the screen
    intro.classList.toggle('is-running', t > DROP0);

    // the advance
    if (t > RUN0 && !S.done) S.p = Math.min(1, S.p + dt / RUN);
    const rev = SWEEPS * cubicInOut(S.p);
    const minRev = rev, hourRev = rev / 12;              // ends on 12 and III
    if (!S.done && S.p >= 1) { S.done = true; S.Te = t; }
    const e = S.done ? t - S.Te : 0;

    const moved = !S.first && dt > 0;
    S.spanMin = moved ? ease(S.spanMin, minRev - S.prevMin) : 0;
    S.spanHour = moved ? ease(S.spanHour, hourRev - S.prevHour) : 0;
    S.prevMin = minRev; S.prevHour = hourRev;

    // unfold, then fold away as the hands come to rest
    let g = expoOut((t - GROW0) / (GROW1 - GROW0));
    if (S.done) g *= 1 - cubicInOut((e - FOLD0) / FOLD);
    const pivot = 0.032 * cubicOut((t - GROW0) / 0.45);
    S.hands = [fract(minRev), fract(hourRev), S.spanMin, S.spanHour];
    S.handK = [g, size, pivot, 0];

    // the drop: eases in from the pivot, floods the screen, then becomes the new background
    const k = (t - DROP0) / (DROP1 - DROP0);
    let parity = 0, rad = 0;
    if (k >= 1) { parity = 1; S.drop = [0, 0, 0, 0]; }
    else if (k > 0) {
      rad = cover * cubicOut(smooth(k));
      S.drop = [0, 0, Math.max(rad, 0.0001), 0.012 * (1 - k) * Math.min(1, k * 6)];
    } else S.drop = [0, 0, 0, 0];
    S.dropBlur = moved && k > 0 && k < 1 ? ease(S.dropBlur, Math.min(Math.abs(rad - S.prevDrop), 0.05)) : 0;
    S.prevDrop = rad;

    // each full sweep flips the background; the current sweep is drawn as a wedge
    const turns = Math.floor(rev + 1e-6);
    S.spanWipe = moved ? ease(S.spanWipe, rev - S.prevRev) : 0;
    S.prevRev = rev;
    S.wipe = [fract(rev), (parity + turns) % 2, t > RUN0 && !S.done ? 1 : 0, S.spanWipe];

    // the iris: the pivot's hole opens onto the page
    const ik = S.done ? (e - IRIS0) / IRIS : 0;
    if (ik > 0) {
      const irisR = 0.032 * 0.38 + cubicInOut(ik) * cover;
      const ring = 0.02 * (1 - smooth(ik * 1.6));
      S.iris = [irisR, Math.max(ring, 0)];
      intro.classList.add('is-iris');
      intro.style.setProperty('--iris', `${(irisR * innerHeight) / 2}px`);
      if (!S.revealed && ik > 0.08) {
        S.revealed = true;
        intro.classList.add('is-revealed');
        root.classList.add('intro-done');
        root.classList.remove('is-locked');
        status.textContent = 'ChronoStack loaded.';
        announce('chronostack:intro-done');
      }
      if (!S.gone && ik >= 1) { S.gone = true; root.classList.add('intro-gone'); }
    } else S.iris = [0, 0];

    S.first = false;
  }

  function render() {
    resize();
    gl.useProgram(prog.p);
    gl.uniform2f(prog.u.uRes, W, H);
    gl.uniform1f(prog.u.uTime, S.T);
    gl.uniform4fv(prog.u.uHands, S.hands);
    gl.uniform4fv(prog.u.uHandK, S.handK);
    gl.uniform4fv(prog.u.uDrop, S.drop);
    gl.uniform1f(prog.u.uDropBlur, S.dropBlur);
    gl.uniform4fv(prog.u.uWipe, S.wipe);
    gl.uniform2fv(prog.u.uIris, S.iris);
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, minTex); gl.uniform1i(prog.u.uMinTex, 0);
    gl.activeTexture(gl.TEXTURE1); gl.bindTexture(gl.TEXTURE_2D, hourTex); gl.uniform1i(prog.u.uHourTex, 1);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  // Warm-up: run every shader path once on a single pixel so the first visible frame
  // doesn't pay for shader compilation and texture upload.
  function warmUp() {
    const keep = S;
    S = { ...keep, hands: [0.1, 0.02, 0.05, 0.004], handK: [1, 0.6, 0.03, 0], drop: [0, 0, 0.5, 0.01],
      dropBlur: 0.01, wipe: [0.3, 0, 1, 0.02], iris: [0.1, 0.01] };
    gl.enable(gl.SCISSOR_TEST);
    gl.scissor(0, 0, 1, 1);
    render();
    gl.disable(gl.SCISSOR_TEST);
    gl.finish();
    S = keep;
  }
  warmUp();

  // ---- input: hold to fast-forward
  const isControl = (e) => e.target.closest && e.target.closest('a, button');
  function setHolding(on) {
    S.holding = on && !S.done;
    intro.classList.toggle('is-holding', S.holding);
  }
  intro.addEventListener('pointerdown', (e) => {
    if (isControl(e) || S.revealed) return;
    setHolding(true);
    intro.setPointerCapture?.(e.pointerId);
  });
  const release = () => setHolding(false);
  intro.addEventListener('pointerup', release);
  intro.addEventListener('pointercancel', release);
  addEventListener('keydown', (e) => {
    if (S.revealed) return;
    if (e.code === 'Space' && !e.repeat && !isControl(e)) { e.preventDefault(); setHolding(true); }
    if (e.key === 'Escape') skip();
  });
  addEventListener('keyup', (e) => { if (e.code === 'Space') setHolding(false); });

  function skip() {
    if (S.revealed) return;
    while (!S.done) step(1 / 30);
    S.T = Math.max(S.T, S.Te + FOLD0 + FOLD);
    S.first = true;
  }

  // ---- run
  let running = false, last = 0, frameAvg = 1 / 60, started = false;
  function frame(now) {
    if (S.gone) { running = false; return; }
    // the clock starts on the first real frame, never with a catch-up jump
    if (!started) { started = true; last = now; }
    const real = (now - last) / 1000;
    last = now;
    const dt = Math.min(real, 1 / 30);

    // adaptive resolution: step down when frames run long, back up when there is headroom
    if (real > 0 && real < 0.1) frameAvg += (real - frameAvg) * 0.1;
    if (S.T - lastQualityChange > 0.4) {
      if (frameAvg > 1 / 45 && quality > 0.5) { quality = Math.max(0.5, quality * 0.85); lastQualityChange = S.T; }
      else if (frameAvg < 1 / 70 && quality < 1) { quality = Math.min(1, quality / 0.85); lastQualityChange = S.T; }
    }

    S.rate += ((S.holding ? 4 : 1) - S.rate) * (1 - Math.exp(-dt * 6));
    step(dt * S.rate);
    render();
    requestAnimationFrame(frame);
  }
  function run() {
    if (running) return;
    running = true; started = false;
    requestAnimationFrame(frame);
  }

  $('#replay').addEventListener('click', () => {
    scrollTo(0, 0);
    reset();
    run();
  });

  intro.classList.add('is-started');
  if (FREEZE != null) {
    while (S.T < FREEZE) step(1 / 60);
    const still = () => { render(); requestAnimationFrame(still); };
    still();
    return;
  }
  run();
}

if (!gl) goStatic();
else start().catch((e) => { console.error(e); goStatic(); });
