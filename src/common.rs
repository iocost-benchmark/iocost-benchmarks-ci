use anyhow::{anyhow, bail, Result};
use glob::glob;
use semver::{Version, VersionReq};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, Hash, PartialEq)]
pub enum MajorMinor {
    V2_1,
    V2_2,
}

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct BenchVersion {
    pub major_minor: MajorMinor,
    pub semver: Version,
}

impl BenchVersion {
    pub fn new(version: &str) -> Self {
        let major_minor = match version {
            "2.1" => MajorMinor::V2_1,
            "2.2" => MajorMinor::V2_2,
            _ => unimplemented!(),
        };

        let version_str = run_resctl(version, &["--version"])
            .expect("Could not run resctl-bench to get version")
            .split_whitespace()
            .nth(1)
            .expect("Version string has bad format while splitting at space")
            .to_owned();

        let semver = Version::parse(&version_str).expect("Failed to parse version with semver");

        BenchVersion {
            major_minor,
            semver,
        }
    }
}

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct BenchMerge {
    pub version: BenchVersion,
    pub version_str: String,
    pub model_name: String,
    pub path: PathBuf,
}

#[allow(dead_code)]
impl BenchMerge {
    pub fn merge(version: String, model_name: String) -> Result<Self> {
        let directory = database_directory(&version, &model_name);
        let output_path = merged_file(&version, &model_name);

        Self::do_merge(&version, &directory, &output_path)?;

        Ok(BenchMerge {
            version: BenchVersion::new(&version),
            version_str: version,
            model_name,
            path: output_path,
        })
    }

    pub fn do_merge(version: &str, directory: &Path, output_path: &Path) -> Result<()> {
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

    pub fn save_pdf_in(&self, target_dir: &Path) -> Result<()> {
        let filename = self.build_descriptive_filename("pdf");
        save_pdf_to(&self.version_str, &self.path, target_dir, filename)
    }

    pub fn create_hwdb_in(&self, target_dir: &Path) -> Result<()> {
        // The hwdb subcommand got introduced in 2.2.4.
        if !VersionReq::parse(">=2.2.4")
            .unwrap()
            .matches(&self.version.semver)
        {
            println!(
                "Skipping hwdb generation as this version does not have hwdb support: {}",
                self.version.semver
            );
            return Ok(());
        }

        let filename = self.build_descriptive_filename("hwdb");

        let output = run_resctl(
            &self.version_str,
            &["--result", &self.path.to_string_lossy(), "hwdb"],
        )?;

        fs::create_dir_all(target_dir).expect("Could not create the target hwdb directory");
        let mut file = fs::File::create(target_dir.join(filename))?;
        write!(file, "{}", output)?;

        Ok(())
    }

    pub fn build_descriptive_filename(&self, extension: &str) -> String {
        let extension = if extension.is_empty() {
            extension.to_string()
        } else {
            format!(".{}", extension)
        };

        let date = chrono::offset::Utc::today();

        format!(
            "iocost-tune-{}-{}-{}{}",
            self.version_str, self.model_name, date, extension
        )
    }
}

pub fn save_pdf_to(
    version: &str,
    result: &Path,
    target_dir: &Path,
    filename: impl Into<Option<String>>,
) -> Result<()> {
    fs::create_dir_all(target_dir)?;

    // Build target path while replacing the json.gz extension with .pdf.
    let pdf_path = match filename.into() {
        Some(filename) => target_dir.join(PathBuf::from(filename)),
        None => {
            let result_filename = result
                .file_name()
                .expect("Malformed result path")
                .to_string_lossy()
                .to_string();
            target_dir
                .join(result_filename)
                .with_extension("")
                .with_extension("pdf")
        }
    };

    println!("PDF Path: {:#?}", pdf_path);
    run_resctl(
        version,
        &[
            "--result",
            &result.to_string_lossy(),
            "format",
            &format!("iocost-tune:pdf={}", pdf_path.to_string_lossy()),
        ],
    )
    .map(|_| ())
}

pub fn run_resctl<S: AsRef<std::ffi::OsStr>>(version: &str, args: &[S]) -> Result<String> {
    let bench_path = format!("./resctl-demo-v{}/resctl-bench", version);
    let output = std::process::Command::new(bench_path).args(args).output()?;

    if !output.stderr.is_empty() {
        bail!(String::from_utf8(output.stderr)?);
    }

    String::from_utf8(output.stdout).map_err(|e| anyhow!(e))
}

pub fn database_directory(version: &str, model_name: &str) -> PathBuf {
    PathBuf::from(format!("database/{}/{}", version, model_name))
}

pub fn merged_file(version: &str, model_name: &str) -> PathBuf {
    database_directory(version, model_name).join("merged-results.json.gz")
}
