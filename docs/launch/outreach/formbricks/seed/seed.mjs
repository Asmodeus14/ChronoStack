// Seeds an organization, an owner, a workspace and a write API key with
// Formbricks' own Prisma client, then projects them into SpiceDB with the
// image's `formbricks-authzed backfill`. Run inside the formbricks container:
//   node /seed/seed.mjs
// Throwaway test credentials; the API key is fbk_chronostacktestsecret0123456789abcdef.
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { prisma } = require("/home/nextjs/packages/database/dist/index.cjs");

const ORG = "cstestorganization00000001";
const USER = "cstestuser0000000000000001";
const WORKSPACE = "cstestworkspace00000000001";
const API_KEY = "cstestapikey00000000000001";

await prisma.user.upsert({
  where: { id: USER },
  update: {},
  create: { id: USER, name: "Test Owner", email: "owner@example.com" },
});
await prisma.organization.upsert({
  where: { id: ORG },
  update: {},
  create: { id: ORG, name: "Test Organization" },
});
await prisma.membership.upsert({
  where: { userId_organizationId: { userId: USER, organizationId: ORG } },
  update: {},
  create: { userId: USER, organizationId: ORG, role: "owner", accepted: true },
});
await prisma.workspace.upsert({
  where: { id: WORKSPACE },
  update: {},
  create: { id: WORKSPACE, name: "Test Workspace", organizationId: ORG },
});
await prisma.apiKey.upsert({
  where: { id: API_KEY },
  update: {},
  create: {
    id: API_KEY,
    label: "test",
    organizationId: ORG,
    lookupHash: "0014ed0eb68168a314e4ac067b51d43c3f41ef7f2ec692a6572eb6b4240a6208",
    hashedKey: "$2b$12$02MX0czyYZpx2tk6dbWsZeCkZ2nt3si.zpM1YWMmHQvQi/Lpqn/46",
    apiKeyWorkspaces: { create: { workspaceId: WORKSPACE, permission: "write" } },
  },
});
await prisma.$disconnect();

const out = execFileSync("formbricks-authzed", ["backfill", "--apply", `--organization-id=${ORG}`], {
  encoding: "utf8",
});
console.error(out.trim());
console.log(WORKSPACE);
