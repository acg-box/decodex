pub(in crate::tracker::linear) const TEAM_LABEL_BY_NAME_QUERY: &str = r#"
query TeamLabelByName($teamId: ID!, $labelName: String!) {
  issueLabels(filter: { team: { id: { eq: $teamId } }, name: { eq: $labelName } }, first: 1) {
    nodes {
      id
      name
    }
  }
}
"#;
