pub(in crate::tracker::linear) const ISSUE_RELATIONS_QUERY: &str = r#"
query IssueRelations($issueId: String!, $after: String) {
  issue(id: $issueId) {
    relations(first: 50, after: $after) {
      nodes {
        issue {
          id
        }
        relatedIssue {
          id
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
"#;

pub(in crate::tracker::linear) const ISSUE_INVERSE_RELATIONS_QUERY: &str = r#"
query IssueInverseRelations($issueId: String!, $after: String) {
  issue(id: $issueId) {
    inverseRelations(first: 50, after: $after) {
      nodes {
        issue {
          id
        }
        relatedIssue {
          id
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
"#;
