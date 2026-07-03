pub(in crate::tracker::linear) const ISSUE_UPDATE_MUTATION: &str = r#"
mutation UpdateIssue($id: String!, $input: IssueUpdateInput!) {
  issueUpdate(id: $id, input: $input) {
    success
  }
}
"#;
pub(in crate::tracker::linear) const ISSUE_UPDATE_BRIEF_MUTATION: &str = r#"
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
pub(in crate::tracker::linear) const ISSUE_CREATE_MUTATION: &str = r#"
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
pub(in crate::tracker::linear) const COMMENT_CREATE_MUTATION: &str = r#"
mutation CreateComment($input: CommentCreateInput!) {
  commentCreate(input: $input) {
    success
  }
}
"#;
pub(in crate::tracker::linear) const ISSUE_ARCHIVE_MUTATION: &str = r#"
mutation ArchiveIssue($id: String!, $trash: Boolean) {
  issueArchive(id: $id, trash: $trash) {
    success
  }
}
"#;
