pub(in crate::tracker::linear) const ISSUE_COMMENTS_QUERY: &str = r#"
query IssueComments($issueId: String!, $after: String) {
  issue(id: $issueId) {
    comments(first: 100, after: $after) {
      nodes {
        body
        createdAt
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
"#;
