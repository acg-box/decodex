pub(in crate::tracker::linear) const ISSUES_WITH_LABEL_QUERY: &str = r#"
query IssuesWithLabel($labelName: String!, $after: String) {
  issues(filter: { labels: { name: { eq: $labelName } } }, first: 50, after: $after) {
    nodes {
      id
      identifier
      title
      creator {
        displayName
        name
        email
      }
      description
      priority
      createdAt
      updatedAt
      state {
        id
        name
      }
      team {
        id
        name
        states(first: 50) {
          nodes {
            id
            name
          }
        }
        labels(first: 100) {
          nodes {
            id
            name
          }
        }
      }
      labels(first: 50) {
        nodes {
          id
          name
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
      inverseRelations(first: 50) {
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
    pageInfo {
      hasNextPage
      endCursor
    }
  }
}
"#;
