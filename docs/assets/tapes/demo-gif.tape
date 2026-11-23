# Short looping GIF of the same scenario (the stack is already up).
# Rendered by .github/workflows/demo-video.yml after demo.tape.
Output docs/assets/demo/chronostack-demo.gif

Require chronostack

Set Shell "bash"
Set FontSize 16
Set Width 1200
Set Height 720
Set Padding 20
Set Framerate 12
Set TypingSpeed 35ms
Set Theme "Catppuccin Mocha"
Set WaitTimeout 240s

Hide
Type `cd examples/saas-trial-demo && export PS1='$ ' && clear`
Enter
Sleep 500ms
Show

Type "# 15 virtual days: a 14-day trial, a daily 03:00 job, a webhook outage on day 14."
Enter
Sleep 700ms
Type "chronostack run scenarios/trial.yml"
Enter
Wait+Screen /(PASSED|FAILED) .* steps/
Sleep 1500ms
Hide
Type "clear"
Enter
Sleep 300ms
Show
Type "chronostack report show"
Enter
Wait+Screen /12\. it was delivered/
Sleep 4s
