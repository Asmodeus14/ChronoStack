# README GIF: a regression makes the scenario fail, and the report says why.
# Rendered by .github/workflows/demo-gif.yml against the demo started with
# compose.bug.yml (the worker treats a 503 as a delivered webhook).
Output docs/assets/failure.gif

Set Shell "bash"
Set FontSize 15
Set Width 1150
Set Height 700
Set Padding 16
Set Framerate 10
Set TypingSpeed 35ms
Set Theme "Catppuccin Mocha"

Hide
Type `export PATH="$PWD/target/debug:$PATH" && cd examples/saas-trial-demo && export PS1='$ '`
Enter
Type `report() { f=$(ls -t .chronostack/reports/*.md | head -1); sed -n '/^## What failed/,/^The steps after/p;/^## Requests/,/^## Outage/p' "$f" | grep -v '^## Outage'; }`
Enter
Type "clear"
Enter
Sleep 300ms
Show

Type "# Same scenario, but the worker now has a bug (it treats a 503 as delivered):"
Enter
Sleep 600ms
Type "chronostack run scenarios/trial.yml | tail -6"
Enter
Sleep 30s
Type "report      # the Markdown report ChronoStack wrote"
Enter
Sleep 7s
