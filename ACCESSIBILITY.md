# Accessibility

ChronoStack should be usable by everyone, whether you use a screen reader, the keyboard only, magnification, or reduced motion. This page says what we do today, where we know we fall short, and how to tell us about a problem.

We haven't had a formal audit, so we don't claim conformance with any standard. We aim for [WCAG 2.2](https://www.w3.org/TR/WCAG22/) level AA on the website and docs, and we treat a barrier as a bug.

## The command line

- **Plain text output.** The CLI doesn't use colour or other terminal escape codes. Status is spelled out in words (`ok`, `PASSED`, `FAILED`), so output reads the same in a screen reader, a braille display, a log file or CI.
- **Machine-readable output.** The main commands (`now`, `doctor`, `probe`, `advance`, `run`, `fault list` and others) accept `--json`. Use it if you'd rather pipe results into your own tools than read formatted text.
- **No interactive prompts.** Every command takes its input as arguments or from files (`chronostack.yml`, scenario files), so nothing requires a full-screen terminal UI.
- **Scenario reports are Markdown.** `chronostack run` writes reports as plain Markdown files that you can open in any editor or reader.

## The website

- A **skip link** goes straight to the main content, and the page has labelled landmarks (`header`, `nav`, `main`).
- **Reduced motion is respected.** If your system asks for reduced motion, the animated intro doesn't run and nothing autoplays; the controls still work.
- **The intro can be skipped** with `Esc`, without a mouse.
- **Decorative graphics** (the clock hands, the background canvas) are hidden from screen readers. Buttons that only show an icon have text labels, and the copy button announces when it has copied.
- The page sets its language (`lang="en"`).

## The docs and README

- The demo GIFs in the README have **alt text that describes what happens** in each recording, including the outcome. The same steps are also written out as commands in the text.
- The docs are plain Markdown on GitHub, so they work with GitHub's own accessibility support and any reader that renders Markdown.

## Known gaps

- **The demo recordings are GIFs.** They have no pause control on GitHub. The alt text and written steps cover the same content.
- **Colour contrast and zoom haven't been measured** across the whole website. If something is hard to read at 200% zoom or in high-contrast mode, please tell us.
- **The website hasn't been tested with every screen reader.** Reports from NVDA, JAWS, VoiceOver or TalkBack users are especially welcome.

## Report a barrier

[Open an issue](https://github.com/Asmodeus14/ChronoStack/issues/new/choose) and tell us:

- what you were trying to do and where (CLI command, website section or doc page);
- what got in the way;
- your assistive technology, browser or terminal, and operating system, if relevant.

Accessibility bugs are treated like any other bug, and a fix PR is welcome too: see [CONTRIBUTING.md](CONTRIBUTING.md).
