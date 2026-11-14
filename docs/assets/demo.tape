# Terminal recording for the README (charmbracelet/vhs).
# Rendered by .github/workflows/demo-gif.yml against a running demo stack;
# the stack is started before recording so the GIF shows only ChronoStack.
Output docs/assets/demo.gif

Set Shell "bash"
Set FontSize 15
Set Width 1280
Set Height 760
Set Padding 18
Set TypingSpeed 45ms
Set Theme "Catppuccin Mocha"

Hide
Type `export PATH="$PWD/target/debug:$PATH" && cd examples/saas-trial-demo && export PS1='$ ' && clear`
Enter
Sleep 500ms
Show

Type "# A SaaS app: Postgres, Redis, API, worker. 14-day trials, a daily 03:00 job, webhooks."
Enter
Sleep 1s
Type "chronostack now"
Enter
Sleep 2s
Type "# Run 15 virtual days with a webhook outage on day 14, and check the results:"
Enter
Sleep 1s
Type "chronostack run scenarios/trial.yml 2>&1 | tee /tmp/demo-run.log"
Enter
Sleep 45s
Sleep 4s
