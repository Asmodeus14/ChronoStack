// Drives tududi's API like the web app does. Run inside the tududi container:
//   node /seed/api.mjs setup                  -> creates the tasks, prints "<uidMonthly1st> <uidLastFriday>"
//   node /seed/api.mjs due <uid>              -> prints the task's due date as the user sees it (YYYY-MM-DD)
//   node /seed/api.mjs complete <uid>         -> marks it done (tududi moves it to the next due date), prints the new due date
//   node /seed/api.mjs due|complete rent|retro -> the same, by the tasks created in setup
// The user is in America/New_York.
const BASE = "http://localhost:3002/api";
let cookie = "";
let csrf = "";

async function call(method, path, body) {
  const res = await fetch(BASE + path, {
    method,
    headers: { "content-type": "application/json", cookie, ...(csrf ? { "x-csrf-token": csrf } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  const set = res.headers.get("set-cookie");
  if (set) cookie = set.split(";")[0];
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status} ${text.slice(0, 300)}`);
  return text ? JSON.parse(text) : null;
}

async function login() {
  for (let i = 0; i < 60; i++) {
    try {
      await call("POST", "/login", { email: "owner@example.com", password: "chronostack-test-password" });
      const t = await call("GET", "/csrf-token");
      csrf = t.csrfToken ?? t.token ?? "";
      return;
    } catch (e) {
      if (i === 59) throw e;
      await new Promise((r) => setTimeout(r, 2000));
    }
  }
}

const dueOf = async (uid) => String((await call("GET", `/task/${uid}`)).due_date).slice(0, 10);

import { readFileSync, writeFileSync } from "node:fs";
const IDS = "/tmp/tududi-task-uids.json";
const [cmd, which] = process.argv.slice(2);
const uid = which === "rent" || which === "retro" ? JSON.parse(readFileSync(IDS, "utf8"))[which] : which;
await login();
if (cmd === "setup") {
  await call("PATCH", "/profile", { timezone: "America/New_York" });
  const a = await call("POST", "/task", {
    name: "Pay rent (monthly on the 1st)", due_date: "2026-12-01",
    recurrence_type: "monthly", recurrence_interval: 1, recurrence_month_day: 1,
  });
  const b = await call("POST", "/task", {
    name: "Team retro (last Friday of the month)", due_date: "2027-01-29",
    recurrence_type: "monthly_weekday", recurrence_interval: 1, recurrence_weekday: 5, recurrence_week_of_month: 5,
  });
  const ids = { rent: a.uid ?? a.task?.uid, retro: b.uid ?? b.task?.uid };
  writeFileSync(IDS, JSON.stringify(ids));
  console.log(`${ids.rent} ${ids.retro}`);
} else if (cmd === "due") {
  console.log(await dueOf(uid));
} else if (cmd === "complete") {
  await call("PATCH", `/task/${uid}`, { status: "done" });
  console.log(await dueOf(uid));
} else {
  throw new Error("usage: setup | due <uid> | complete <uid>");
}
