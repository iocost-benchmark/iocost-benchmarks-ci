use anyhow::{anyhow, Error};
use linkify::LinkFinder;
use octocrab::Octocrab;
use serde::Deserialize;
use std::fs::File;
use std::io::copy;
use tempfile::Builder;

// see: https://docs.github.com/en/actions/reference/events-that-trigger-workflows#webhook-events
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case", tag = "event_name")]
pub enum ActionContextPayload {
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct IssueEvent {
    action: IssueEventAction,
    issue: Issue,
    repository: Repository,
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
    id: u64,
    title: String,
    body: String,
    user: User,
    locked: bool,
    state: IssueState,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct Repository {
    default_branch: String,
    full_name: String,
    name: String,
    owner: RepositoryOwner,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct RepositoryOwner {
    login: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct User {
    #[serde(rename = "login")]
    username: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub struct IssueCommentEvent {
    action: IssueCommentEventAction,
    comment: IssueComment,
    issue: Issue,
    repository: Repository,
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
    id: u64,
    body: String,
    user: User,
    author_association: CommentAuthorAssociation,
}

/// Process a GitHub Actions event
pub async fn process_event(token: String, context: String) -> Result<(), Error> {
    // create a static instance of octocrab using the Actions token
    // to communicate with GitHub api
    octocrab::initialise(Octocrab::builder().personal_token(token))?;

    let context: ActionContextPayload = serde_json::from_str(&context).unwrap();
    println!("decoded context: {:#?}", context);

    match context {
        ActionContextPayload::Issues { event } => match event.action {
            IssueEventAction::Opened => process_submission(event).await,
            IssueEventAction::Edited => process_submission(event).await,
            IssueEventAction::Closed => Ok(()),
            IssueEventAction::Locked => Ok(()),
            _ => Err(anyhow!("Action {:?} not yet implemented", event.action)),
        },
        ActionContextPayload::IssueComment { event: _ } => {
            todo!("Handle issue comment")
        }
        ActionContextPayload::WorkflowDispatch {} => {
            todo!("Handle workflow dispatch")
        }
        ActionContextPayload::Unimplemented => Err(anyhow!("Event not yet implemented")),
    }

    // TODO handle errors and post as comment
}

pub async fn process_submission(event: IssueEvent) -> Result<(), Error> {
    // bail if issue is closed
    if event.issue.state != IssueState::Open {
        return Ok(());
    }

    // bail if issue is locked
    if event.issue.locked {
        return Ok(());
    }

    // make sure the event is correct
    match event.action {
        IssueEventAction::Opened | IssueEventAction::Edited => {}
        _ => return Err(anyhow!("submission type not implemented")),
    };

    // extract URLs from the comment body
    let tmp_dir = Builder::new().prefix("iocost-benchmark-ci").tempdir()?;
    for link in LinkFinder::new().links(&event.issue.body) {
        let url = link.as_str();

        // TODO low priority - use the project URL from event body rather than hard-coded
        // skip URLs which are not files hosted in this github issue
        if !url.starts_with("https://github.com/iocost-benchmark/benchmarks/files/") {
            return Err(anyhow!("The file must be uploaded to the GitHub issue"));
        }

        // check the filetype is expected
        if !url.ends_with(".json.gz") {
            return Err(anyhow!("The file type must be json.gz"));
        }

        // TODO add URL to a list of benchmarks to look at
        println!("found link={:?}", url);

        // TODO move download code elsewhere
        let response = reqwest::get(url).await?;
        let mut dest = {
            let fname = response
                .url()
                .path_segments()
                .and_then(|segments| segments.last())
                .and_then(|name| if name.is_empty() { None } else { Some(name) })
                .unwrap();

            println!("file to download: '{}'", fname);
            let fname = tmp_dir.path().join(fname);
            println!("will be located under: '{:?}'", fname);
            File::create(fname)?
        };
        let content = response.text().await?;
        copy(&mut content.as_bytes(), &mut dest)?;
    }

    // TODO extract all json files to memory & parse json (error if any fails to extract/parse)
    // TODO sort submissions by model type { modelA = [benchmarkA, benchmarkB], modelB=[benchmarkC]}

    // TODO extract model type from json
    // TODO create a git branch
    // TODO create directories for each model
    // TODO move original json.gz files inside repo (careful not to overwrite)
    // TODO run merge on each model type with existing files in repo

    // TODO put benchmark result in comment text
    // TODO upload PDFs of benchmark result and attach to comment text
    let comment_text = "👋 Hello and thank you for your submission!\n\n\nHere is where the result should go once the benchmark has ran.";

    octocrab::instance()
        .issues(event.repository.owner.login, event.repository.name)
        .create_comment(event.issue.id, comment_text)
        .await?;

    Ok(())
}
