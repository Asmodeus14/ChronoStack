# README hero GIF: a trial expires after 15 virtual days.
# Rendered by .github/workflows/demo-gif.yml against a running demo stack.
Output docs/assets/quickstart.gif

Set Shell "bash"
Set FontSize 16
Set Width 1100
Set Height 640
Set Padding 16
Set Framerate 10
Set TypingSpeed 35ms
Set Theme "Catppuccin Mocha"

# Off camera: CLI on PATH, demo folder, two small helpers that talk to the demo app.
Hide
Type `export PATH="$PWD/target/debug:$PATH" && cd examples/saas-trial-demo && export PS1='$ '`
Enter
Type `signup() { docker exec saas-trial-demo-chronostack-api-1 node scripts/call.js signup "$1" >/dev/null && echo "signed up $1 (14-day trial)"; }`
Enter
Type `users() { docker exec saas-trial-demo-chronostack-db-1 psql -U app -d app -c "select email, plan, trial_ends_at::date as trial_ends, downgraded_at::timestamp(0) as downgraded_at from users"; }`
Enter
Type "clear"
Enter
Sleep 300ms
Show

Type "# Postgres, Redis, an API and a worker: all on one virtual clock."
Enter
Sleep 600ms
Type "chronostack now"
Enter
Sleep 1500ms
Type "signup alice@example.com"
Enter
Sleep 1500ms
Type "# Jump 15 days. The daily 03:00 job runs at every virtual 03:00:"
Enter
Sleep 600ms
Type "chronostack advance 15d"
Enter
Sleep 26s
Type "users"
Enter
Sleep 5s
