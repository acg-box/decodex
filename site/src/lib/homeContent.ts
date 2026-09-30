const decodexGitHubUrl = "https://github.com/acg-box/decodex";

const productLoops = [
  {
    title: "Agent coordination",
    body: "Coordinate goals, collect worker results, and keep decisions linked to the work that needs them.",
  },
  {
    title: "Conversations",
    body: "Start or continue Codex threads for direct work, with conversation and turn history.",
  },
  {
    title: "Account routing",
    body: "Choose fixed or balanced routing. Each conversation keeps its bound account and provider thread.",
  },
  {
    title: "One local workspace",
    body: "Use the desktop app or CLI with one local service that preserves work across restarts.",
  },
];

const commands = [
  "decodex status",
  "decodex agent status",
  "decodex account list",
  "cargo run -p decodex-gpui",
];

const docs = [
  {
    title: "Quickstart",
    href: `${decodexGitHubUrl}/blob/main/openwiki/quickstart.md`,
  },
  {
    title: "Agent coordination",
    href: `${decodexGitHubUrl}/blob/main/openwiki/architecture/chief-coordination.md`,
  },
  {
    title: "Commands and validation",
    href: `${decodexGitHubUrl}/blob/main/openwiki/operations/commands-and-validation.md`,
  },
  {
    title: "Runtime architecture",
    href: `${decodexGitHubUrl}/blob/main/openwiki/architecture/runtime-architecture.md`,
  },
];

export { commands, decodexGitHubUrl, docs, productLoops };
