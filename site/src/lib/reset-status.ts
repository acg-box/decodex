export type ResetStatusValue = "reset" | "not_reset" | "unknown";

export type ResetStatusData = {
  schema: "reset_status/v1";
  source_label: string;
  source_kind: "community";
  source_url: string;
  source_api_url: string;
  status: ResetStatusValue;
  stale: boolean;
  configured: boolean;
  upstream_state?: string | null;
  auto_reset_hours?: number | null;
  reset_at?: string | null;
  updated_at: string;
};

export function resetStatusAnswer(entry: ResetStatusData): "Yes" | "No" {
  return entry.status === "reset" ? "Yes" : "No";
}

export function resetStatusQuestion(): string {
  return "Are we reset today?";
}

export function resetStatusTone(entry: ResetStatusData): "positive" | "neutral" | "muted" {
  switch (entry.status) {
    case "reset":
      return "positive";
    case "not_reset":
      return "neutral";
    case "unknown":
      return "muted";
  }
}
