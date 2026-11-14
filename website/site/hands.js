// The two Victorian hands from the intro, as SVG.
// Units: 1000 = one hand-unit, pivot at the origin, tips pointing up (−y).
// Every shape is a single even-odd path so the piercings stay transparent on any background.

const poly = (pts) => 'M' + pts.map((p) => p.join(',')).join('L') + 'Z';
const ring = (cx, cy, r, t) => circle(cx, cy, r) + circle(cx, cy, r - t);
function circle(cx, cy, r) {
  return `M${cx - r},${cy}a${r},${r} 0 1,0 ${2 * r},0a${r},${r} 0 1,0 ${-2 * r},0Z`;
}
function ellipse(cx, cy, rx, ry, deg) {
  const a = (deg * Math.PI) / 180;
  const dx = rx * Math.cos(a), dy = rx * Math.sin(a);
  return `M${cx + dx},${cy + dy}A${rx},${ry} ${deg} 1,0 ${cx - dx},${cy - dy}A${rx},${ry} ${deg} 1,0 ${cx + dx},${cy + dy}Z`;
}

export const MINUTE_PATH = [
  poly([[-9, 0], [-6, 112], [6, 112], [9, 0]]),
  ring(0, 146, 35, 11),
  poly([[-14, 0], [-4.5, -502], [4.5, -502], [14, 0]]),
  ring(0, -546, 46, 11),
  poly([[-4, -588], [-2.4, -740], [2.4, -740], [4, -588]]),
  'M0,-845Q4,-792 17,-768Q4,-750 0,-726Q-4,-750 -17,-768Q-4,-792 0,-845Z',
].join('');

export const HOUR_PATH = [
  poly([[-11, 0], [-7, 100], [7, 100], [11, 0]]),
  ring(0, 132, 34, 12),
  poly([[-20, 0], [-9, -302], [9, -302], [20, 0]]),
  'M0,-560C12,-520 83,-470 75,-400C68,-340 30,-325 12,-290L-12,-290C-30,-325 -68,-340 -75,-400C-83,-470 -12,-520 0,-560Z',
  ellipse(35, -402, 15, 30, -26),
  ellipse(-35, -402, 15, 30, 26),
  'M0,-478L8,-452L0,-426L-8,-452Z',
].join('');

export const PIVOT_PATH = circle(0, 0, 40) + circle(0, 0, 15);

/** Markup for a pair of hands. `size` is the visible radius in hand-units. */
export function handsSVG({ size = 900, label = '', className = '' } = {}) {
  const aria = label ? `role="img" aria-label="${label}"` : 'aria-hidden="true"';
  return `<svg class="hands ${className}" viewBox="${-size} ${-size} ${2 * size} ${2 * size}" ${aria}>
    <g class="hands-hour"><path fill-rule="evenodd" d="${HOUR_PATH}"/></g>
    <g class="hands-min"><path fill-rule="evenodd" d="${MINUTE_PATH}"/></g>
    <path fill-rule="evenodd" d="${PIVOT_PATH}"/>
  </svg>`;
}

/** Point a pair of hands at a time given in minutes. */
export function setHands(svg, minutes) {
  svg.querySelector('.hands-min').setAttribute('transform', `rotate(${((minutes / 60) * 360) % 360})`);
  svg.querySelector('.hands-hour').setAttribute('transform', `rotate(${((minutes / 720) * 360) % 360})`);
}
