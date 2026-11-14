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
const GROW0 = 0.35, GROW1 = 1.15;   // the hands unfold from the pivot
const DROP0 = 1.3, DROP1 = 2.2;     // a drop of white floods the screen
const RUN0 = 2.15, RUN = 3.6;       // the advance: three sweeps, each one inverts the screen
const SWEEPS = 3;
const TOTAL = 14 * 1440 + 180;      // +14d 03:00 in minutes; the hour hand ends on III

const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const expoOut = (x) => (x >= 1 ? 1 : 1 - Math.pow(2, -10 * clamp(x)));
const cubicIn = (x) => clamp(x) ** 3;
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
const VS = `#version 300 es
in vec2 aPos;
void main(){ gl_Position = vec4(aPos, 0., 1.); }`;

const FS = `#version 300 es
precision highp float;
out vec4 o;
uniform vec2 uRes;
uniform float uTime;
uniform sampler2D uMinTex, uHourTex;
uniform vec4 uHands;   // minute rev, hour rev, minute delta, hour delta
uniform vec4 uHandK;   // length (0..1), size in scene units, pivot radius, -
uniform vec4 uDrop;    // cx, cy, radius, wobble
uniform vec3 uWipe;    // sweep fraction, base parity, wipe on
uniform vec3 uCursor;  // x, y, radius
const float TAU = 6.28318530718;
const vec3 INK = vec3(.043, .047, .055);
const vec3 PAPER = vec3(.957, .949, .925);
const float ML=${MINUTE.len}, MT=${MINUTE.tail}, MW=${MINUTE.halfW};
const float HL=${HOUR.len}, HT=${HOUR.tail}, HW=${HOUR.halfW};

float hash(vec2 p){ p=fract(p*vec2(123.34,456.21)); p+=dot(p,p+45.32); return fract(p.x*p.y); }
// soft XOR of two coverages: keeps the antialiased edges
float xr(float a, float b){ return a + b - 2.*a*b; }

float hand(sampler2D t, vec2 q, float a, float g, float L, float T, float W){
  vec2 d = vec2(sin(a), cos(a));
  vec2 uv = vec2(dot(q, vec2(d.y,-d.x))/(2.*W) + .5, (dot(q,d)/g + T)/(L+T));
  if (uv.x<0. || uv.x>1. || uv.y<0. || uv.y>1.) return 0.;
  return texture(t, uv).a;
}
float handBlur(sampler2D t, vec2 q, float rev, float drev, float g, float L, float T, float W, float jit){
  float span = clamp(drev, -1., 1.);
  int n = int(clamp(ceil(abs(span)*160.), 1., 28.));
  float acc = 0.;
  for (int i=0; i<28; i++){
    if (i>=n) break;
    acc += hand(t, q, (rev - span*(float(i)+jit)/float(n))*TAU, g, L, T, W);
  }
  return min(1., acc/float(n)*(1. + 2.5*min(abs(span), 1.)));
}

void main(){
  vec2 p = (gl_FragCoord.xy - .5*uRes)/uRes.y*2.;
  float px = 2./uRes.y;
  float r = length(p);
  float v = uWipe.y;

  // the drop: a circle of the opposite colour with a liquid edge
  if (uDrop.z > 0.){
    vec2 d = p - uDrop.xy;
    float a = atan(d.y, d.x);
    float edge = uDrop.z + uDrop.w*(.6*sin(a*5. + uTime*3.1) + .4*sin(a*9. - uTime*4.3));
    v = xr(v, 1. - smoothstep(edge - px, edge + px, length(d)));
  }

  // the sweep: everything the minute hand has passed this turn is inverted
  if (uWipe.z > 0.){
    float ang = fract(atan(p.x, p.y)/TAU);
    float w = smoothstep(-px, px, (uWipe.x - ang)*TAU*r) * smoothstep(-px, px, ang*TAU*r);
    v = xr(v, w);
  }

  // the hands and the pivot
  if (uHandK.x > .001){
    vec2 q = p/uHandK.y;
    float jit = hash(gl_FragCoord.xy);
    float hh = handBlur(uHourTex, q, uHands.y, uHands.w, uHandK.x, HL, HT, HW, jit);
    float mm = handBlur(uMinTex,  q, uHands.x, uHands.z, uHandK.x, ML, MT, MW, jit);
    v = xr(v, max(hh, mm));
  }
  if (uHandK.z > 0.){
    float pr = uHandK.z;
    float pv = 1. - smoothstep(pr - px, pr + px, r);
    float hole = 1. - smoothstep(pr*.38 - px, pr*.38 + px, r);
    v = xr(v, pv - hole);
  }

  // the cursor inverts too
  if (uCursor.z > 0.){
    v = xr(v, 1. - smoothstep(uCursor.z - px, uCursor.z + px, length(p - uCursor.xy)));
  }

  vec3 col = mix(INK, PAPER, v);
  col += (hash(gl_FragCoord.xy + fract(uTime*7.31)*91.7) - .5)*.035;
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

  const minTex = texture(drawHand(MINUTE, 192, drawMinute));
  const hourTex = texture(drawHand(HOUR, 256, drawHour));

  let W = 0, H = 0;
  function resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.round(innerWidth * dpr), h = Math.round(innerHeight * dpr);
    if (w === W && h === H) return;
    W = canvas.width = w; H = canvas.height = h;
    gl.viewport(0, 0, W, H);
  }
  resize();
  addEventListener('resize', resize);

  // ---- state
  let S;
  const cursor = { x: 99, y: 99, r: 0, on: false };
  function reset() {
    S = {
      T: 0, rate: 1, holding: false, p: 0, done: false, Te: 0, revealed: false,
      prevMin: 0, prevHour: 0, first: true,
      hands: [0, 0, 0, 0], handK: [0, 0.5, 0, 0], drop: [0, 0, 0, 0], wipe: [0, 0, 0], v: 0,
    };
    intro.classList.remove('is-revealed', 'is-holding', 'is-running');
    root.classList.remove('intro-done');
    root.classList.add('is-locked');
    status.textContent = 'Loading ChronoStack…';
    announce('chronostack:intro-start');
  }
  reset();

  function step(dt) {
    S.T += dt;
    const t = S.T;
    const aspect = innerWidth / innerHeight;
    const size = Math.min(0.92, aspect * 0.8);          // hand scale, in half-screen-heights
    const cover = Math.hypot(aspect, 1) * 1.08;            // radius that covers the screen
    intro.classList.toggle('is-running', t > DROP0);

    // the advance
    if (t > RUN0 && !S.done) S.p = Math.min(1, S.p + dt / RUN);
    const rev = SWEEPS * cubicInOut(S.p);
    const minRev = rev, hourRev = rev / 12;              // ends on 12 and III
    const dMin = S.first ? 0 : minRev - S.prevMin;
    const dHour = S.first ? 0 : hourRev - S.prevHour;
    S.prevMin = minRev; S.prevHour = hourRev; S.first = false;
    S.v = TOTAL * cubicInOut(S.p);

    if (!S.done && S.p >= 1) { S.done = true; S.Te = t; }
    const e = S.done ? t - S.Te : 0;

    // unfold, then fold away at the end
    let g = expoOut((t - GROW0) / (GROW1 - GROW0));
    if (S.done) g *= 1 - cubicIn((e - 0.15) / 0.55);
    let pivot = 0.032 * cubicOut((t - GROW0) / 0.5);
    if (S.done) pivot *= 1 - cubicIn((e - 0.55) / 0.45);
    S.hands = [fract(minRev), fract(hourRev), dMin, dHour];
    S.handK = [g, size, pivot, 0];

    // the drop: grows from the pivot to cover the screen, then becomes the new background
    const k = (t - DROP0) / (DROP1 - DROP0);
    let parity = 0;
    if (k >= 1) { parity = 1; S.drop = [0, 0, 0, 0]; }
    else if (k > 0) {
      const rad = cover * Math.pow(cubicOut(k), 1.15);
      S.drop = [0, 0, Math.max(rad, 0.0001), 0.012 * (1 - k) * Math.min(1, k * 6)];
    } else S.drop = [0, 0, 0, 0];

    // each full sweep flips the background; the current sweep is drawn as a wedge
    const turns = Math.floor(rev + 1e-6);
    S.wipe = [fract(rev), (parity + turns) % 2, k >= 1 && !S.done ? 1 : 0];

    if (S.done && !S.revealed && e > 0.9) {
      S.revealed = true;
      intro.classList.add('is-revealed');
      root.classList.add('intro-done');
      root.classList.remove('is-locked');
      status.textContent = 'ChronoStack loaded.';
      announce('chronostack:intro-done');
    }

  }

  function fmtTime(v) {
    const m = Math.max(0, Math.floor(v + 1e-6));
    const d = Math.floor(m / 1440), h = Math.floor((m % 1440) / 60), mm = m % 60;
    return `+${String(d).padStart(2, '0')}d ${String(h).padStart(2, '0')}:${String(mm).padStart(2, '0')}`;
  }
  function updateDom() {}

  function render() {
    resize();
    gl.useProgram(prog.p);
    gl.uniform2f(prog.u.uRes, W, H);
    gl.uniform1f(prog.u.uTime, S.T);
    gl.uniform4fv(prog.u.uHands, S.hands);
    gl.uniform4fv(prog.u.uHandK, S.handK);
    gl.uniform4fv(prog.u.uDrop, S.drop);
    gl.uniform3fv(prog.u.uWipe, S.wipe);
    gl.uniform3f(prog.u.uCursor, cursor.x, cursor.y, cursor.r);
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, minTex); gl.uniform1i(prog.u.uMinTex, 0);
    gl.activeTexture(gl.TEXTURE1); gl.bindTexture(gl.TEXTURE_2D, hourTex); gl.uniform1i(prog.u.uHourTex, 1);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  // ---- input: hold to fast-forward
  const toScene = (e) => [((e.clientX - innerWidth / 2) / innerHeight) * 2, (-(e.clientY - innerHeight / 2) / innerHeight) * 2];
  const isControl = (e) => e.target.closest && e.target.closest('a, button');
  function setHolding(on) {
    S.holding = on && !S.done;
    intro.classList.toggle('is-holding', S.holding);
  }
  intro.addEventListener('pointerdown', (e) => {
    if (isControl(e)) return;
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
    S.T = Math.max(S.T, S.Te + 0.5);
    S.first = true;
  }
  $('#replay').addEventListener('click', () => {
    scrollTo(0, 0);
    reset();
    if (!running) { running = true; last = performance.now(); requestAnimationFrame(frame); }
  });

  // ---- run
  intro.classList.add('is-started');
  if (FREEZE != null) {
    while (S.T < FREEZE) step(1 / 60);
    const still = () => { step(0); updateDom(); render(); requestAnimationFrame(still); };
    still();
    return;
  }
  let last = performance.now();
  let running = true;
  function frame(now) {
    // once the overlay has faded, stop drawing
    if (S.revealed && S.T - S.Te > 2.5) { running = false; return; }
    const dt = Math.min((now - last) / 1000, 1 / 20);
    last = now;
    S.rate += ((S.holding ? 4 : 1) - S.rate) * (1 - Math.exp(-dt * 6));
    step(dt * S.rate);
    updateDom();
    render();
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}

if (!gl) goStatic();
else start().catch((e) => { console.error(e); goStatic(); });
