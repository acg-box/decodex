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
