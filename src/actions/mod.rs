//! GitHub Actions context parser.
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case", tag = "event_name")]
// see: https://docs.github.com/en/actions/reference/events-that-trigger-workflows#webhook-events
pub enum ContextPayload {
    Issues {
        event: IssueEvent,
    },

    IssueComment {
        event: IssueCommentEvent,
    },

    WorkflowDispatch {},

    #[serde(other)]
    Unimplemented,
}

impl ContextPayload {
    pub fn from_str(context: String) -> Result<ContextPayload, serde_json::Error> {
        serde_json::from_str(&context)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct IssueEvent {
    pub action: IssueEventAction,
    pub issue: Issue,
    pub repository: Repository,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum IssueEventAction {
    Opened,
    Edited,
    Deleted,
    Closed,
    Reopened,
    Locked,

    #[serde(other)]
    Unimplemented,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum IssueState {
    Open,
    Closed,

    #[serde(other)]
    Unimplemented,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct Issue {
    #[serde(rename = "number")]
    pub id: u64,
    pub title: String,
    pub body: String,
    pub user: User,
    pub locked: bool,
    pub state: IssueState,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct Repository {
    pub default_branch: String,
    pub full_name: String,
    pub name: String,
    pub owner: RepositoryOwner,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct RepositoryOwner {
    pub login: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct User {
    #[serde(rename = "login")]
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct IssueCommentEvent {
    pub action: IssueCommentEventAction,
    pub comment: IssueComment,
    pub issue: Issue,
    pub repository: Repository,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum IssueCommentEventAction {
    Created,
    Edited,
    Deleted,

    #[serde(other)]
    Unimplemented,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum CommentAuthorAssociation {
    Collaborator,
    Contributor,
    Member,
    Owner,

    #[serde(other)]
    None,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct IssueComment {
    pub id: u64,
    pub body: String,
    pub user: User,
    pub author_association: CommentAuthorAssociation,
}
