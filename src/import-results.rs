use anyhow::{bail, Result, Context};
use common::{load_json, merged_file, save_pdf_to, BenchMerge};
use serde::{Serialize, Deserialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;
use semver::VersionReq;
use clap::Parser;

use crate::common::{database_directory, run_resctl, BenchVersion};

mod common;

static ALLOWED_PREFIXES: &[&str] = &[
    "https://github.com/",
    "https://iocost-submit-us-east-1.s3.us-east-1.amazonaws.com/",
    "https://iocost-submit.s3.eu-north-1.amazonaws.com/",
];
static GH_CONTEXT_ENVVAR: &str = "GITHUB_CONTEXT";

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

async fn download_url(url: &str) -> Result<String> {
    let response = reqwest::get(url).await?;
    let contents = response.bytes().await?;
    // Use md5sum of the data as filename, we only care about exact duplicates.
    let path = format!("result-{:x}.json.gz", md5::compute(&contents));
    let mut file = fs::File::create(&path)?;
    file.write_all(&contents)?;
    Ok(path)
}


#[skip_serializing_none]
#[derive(Serialize)]
struct BenchResult {
    /// Drive model name
    model_name: String,
    /// Path to the result directory in the database
    #[serde(skip_serializing)]
    dir: String,
    /// Path to the source result file
    #[serde(skip_serializing)]
    result_file: String,
    /// resctl-bench version used to generate the result (major.minor)
    version: String,
    /// Github issue the result is related to, if any
    issue: Option<u64>,
    /// Result file url, if provided through a Github issue
    url: Option<String>,
}

impl BenchResult {
    /// Creates a BenchResult extracting the model and version info from
    /// a json file (`json_result_file`) and set it to store the output
    /// data into `database_path`
    async fn new(json_result_file: &str, database_path: &str)
    -> Result<Self>
    {
        let result = load_json(&json_result_file)
            .expect(&format!("Error parsing json file {}", &json_result_file));
        let full_version = result[0]["sysinfo"]["bench_version"]
            .to_string()
            .split_whitespace()
            .collect::<Vec<_>>()[0]
            .to_string();
        let version = {
            let v = semver::Version::parse(&full_version)?;
            format!("{}.{}", v.major, v.minor)
        };
        semver::Version::parse(&full_version)?;
        let model_name = result[0]["sysinfo"]["sysreqs_report"]["scr_dev_model"]
            .to_string()
            .replace(" ", "_");
        let dir = PathBuf::from(database_path)
            .join(&version)
            .join(&model_name)
            .into_os_string()
            .into_string().unwrap();
        Ok(BenchResult {
            model_name,
            dir,
            result_file: json_result_file.to_string(),
            version,
            issue: None,
            url: None
        })
    }

    /// Runs resctl-demo to validate the file in self.path.
    fn validate(&self) -> Result<()> {
        run_resctl(
            &self.version,
            &["--result", "/tmp/result.json", "merge", &self.result_file],
        )?;
        Ok(())
    }

    /// Returns a path for the result file in the DB
    fn db_file(&self) -> PathBuf {
        PathBuf::from(&self.dir).join(&self.result_file)
    }

    /// Returns a path for the metadata file in the DB
    fn metadata_file_path(&self) -> PathBuf {
        let basename = Path::new(&self.result_file)
            .with_extension("")
            .with_extension("")
            .to_str()
            .unwrap()
            .to_string();
        let mut metadata_filepath = PathBuf::from(&self.dir).join(basename);
        metadata_filepath.set_extension("json.metadata");
        metadata_filepath
    }

    /// Processes the result and stores the output files in the DB.
    /// If an `id` string is provided, it'll be used to name the
    /// directory for the pdf outputs
    fn add_to_database(&self, id: Option<&str>) -> Result<()> {
        let pdfs_dir = match id {
            Some(id) => PathBuf::from(".")
                .join(&format!("pdfs-for-{}", id)),
            None => {
                PathBuf::from(".")
                    .join(&format!("pdfs-for-{}-{}", &self.model_name, &self.version))
            }
        };
        save_pdf_to(&self.version, &PathBuf::from(&self.result_file), &pdfs_dir, None)?;
        // Generate DB directory and place the result file there
        fs::create_dir_all(&self.dir).ok();
        fs::rename(&self.result_file, &self.db_file())?;
        // Create metadata file and save it in the DB dir
        let mut metadata_file = fs::File::create(&self.metadata_file_path())?;
        write!(metadata_file, "{}", serde_json::to_string(self)?)?;
        Ok(())
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

async fn run_as_gh_workflow(database_path: &str) -> Result<()>{
    let envvar_contents = std::env::var(GH_CONTEXT_ENVVAR)
        .context(format!("Can't read environment variable {}", GH_CONTEXT_ENVVAR))?;
    let context = json::parse(&envvar_contents)?;
    let issue_id = context["event"]["issue"]["number"].as_u64().unwrap();
    let git_repo = git2::Repository::open(".")?;
    let mut index = git_repo.index()?;
    // HashMap to keep the complete set of results
    let mut merged = HashMap::new();

    // Download and validate all provided URLs.
    let urls = get_urls(&context)?;
    let mut errors = vec![];
    for url in urls {
        // Download resctl-bench result, process and validate it,
        // and add it to the database and the repo
        let path = download_url(&url).await?;
        let mut result = BenchResult::new(&path, database_path).await?;
        result.issue = Some(issue_id);
        result.url = Some(url.clone());
        if let Err(e) = result.validate() {
            errors.push(
                format!("File {} failed validation: \n\n{}", url, e)
            );
            continue;
        }
        result.add_to_database(Some(&issue_id.to_string()))?;
        index.add_path(&result.db_file())?;
        index.add_path(&result.metadata_file_path())?;
        merged
            .entry(format!("{}-{}", &result.version, &result.model_name))
            .or_insert_with(|| HighLevel::new(&result.version, &result.model_name))
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
    let oid = index.write_tree()?;
    let tree = git_repo.find_tree(oid)?;
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
    let commit_title = format!("Automated update from issue {}", issue_id);
    let commit_message = format!("{commit_title}\n\n{description}");
    let commit = git_repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &commit_message,
        &tree,
        &[&parent_commit],
    )?;
    let branch_name = format!("iocost-bot/{}", issue_id);
    git_repo.branch(&branch_name, &git_repo.find_commit(commit)?, true)?;

    // The rest of the process happens in the workflow.
    Ok(())
}


/// Top-level struct to parse the config toml file
#[derive(Debug, Deserialize)]
struct TomlData {
    config: Config,
}

/// Struct to parse the [config] section of the config toml file
#[derive(Debug, Deserialize)]
struct Config {
    database_dir: Option<String>,
}

#[derive(Parser, Debug)]
#[command(version, about)]
/// Imports resctl-bench results into a common database
///
/// By default, it runs as part of a Github workflow, reading the input
/// from an environment variable (GITHUB_CONTEXT). Run with -r
/// (--result) to specify a local result file.
struct Cli {
    /// Path of the toml config file to load
    #[arg(short, long, value_name = "FILE")]
    config_file: Option<String>,

    /// Result file to process
    #[arg(short, long, value_name = "FILE.json.gz")]
    result: Option<String>,

    /// Output database dir
    #[arg(short, long, value_name = "DIR", default_value = "database")]
    database_dir: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    // Load config from toml file, if specified
    let config: Option<TomlData> = match args.config_file {
        Some(path) => {
            match fs::read_to_string(&path) {
                Ok(contents) => {
                    toml::from_str(&contents)
                        .expect(&format!("Error parsing toml file {}", &path))
                },
                Err(_) => {
                    eprintln!("Can't open config file: {}", &path);
                    exit(1);
                }
            }
        },
        None => None,
    };

    // Process general parameters
    let database_dir;
    if let Some(config) = config {
        database_dir = config.config.database_dir.unwrap_or(args.database_dir.unwrap());
    } else {
        database_dir = args.database_dir.unwrap()
    }

    if let Some(result_file) = args.result {
        // Run with result file as input
        let bench_result = BenchResult::new(
            &result_file,
            &database_dir).await?;
        bench_result.validate()
            .expect(&format!("File {} failed validation", &result_file));
        bench_result.add_to_database(None)?;
    } else {
        // Run as part of a Github workflow
        println!("No result file specified: reading result info from \
                  Github workflow ({} envvar)", GH_CONTEXT_ENVVAR);
        run_as_gh_workflow(&database_dir).await?;
    }

    exit(1);
}
