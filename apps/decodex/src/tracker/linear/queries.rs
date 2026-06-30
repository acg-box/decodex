pub(super) const LINEAR_GRAPHQL_URL: &str = "https://api.linear.app/graphql";
pub(super) const ISSUES_WITH_LABEL_QUERY: &str = r#"
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
pub(super) const ISSUE_BY_IDENTIFIER_QUERY: &str = r#"
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
pub(super) const ISSUES_BY_IDS_QUERY: &str = r#"
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
pub(super) const ISSUE_BLOCKERS_QUERY: &str = r#"
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
pub(super) const ISSUE_COMMENTS_QUERY: &str = r#"
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
pub(super) const ISSUE_UPDATE_MUTATION: &str = r#"
mutation UpdateIssue($id: String!, $input: IssueUpdateInput!) {
  issueUpdate(id: $id, input: $input) {
    success
  }
}
"#;
pub(super) const ISSUE_UPDATE_BRIEF_MUTATION: &str = r#"
mutation UpdateIssueBrief($id: String!, $input: IssueUpdateInput!) {
  issueUpdate(id: $id, input: $input) {
    success
    issue {
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
}
"#;
pub(super) const ISSUE_CREATE_MUTATION: &str = r#"
mutation CreateIssue($input: IssueCreateInput!) {
  issueCreate(input: $input) {
    success
    issue {
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
}
"#;
pub(super) const COMMENT_CREATE_MUTATION: &str = r#"
mutation CreateComment($input: CommentCreateInput!) {
  commentCreate(input: $input) {
    success
  }
}
"#;
pub(super) const ISSUE_ARCHIVE_MUTATION: &str = r#"
mutation ArchiveIssue($id: String!, $trash: Boolean) {
  issueArchive(id: $id, trash: $trash) {
    success
  }
}
"#;
pub(super) const TEAM_LABEL_BY_NAME_QUERY: &str = r#"
query TeamLabelByName($teamId: ID!, $labelName: String!) {
  issueLabels(filter: { team: { id: { eq: $teamId } }, name: { eq: $labelName } }, first: 1) {
    nodes {
      id
      name
    }
  }
}
"#;
