use anyhow::{anyhow, bail, Result};
use git2::Index;
use glob::glob;
use json::JsonValue;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

static ALLOWED_PREFIXES: &[&str] = &[
    "https://github.com/",
    "https://iocost-submit.s3.af-south-1.amazonaws.com/",
    "https://iocost-submit.s3.ap-east-1.amazonaws.com/",
    "https://iocost-submit.s3.ap-northeast-1.amazonaws.com/",
    "https://iocost-submit.s3.ap-northeast-2.amazonaws.com/",
    "https://iocost-submit.s3.ap-northeast-3.amazonaws.com/",
    "https://iocost-submit.s3.ap-south-1.amazonaws.com/",
    "https://iocost-submit.s3.ap-southeast-1.amazonaws.com/",
    "https://iocost-submit.s3.ap-southeast-2.amazonaws.com/",
    "https://iocost-submit.s3.ap-southeast-3.amazonaws.com/",
    "https://iocost-submit.s3.ca-central-1.amazonaws.com/",
    "https://iocost-submit.s3.eu-central-1.amazonaws.com/",
    "https://iocost-submit.s3.eu-north-1.amazonaws.com/",
    "https://iocost-submit.s3.eu-south-1.amazonaws.com/",
    "https://iocost-submit.s3.eu-west-1.amazonaws.com/",
    "https://iocost-submit.s3.eu-west-2.amazonaws.com/",
    "https://iocost-submit.s3.eu-west-3.amazonaws.com/",
    "https://iocost-submit.s3.me-south-1.amazonaws.com/",
    "https://iocost-submit.s3.sa-east-1.amazonaws.com/",
    "https://iocost-submit.s3.us-east-1.amazonaws.com/",
    "https://iocost-submit.s3.us-east-2.amazonaws.com/",
    "https://iocost-submit.s3.us-west-1.amazonaws.com/",
    "https://iocost-submit.s3.us-west-2.amazonaws.com/",
];

fn is_url_allowlisted(link: &str) -> bool {
    for prefix in ALLOWED_PREFIXES {
        if link.starts_with(prefix) {
            return true;
        }
    }

    false
}

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

        if is_url_allowlisted(link) {
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

struct BenchResult {
    url: String,
    path: String,
    version: String,
    model_name: String,
}

impl BenchResult {
    async fn new(url: String) -> Result<Self> {
        let path = BenchResult::download_url(&url).await?;
        let json = BenchResult::load_json(&path)?;

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
            url,
            path,
            version,
            model_name,
        })
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

    fn load_json(filename: &str) -> Result<JsonValue> {
        let f = std::fs::File::open(&filename)?;

        let mut buf = vec![];
        libflate::gzip::Decoder::new(f)?.read_to_end(&mut buf)?;

        Ok(json::parse(&String::from_utf8(buf)?)?[0].clone())
    }

    fn validate(&self) -> Result<()> {
        run_resctl(
            &self.version,
            &["--result", "/tmp/result.json", "merge", &self.path],
        )
        .map(|_| ())
    }

    fn database_directory(&self) -> PathBuf {
        PathBuf::from(format!("database/{}/{}", self.version, self.model_name))
    }

    fn add_to_database(&self) -> Result<PathBuf> {
        let model_directory = self.database_directory();
        fs::create_dir_all(&model_directory).ok();

        let database_file = model_directory.join(&self.path);
        fs::rename(&self.path, &database_file)?;

        Ok(database_file)
    }

    fn merge_id(&self) -> String {
        format!("{}-{}", self.version, self.model_name)
    }
}

#[derive(Eq, Hash, PartialEq)]
struct BenchMerge {
    version: String,
    model_name: String,
    path: PathBuf,
    new_files: Vec<String>,
}

impl BenchMerge {
    fn merge(index: &mut Index, results: &[BenchResult]) -> Result<Self> {
        let reference = &results[0];

        let version = reference.version.clone();

        let path = reference
            .database_directory()
            .join("merged-results.json.gz");

        let database_directory = reference.database_directory();
        Self::do_merge(&version, &database_directory, &path)?;

        index.add_path(&path)?;

        let model_name = reference.model_name.clone();

        let new_files = results
            .iter()
            .map(|r| {
                PathBuf::from(&r.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();

        /* Add the result formatted output as a new file in the repository.
         * We could upload it to the issue, but the API has no way of doing
         * it at the moment, and it may actually be better to have it in
         * the repository. */
        let base_args = &["--result", &path.to_string_lossy().to_string(), "format"];

        let format = run_resctl(
            &version,
            &[base_args.to_vec(), vec!["iocost-tune"]].concat(),
        )?;

        let format_path = database_directory.join(format!("{}.txt", model_name));
        let mut file = fs::File::create(&format_path)?;
        file.write_all(format.as_bytes())?;

        index.add_path(&format_path)?;

        // And add the PDF version as well
        let pdf_path = database_directory.join(format!("{}.pdf", model_name));
        let pdf_arg = format!("iocost-tune:pdf={}", pdf_path.to_string_lossy().to_string());
        run_resctl(&version, &[base_args.to_vec(), vec![&pdf_arg]].concat())?;

        index.add_path(&pdf_path)?;

        Ok(BenchMerge {
            version,
            model_name,
            path,
            new_files,
        })
    }

    fn do_merge(version: &str, directory: &Path, output_path: &Path) -> Result<()> {
        let results = glob(&format!("{}/result-*.json.gz", directory.to_string_lossy()))
            .unwrap()
            .into_iter()
            .flatten()
            .map(|p| p.to_string_lossy().to_string());

        let mut arguments = vec![
            "--result".to_string(),
            output_path.to_string_lossy().to_string(),
            "merge".to_string(),
        ];
        arguments.extend(results);

        println!("Merging results with: {}", arguments.join(" "));
        let output = run_resctl(version, arguments.as_slice())?;
        println!("{}", output);

        Ok(())
    }
}

fn run_resctl<S: AsRef<std::ffi::OsStr>>(version: &str, args: &[S]) -> Result<String> {
    let bench_path = format!("./resctl-demo-v{}/resctl-bench", version);
    let output = std::process::Command::new(bench_path).args(args).output()?;

    if !output.stderr.is_empty() {
        bail!(String::from_utf8(output.stderr)?);
    }

    String::from_utf8(output.stdout).map_err(|e| anyhow!(e))
}

#[tokio::main]
async fn main() -> Result<()> {
    let context = json::parse(&std::env::var("GITHUB_CONTEXT")?)?;

    let git_repo = git2::Repository::open(".")?;
    let mut index = git_repo.index()?;

    let mut to_merge = HashMap::new();

    // Download and validate all provided URLs.
    let urls = get_urls(&context)?;
    let mut errors = vec![];
    for url in urls {
        let bench_result = BenchResult::new(url).await?;

        if let Err(e) = bench_result.validate() {
            errors.push(format!(
                "= File {} failed validation: =\n\n{}",
                bench_result.url, e
            ));
            continue;
        }

        let database_file = bench_result.add_to_database()?;
        index.add_path(&database_file)?;

        to_merge
            .entry(bench_result.merge_id())
            .or_insert(vec![])
            .push(bench_result);
    }

    let issue_id = context["event"]["issue"]["number"].as_u64().unwrap();

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

    if to_merge.is_empty() {
        println!("Found no results files to merge...");
        return Ok(());
    }

    // Call rectl-bench to merge all files for the directories with new files.
    println!("Merging results...");

    let mut merged = vec![];
    for (merge_id, bench_results) in &to_merge {
        println!("Merging {}...", merge_id);
        let merge = BenchMerge::merge(&mut index, bench_results)?;
        merged.push(merge);
    }

    // Commit the new and changed files.
    let sig = git2::Signature::now("iocost bot", "iocost-bot@has.no.email")?;

    let parent_commit = git_repo.head()?.peel_to_commit()?;

    let oid = index.write_tree()?;
    let tree = git_repo.find_tree(oid)?;

    // FIXME: add more detail about the merged files.
    let description = format!(
        "Closes #{}\n\n{}",
        issue_id,
        merged
            .iter()
            .map(|m| format!(
                "[{}] {} ({} new files)",
                m.version,
                m.model_name,
                m.new_files.len()
            ))
            .collect::<Vec<String>>()
            .join(", ")
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
