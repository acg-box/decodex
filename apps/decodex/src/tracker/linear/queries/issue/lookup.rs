pub(in crate::tracker::linear) const ISSUE_BY_IDENTIFIER_QUERY: &str = r#"
query IssueByIdentifier($issueIdentifier: String!) {
  issue(id: $issueIdentifier) {
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
}
"#;
pub(in crate::tracker::linear) const ISSUES_BY_IDS_QUERY: &str = r#"
query IssuesByIds($issueIds: [ID!], $after: String) {
  issues(filter: { id: { in: $issueIds } }, first: 50, after: $after) {
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
