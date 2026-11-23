# Demo video (1920x1080, 30 fps, H.264) for the README and launch posts.
# Rendered by .github/workflows/demo-video.yml with the published release CLI,
# against the demo app (examples/saas-trial-demo). Nothing is staged: `Wait`
# holds the recording until the real output appears.
Output docs/assets/demo/chronostack-demo.mp4

Require chronostack

Set Shell "bash"
Set FontSize 24
Set Width 1920
Set Height 1080
Set Padding 40
Set Framerate 30
Set TypingSpeed 35ms
Set Theme "Catppuccin Mocha"
Set WaitTimeout 240s

Hide
Type `cd examples/saas-trial-demo && export PS1='$ ' BUILDKIT_PROGRESS=quiet COMPOSE_PROGRESS=quiet && clear`
Enter
Sleep 500ms
Show

Type "# ChronoStack: move time forward for a whole Docker Compose stack."
Enter
Sleep 700ms
Type "chronostack --version"
Enter
Wait+Screen /chronostack \d/
Sleep 1500ms

Type "# Postgres, Redis, an API, a worker and a webhook receiver, on one virtual clock:"
Enter
Sleep 500ms
Type "chronostack up --wait"
Enter
Wait+Screen /is up\. Try/
Sleep 3s

Hide
Type "clear"
Enter
Sleep 300ms
Show
Type "# A 14-day trial. A daily 03:00 job. The partner's webhook is down on day 14, 02:00-06:00."
Enter
Sleep 900ms
Type "chronostack run scenarios/trial.yml"
Enter
Wait+Screen /(PASSED|FAILED) .* steps/
Sleep 2500ms

Hide
Type "clear"
Enter
Sleep 300ms
Show
Type "chronostack report show"
Enter
Wait+Screen /12\. it was delivered/
Sleep 1500ms
Screenshot docs/assets/demo/chronostack-demo-poster.png
Sleep 5s

Hide
Type `clear && printf '\n\n\n\n\n\n\n\n\n            ChronoStack\n\n            Move time forward for your whole Docker Compose stack:\n            Postgres, Redis and your services, with no code changes.\n\n            GitHub: github.com/Asmodeus14/ChronoStack\n'`
Enter
Sleep 300ms
Show
Sleep 4s
