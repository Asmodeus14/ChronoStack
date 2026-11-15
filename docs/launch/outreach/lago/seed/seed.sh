#!/bin/sh
# Creates plans, customers and subscriptions through Lago's REST API.
# Run inside the api container: sh /seed/seed.sh
# Subscriptions start in December 2026 (before the scenario's run to March 2028),
# on the month-end anniversary days that make billing periods hardest.
set -e
API=http://localhost:3000/api/v1
AUTH="Authorization: Bearer chronostack-test-api-key"

post() {
  out=$(curl -s -w '\n%{http_code}' -X POST -H "$AUTH" -H 'Content-Type: application/json' "$API/$1" -d "$2")
  code=$(echo "$out" | tail -n 1)
  case "$code" in 2*) ;; *) echo "POST $1 failed ($code): $(echo "$out" | head -n -1)" >&2; exit 1 ;; esac
}

# Wait for the API.
i=0; until curl -sf "$API/../../health" >/dev/null 2>&1 || [ $i -ge 60 ]; do i=$((i+1)); sleep 2; done

for p in monthly quarterly yearly; do
  post plans "{\"plan\":{\"name\":\"$p\",\"code\":\"$p\",\"interval\":\"$p\",\"amount_cents\":1000,\"amount_currency\":\"USD\",\"pay_in_advance\":false}}"
done

post customers '{"customer":{"external_id":"c-utc","name":"UTC customer","currency":"USD","timezone":"UTC"}}'
post customers '{"customer":{"external_id":"c-ny","name":"New York customer","currency":"USD","timezone":"America/New_York"}}'
post customers '{"customer":{"external_id":"c-syd","name":"Sydney customer","currency":"USD","timezone":"Australia/Sydney"}}'

# external_id, customer, plan, billing_time, subscription_at (noon local time)
sub() {
  post subscriptions "{\"subscription\":{\"external_id\":\"$1\",\"external_customer_id\":\"$2\",\"plan_code\":\"$3\",\"billing_time\":\"$4\",\"subscription_at\":\"$5\"}}"
}
sub m31-utc c-utc monthly anniversary 2026-12-31T12:00:00Z
sub m30-ny  c-ny  monthly anniversary 2026-12-30T17:00:00Z
sub m29-syd c-syd monthly anniversary 2026-12-29T01:00:00Z
sub cal-ny  c-ny  monthly calendar    2026-12-15T17:00:00Z
sub q30-utc c-utc quarterly anniversary 2026-11-30T12:00:00Z
sub y31-utc c-utc yearly anniversary  2026-12-31T12:00:00Z

echo seeded
