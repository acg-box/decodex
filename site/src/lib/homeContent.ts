const decodexGitHubUrl = "https://github.com/acg-box/decodex";

const productLoops = [
  {
    title: "Project registry",
    body: "Explicit service configs, workflow policy, identity routing, and queue eligibility.",
  },
  {
    title: "Retained lanes",
    body: "Issue intake, attempt state, progress evidence, and recovery paths for long-running work.",
  },
  {
    title: "Operator surface",
    body: "Local status, account-pool controls, lane inspection, interrupts, and steer requests.",
  },
  {
    title: "Delivery policy",
    body: "Commit, review handoff, landing, closeout, and cleanup stay tied to repository authority.",
  },
];

const commands = [
  "decodex serve --listen-address 127.0.0.1:8192",
  "decodex status --live",
  "decodex diagnose --json",
  "decodex lane steer <ISSUE> --run-id <RUN_ID> --expected-turn-id <TURN_ID> --message <TEXT>",
];

const docs = [
  {
    title: "Runtime contract",
    href: `${decodexGitHubUrl}/blob/main/openwiki/specs/contracts-and-data.md`,
  },
  {
    title: "Operator control",
    href: `${decodexGitHubUrl}/blob/main/openwiki/workflows/runtime-operator-workflows.md`,
  },
  {
    title: "Project workflow",
    href: `${decodexGitHubUrl}/blob/main/openwiki/architecture/runtime-architecture.md`,
  },
];

export { commands, decodexGitHubUrl, docs, productLoops };
