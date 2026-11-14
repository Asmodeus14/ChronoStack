// The inverting cursor from the intro, everywhere.
// One dot with mix-blend-mode: difference, so it is white on black and black on white.
// Mouse and trackpad only; touch devices keep their own behaviour.

const FINE = matchMedia('(pointer: fine)').matches;
const REDUCED = matchMedia('(prefers-reduced-motion: reduce)').matches;

if (FINE) {
  const root = document.documentElement;
  const dot = document.createElement('div');
  dot.className = 'cursor';
  dot.setAttribute('aria-hidden', 'true');
  document.body.appendChild(dot);
  root.classList.add('has-cursor');

  const intro = document.getElementById('intro');
  const INTERACTIVE = 'a, button, [role="tab"], summary, label, input, select, textarea';
  const SIZE = { rest: 14, hover: 54, hold: 64, press: 10 };

  let x = innerWidth / 2, y = innerHeight / 2, tx = x, ty = y;
  let size = 0, seen = false, hover = false, down = false;

  addEventListener('pointermove', (e) => {
    if (e.pointerType !== 'mouse') return;
    tx = e.clientX; ty = e.clientY;
    if (!seen) { x = tx; y = ty; seen = true; }
    hover = !!(e.target.closest && e.target.closest(INTERACTIVE));
  }, { passive: true });
  addEventListener('pointerdown', (e) => { if (e.pointerType === 'mouse') down = true; });
  addEventListener('pointerup', () => { down = false; });
  document.addEventListener('pointerleave', () => { seen = false; });
  addEventListener('blur', () => { seen = false; });

  let last = performance.now();
  function frame(now) {
    const dt = Math.min((now - last) / 1000, 0.05);
    last = now;
    const holding = intro && intro.classList.contains('is-holding');
    const target = !seen ? 0
      : holding ? SIZE.hold
      : down ? SIZE.press
      : hover ? SIZE.hover
      : SIZE.rest;
    const follow = REDUCED ? 1 : 1 - Math.exp(-dt * 28);
    const grow = REDUCED ? 1 : 1 - Math.exp(-dt * 16);
    x += (tx - x) * follow;
    y += (ty - y) * follow;
    size += (target - size) * grow;
    dot.style.transform = `translate3d(${x}px, ${y}px, 0) translate(-50%, -50%)`;
    dot.style.width = dot.style.height = `${size.toFixed(2)}px`;
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}
