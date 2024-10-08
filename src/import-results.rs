use anyhow::{bail, Result};
use common::{load_json, merged_file, save_pdf_to, BenchMerge};
use git2::Index;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use semver::VersionReq;

use crate::common::{database_directory, run_resctl, BenchVersion};

mod common;

static ALLOWED_PREFIXES: &[&str] = &[
    "https://github.com/",
    "https://iocost-submit-us-east-1.s3.us-east-1.amazonaws.com/",
];

/// Returns `true` if the URL specified in `link` is allowed according
/// to its domain name. Returns `false` otherwise.
fn is_url_allowlisted(link: &str) -> bool {
    for prefix in ALLOWED_PREFIXES {
        if link.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Extracts the URLs found in a Github issue context.
/// Only open and unlocked issues are processed
fn get_urls(context: &json::JsonValue) -> Result<Vec<String>> {
    let issue = &context["event"]["issue"];

    // The workflow should already filter this out, but double-check.
    if issue["locked"].as_bool().unwrap() || issue["state"] != "open" {
        panic!("Issue is either locked or not in the open state, workflow should filter this...");
    }

    // created is always for comments, opened is always for issues.
    let body = match context["event"]["action"].as_str().unwrap() {
        "created" => context["event"]["comment"]["body"].as_str(),
        "opened" => issue["body"].as_str(),
        "edited" => {
            if context["event_name"] == "issue_comment" {
                context["event"]["comment"]["body"].as_str()
            } else {
                issue["body"].as_str()
            }
        }
        _ => bail!(
            "Called for event we do not handle: {} / {}",
            context["event_name"],
            context["event"]["action"]
        ),
    }
    .expect("Could not obtain the contents of the issue or comment");

    let mut urls = vec![];
    for link in linkify::LinkFinder::new().links(body) {
        let link = link.as_str();
        if is_url_allowlisted(link) && link.ends_with(".json.gz") {
            println!("URL found: {}", link);
            urls.push(link.to_string());
        } else {
            println!(
                "URL ignored due to not having a allowlisted prefix: {}",
                link
            );
        }
    }
    Ok(urls)
}

/// Models a resctl-bench result submitted in a github issue.
#[derive(Serialize)]
struct BenchResult {
    issue_id: u64,
    url: String,
    #[serde(skip_serializing)]
    path: String,
    version: String,
    model_name: String,
}

impl BenchResult {
    /// Creates a BenchResult from a github issue id and a link to the
    /// resctl-bench results (json file).
    async fn new(issue_id: u64, url: String) -> Result<Self> {
        let path = BenchResult::download_url(&url).await?;
        let json = &load_json(&path)?[0];
        // Data taken from the json file:
        //  - resctl-bench version
        //  - device model name
        let version = {
            let v = semver::Version::parse(
                json["sysinfo"]["bench_version"]
                    .to_string()
                    .split_whitespace()
                    .collect::<Vec<&str>>()[0],
            )?;
            format!("{}.{}", v.major, v.minor)
        };
        let model_name = json["sysinfo"]["sysreqs_report"]["scr_dev_model"]
            .to_string()
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join("_");
        Ok(BenchResult {
            issue_id,
            url,
            path,
            version,
            model_name,
        })
    }

    /// Downloads a file and saves it into a gzipped file with a unique
    /// name based on the file contents.
    async fn download_url(url: &str) -> Result<String> {
        let response = reqwest::get(url).await?;

        let contents = response.bytes().await?;

        // Use md5sum of the data as filename, we only care about exact
        // duplicates.
        let path = format!("result-{:x}.json.gz", md5::compute(&contents));

        let mut file = fs::File::create(&path)?;
        file.write_all(&contents)?;

        Ok(path)
    }

    /// Runs resctl-demo to validate the file in self.path.
    fn validate(&self) -> Result<()> {
        run_resctl(
            &self.version,
            &["--result", "/tmp/result.json", "merge", &self.path],
        )
        .map(|_| ())
    }

    /// Stores the results file, together with a metadata file, in the
    /// appropriate directory in the repo pointed by index, creating the
    /// directories if needed.
    ///
    /// The metadata file is just the json-serialized output of `self`
    /// (BenchResult).
    fn add_to_database(&self, index: &mut Index) -> Result<()> {
        // Save PDF for artifact.
        let pdfs_dir = PathBuf::from(".").join(&format!("pdfs-for-{}", self.issue_id));
        save_pdf_to(&self.version, &PathBuf::from(&self.path), &pdfs_dir, None)?;

        let model_directory = database_directory(&self.version, &self.model_name);
        fs::create_dir_all(&model_directory).ok();

        let database_file = model_directory.join(&self.path);
        fs::rename(&self.path, &database_file)?;

        index.add_path(&database_file)?;

        let mut metadata_path = database_file;
        metadata_path.set_extension("metadata");

        let mut metadata = fs::File::create(&metadata_path)?;
        write!(metadata, "{}", serde_json::to_string(self)?)?;

        index.add_path(&metadata_path)?;

        Ok(())
    }

    /// Provides a unique identifier for a BenchResult, suitable as a
    /// hash table key.
    fn merge_id(&self) -> String {
        format!("{}-{}", self.version, self.model_name)
    }
}


/// Models a resctl-bench high-level summary output
struct HighLevel {
    version: String,
    model_name: String,
    new_files: u64,
}

impl HighLevel {
    fn new(version: &str, model_name: &str) -> Self {
        HighLevel {
            version: version.to_string(),
            model_name: model_name.to_string(),
            new_files: 0,
        }
    }

    fn increment(&mut self) {
        self.new_files += 1;
    }

    /// Runs resctl-bench to generate a high-level summary, if
    /// available, and returns it as a String.
    fn format_high_level(&self) -> String {
        // Get the high-level summary only if `resctl-bench` supports
        // that option (available since v2.2.3)
        let resctl_bench_version = BenchVersion::new(&self.version);
        if VersionReq::parse("<2.2.3")
            .unwrap()
            .matches(&resctl_bench_version.semver)
        {
            return String::new();
        }

        let path = merged_file(&self.version, &self.model_name, None);
        BenchMerge::do_merge(
            &self.version,
            &database_directory(&self.version, &self.model_name),
            &path,
        )
        .expect("Failed to do the merge for obtaining high level summary");

        run_resctl(
            &self.version,
            &[
                "--result",
                &path.to_string_lossy(),
                "format",
                "iocost-tune:high-level",
            ],
        )
        .expect("Failed to run resctl-bench to format high level")
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let context = json::parse(&std::env::var("GITHUB_CONTEXT")?)?;
    let issue_id = context["event"]["issue"]["number"].as_u64().unwrap();
    let git_repo = git2::Repository::open(".")?;
    let mut index = git_repo.index()?;
    let mut merged = HashMap::new();
    let mut errors = vec![];

    // Download and validate all provided URLs.
    for url in get_urls(&context)? {
        let bench_result = BenchResult::new(issue_id, url).await?;
        if let Err(e) = bench_result.validate() {
            errors.push(format!(
                "= File {} failed validation: =\n\n{}",
                bench_result.url, e
            ));
            continue;
        }
        bench_result.add_to_database(&mut index)?;
        merged
            .entry(bench_result.merge_id())
            .or_insert_with(|| HighLevel::new(&bench_result.version, &bench_result.model_name))
            .increment();
    }
    if !errors.is_empty() {
        octocrab::OctocrabBuilder::new()
            .personal_token(context["token"].as_str().unwrap().to_string())
            .build()?
            .issues(
                context["repository_owner"].as_str().unwrap(),
                "iocost-benchmarks",
            )
            .create_comment(issue_id, errors.join("\n\n"))
            .await?;
    }
    if merged.is_empty() {
        println!("Found no new results files to merge...");
        return Ok(());
    }

    // Commit the new and changed files.
    let sig = git2::Signature::now("iocost bot", "iocost-bot@has.no.email")?;
    let parent_commit = git_repo.head()?.peel_to_commit()?;
    let description = format!(
        "Closes #{}\n\n{}",
        issue_id,
        merged
            .iter()
            .map(|(_, v)| format!(
                "[{} ({})] {} new files\n{}",
                v.model_name,
                v.version,
                v.new_files,
                v.format_high_level()
            ))
            .collect::<Vec<String>>()
            .join("\n")
    );
    let commit = git_repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        // Commit message
        &format!("Automated update from issue {}\n\n{}",
            issue_id,
            description),
        &git_repo.find_tree(index.write_tree()?)?,
        &[&parent_commit],
    )?;
    let branch_name = format!("iocost-bot/{}", issue_id);
    git_repo.branch(&branch_name, &git_repo.find_commit(commit)?, true)?;

    // The rest of the process happens in the workflow.
    Ok(())
}
