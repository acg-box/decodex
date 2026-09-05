const decodexGitHubUrl = "https://github.com/acg-box/decodex";

const productLoops = [
  {
    title: "Conversations",
    body: "Start or continue real Codex threads with durable conversation and attempt evidence.",
  },
  {
    title: "Account routing",
    body: "Use service-owned accounts, quota observations, and fixed or balanced routing.",
  },
  {
    title: "Adaptive Programs",
    body: "Repeat bounded evidence-backed Program cycles through the ordinary Conversation runtime.",
  },
  {
    title: "One local product",
    body: "Decodex.app presents the product while decodex serve owns behavior and persistent SQLite state.",
  },
];

const commands = [
  "decodex status",
  "decodex doctor --output json",
  "decodex account list",
  "cargo run -p decodex-gpui",
];

const docs = [
  {
    title: "Local product contract",
    href: `${decodexGitHubUrl}/blob/main/openwiki/specs/local-product-v1.md`,
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
