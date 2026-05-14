export const RESET_STATUS_QUESTION = "Are we reset today?";
export const RESET_STATUS_SOURCE_ACCOUNT = "@thsottiaux";
export const RESET_STATUS_SOURCE_URL = "https://x.com/thsottiaux";

export type ResetStatusAnswer = "yes" | "no" | "unknown";
export type ResetStatusConfidence = "confirmed" | "likely" | "weak";

export type ResetStatusEvidencePost = {
  published_at_label?: string;
  relevance: "related" | "not_related" | "uncertain";
  summary: string;
  url?: string;
};

export type ResetStatusData = {
  answer: ResetStatusAnswer;
  confidence: ResetStatusConfidence;
  evidence_posts: ResetStatusEvidencePost[];
  generated_at: string;
  judgment_mode: "ai_semantic_review";
  observed_for_date: string;
  question: string;
  rationale: string;
  schema: "reset_status/v1";
  search_url?: string;
  source_account: string;
  source_url: string;
  timezone: string;
};

export function resetStatusAnswerLabel(answer: ResetStatusAnswer): string {
  switch (answer) {
    case "yes":
      return "Yes";
    case "no":
      return "No";
    case "unknown":
      return "Unknown";
  }
}

export function resetStatusTone(answer: ResetStatusAnswer): "muted" | "neutral" | "positive" {
  switch (answer) {
    case "yes":
      return "positive";
    case "no":
      return "neutral";
    case "unknown":
      return "muted";
  }
}
