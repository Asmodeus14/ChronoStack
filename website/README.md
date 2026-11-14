# ChronoStack website

The landing page at https://chronostack-dev.vercel.app. Static HTML, CSS and JavaScript, no build step.

- `index.html`: the page and its metadata (Open Graph, Twitter card, JSON-LD).
- `intro/`: the black-and-white WebGL intro (two Victorian clock hands; every sweep inverts the screen).
- `site/`: page styles, scroll animations (GSAP + ScrollTrigger + Lenis from CDNs), the inverting cursor, and the clock hands as SVG.
- `vercel.json`: headers and caching. The Vercel project's root directory is `website`.

Run it locally with any static server, for example `python -m http.server 4321` in this folder.

Debug parameters: `?t=3.6` freezes the intro at 3.6 s, `?nointro` skips it.
