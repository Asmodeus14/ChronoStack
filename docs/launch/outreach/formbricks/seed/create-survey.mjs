// Creates a live link survey through the management API with a close date
// picked 3 days ahead (in the scheduling time zone, Pacific/Auckland), sent the
// way the survey editor sends it: toDateOnlySelection() turns the picked day
// into noon UTC. Prints the picked date (YYYY-MM-DD).
//   node /seed/create-survey.mjs
const picked = new Intl.DateTimeFormat("en-CA", { timeZone: "Pacific/Auckland" }).format(
  new Date(Date.now() + 3 * 24 * 60 * 60 * 1000)
);
const closeOn = `${picked}T12:00:00.000Z`;

// Wait for the web server (it may still be starting).
for (let i = 0; i < 60; i++) {
  try {
    await fetch("http://localhost:3000/health");
    break;
  } catch {
    await new Promise((r) => setTimeout(r, 2000));
  }
}

const res = await fetch("http://localhost:3000/api/v1/management/surveys", {
  method: "POST",
  headers: { "x-api-key": "fbk_chronostacktestsecret0123456789abcdef", "content-type": "application/json" },
  body: JSON.stringify({
    workspaceId: "cstestworkspace00000000001",
    name: `Closes on ${picked}`,
    type: "link",
    status: "inProgress",
    closeOn,
    questions: [
      {
        id: "q1",
        type: "openText",
        headline: { default: "Anything to add?" },
        required: false,
        inputType: "text",
        charLimit: { enabled: false },
      },
    ],
  }),
});
if (res.status !== 200) {
  console.error(res.status, await res.text());
  process.exit(1);
}
console.log(picked);
