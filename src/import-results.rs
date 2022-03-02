use anyhow::{anyhow, bail, Result};
use glob::glob;
use json::JsonValue;
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

static ALLOWED_PREFIXES: &[&str] = &[
    "https://github.com/iocost-benchmark/iocost-benchmarks/files/",
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

async fn download_url(url: &str) -> Result<String> {
    let response = reqwest::get(url).await?;

    let contents = response.bytes().await?;

    // Use md5sum of the data as filename, we only care about exact duplicates.
    let path = format!("result-{:x}.json.gz", md5::compute(&contents));

    let mut file = fs::File::create(&path)?;
    file.write_all(&contents)?;

    Ok(path)
}

fn load_result(filename: &str) -> Result<JsonValue> {
    let f = std::fs::File::open(&filename)?;

    let mut buf = vec![];
    libflate::gzip::Decoder::new(f)?.read_to_end(&mut buf)?;

    Ok(json::parse(&String::from_utf8(buf)?)?[0].clone())
}

fn get_minor_bench_version(result: &JsonValue) -> Result<String> {
    let version = semver::Version::parse(
        result["sysinfo"]["bench_version"]
            .to_string()
            .split_whitespace()
            .collect::<Vec<&str>>()[0],
    )?;

    Ok(format!("{}.{}", version.major, version.minor))
}

fn get_normalized_model_name(result: &JsonValue) -> Result<String> {
    Ok(result["sysinfo"]["sysreqs_report"]["scr_dev_model"]
        .to_string()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join("_"))
}

fn run_resctl<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Result<String> {
    let output = std::process::Command::new("./resctl-demo/resctl-bench")
        .args(args)
        .output()?;

    if !output.stderr.is_empty() {
        panic!("{}", String::from_utf8(output.stderr)?);
    }

    String::from_utf8(output.stdout).map_err(|e| anyhow!(e))
}

fn merge_results_in_dir(path: &Path) -> Result<PathBuf> {
    let results = glob(&format!("{}/result-*.json.gz", path.to_string_lossy()))
        .unwrap()
        .into_iter()
        .flatten()
        .map(|p| p.to_string_lossy().to_string());

    let merged_path = path.join("merged-results.json.gz");
    let mut arguments = vec![
        "--result".to_string(),
        merged_path.to_string_lossy().to_string(),
        "merge".to_string(),
    ];
    arguments.extend(results);

    let output = run_resctl(arguments.as_slice())?;
    println!("{}", output);

    Ok(merged_path)
}

fn get_summary(path: &Path) -> Result<String> {
    run_resctl(&[
        "--result",
        path.to_string_lossy().to_string().as_str(),
        "summary",
    ])
}

#[tokio::main]
async fn main() -> Result<()> {
    let context = json::parse(&std::env::var("GITHUB_CONTEXT")?)?;

    let git_repo = git2::Repository::open(".")?;
    let mut index = git_repo.index()?;

    let mut directories_to_merge = HashSet::new();

    // Download and validate all provided URLs.
    let urls = get_urls(&context)?;
    for url in urls {
        let filename = download_url(&url).await?;
        let result = load_result(&filename)?;

        let bench_version = get_minor_bench_version(&result)?;
        let model_name = get_normalized_model_name(&result)?;

        let model_directory = PathBuf::from(format!("database/{}/{}", bench_version, model_name));
        fs::create_dir_all(&model_directory).ok();

        let database_file = model_directory.join(&filename);
        fs::rename(&filename, &database_file)?;

        index.add_path(&database_file)?;

        directories_to_merge.insert(model_directory);
    }

    // Call rectl-bench to merge all files for the directories with new files.
    println!("Merging results...");

    let mut summaries = vec![];
    for dir in &directories_to_merge {
        let merged_path = merge_results_in_dir(dir.as_path())?;
        index.add_path(&merged_path)?;

        summaries.push(get_summary(&merged_path)?);
    }

    // Commit the new and changed files.
    let sig = git2::Signature::now("iocost bot", "iocost-bot@has.no.email")?;

    let parent_commit = git_repo.head()?.peel_to_commit()?;

    let oid = index.write_tree()?;
    let tree = git_repo.find_tree(oid)?;

    let issue_id = context["event"]["issue"]["number"].as_i64().unwrap();
    let description = format!("Closes #{}\n\n{}", issue_id, summaries.join("\n"));

    let commit_title = format!(
        "Updated {}",
        directories_to_merge
            .iter()
            .map(|d| d.to_string_lossy().to_string())
            .collect::<Vec<String>>()
            .join(", ")
    );

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
