# ChronoStack website

The landing page at https://chronostack-dev.vercel.app. Static HTML, CSS and JavaScript, no build step.

- `index.html`: the page and its metadata (Open Graph, Twitter card, JSON-LD).
- `intro/`: the black-and-white WebGL intro (two Victorian clock hands; every sweep inverts the screen).
- `site/`: page styles, scroll animations (GSAP + ScrollTrigger + Lenis from CDNs), the inverting cursor, the clock hands as SVG, and `try.js` (the simulated terminal in "Try it here").
- `fonts/`: self-hosted IBM Plex Sans, IBM Plex Mono and IM Fell English (latin subset, SIL Open Font License).
- `assets/`: logo, icons, the Open Graph image, and the demo recordings as MP4/WebM with poster frames (made from the GIFs in `docs/assets/`).
- Vercel Web Analytics is loaded from `/_vercel/insights/script.js`; it starts collecting once Analytics is enabled for the project in the Vercel dashboard.
- `vercel.json`: headers and caching. The Vercel project's root directory is `website`.

Run it locally with any static server, for example `python -m http.server 4321` in this folder.

Debug parameters: `?t=3.6` freezes the intro at 3.6 s, `?nointro` skips it.
