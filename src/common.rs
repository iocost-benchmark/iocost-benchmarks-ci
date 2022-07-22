use anyhow::{anyhow, bail, Result};
use glob::glob;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct BenchMerge {
    pub version: String,
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
            version,
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
        save_pdf_to(&self.version, &self.path, target_dir, filename)
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
            self.version, self.model_name, date, extension
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
