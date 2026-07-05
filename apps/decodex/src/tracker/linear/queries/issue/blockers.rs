pub(in crate::tracker::linear) const ISSUE_BLOCKERS_QUERY: &str = r#"
query IssueBlockers($issueId: String!, $after: String) {
  issues(filter: { id: { eq: $issueId } }, first: 1) {
    nodes {
      inverseRelations(first: 50, after: $after) {
        nodes {
          type
          issue {
            id
            identifier
            state {
              id
              name
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
"#;
