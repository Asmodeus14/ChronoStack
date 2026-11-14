# Terminal recording for the README (charmbracelet/vhs).
# Rendered by .github/workflows/demo-gif.yml against a running demo stack;
# the stack is started before recording so the GIF shows only ChronoStack.
Output docs/assets/demo.gif

Set Shell "bash"
Set FontSize 15
Set Width 1200
Set Height 720
Set Padding 16
Set Framerate 12
Set TypingSpeed 40ms
Set Theme "Catppuccin Mocha"

# Off camera: put the CLI on PATH, keep a copy of the output for the workflow log.
Hide
Type `export PATH="$PWD/target/debug:$PATH" && cd examples/saas-trial-demo && export PS1='$ '`
Enter
Type `chronostack() { command chronostack "$@" 2>&1 | tee -a /tmp/demo-run.log; } && clear`
Enter
Sleep 500ms
Show

Type "# A SaaS app: Postgres, Redis, API, worker. 14-day trials, a daily 03:00 job, webhooks."
Enter
Sleep 800ms
Type "chronostack now"
Enter
Sleep 2s
Type "# Run 15 virtual days with a webhook outage on day 14, and check the results:"
Enter
Sleep 800ms
Type "chronostack run scenarios/trial.yml"
Enter
Sleep 32s
