use anyhow::{anyhow, bail, Result};
use glob::glob;
use json::JsonValue;
use semver::{Version, VersionReq};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MINIMUM_DATA_POINTS: usize = 4;
const MINIMUM_DIFFERENT_RESULTS: u64 = 1;

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
    pub data_points: usize,
    pub fwmerge: Option<BenchFWMerge>,
}

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct BenchFWMerge {
    pub fwrev: String,
    pub path: PathBuf,
    pub data_points: usize,
}

#[allow(dead_code)]
impl BenchMerge {
    pub fn merge(version: String, model_name: String) -> Result<Self> {
        let directory = database_directory(&version, &model_name);
        let output_path = merged_file(&version, &model_name, None);

        Self::do_merge(&version, &directory, &output_path)?;

        let data_points = Self::get_data_points(&output_path)?;

        let fwmerge = Self::try_fwmerge(data_points, &version, &model_name, &directory)?;

        Ok(BenchMerge {
            version: BenchVersion::new(&version),
            version_str: version,
            model_name,
            path: output_path,
            data_points,
            fwmerge,
        })
    }

    fn try_fwmerge(
        common_data_points: usize,
        version: &str,
        model_name: &str,
        directory: &Path,
    ) -> Result<Option<BenchFWMerge>> {
        let results = Self::result_paths_for(directory)?;

        let mut fwrev_map: HashMap<String, Vec<PathBuf>> = HashMap::new();

        // This uses alphabetical sorting to determine the latest firmware revision.
        // Based on how fwupd compares versions for NVME devices it should be good
        // enough, as it uses the PLAIN format for version numbers of NVME devices,
        // and does a simple g_strcmp0() for those.
        let max_fwrev = results
            .iter()
            .map(|r| {
                let json = &load_json(&r.to_string_lossy()).expect("Failed to load result")[0];
                let fwrev = json["sysinfo"]["sysreqs_report"]["scr_dev_fwrev"].to_string();

                fwrev_map.entry(fwrev.clone()).or_default().push(r.clone());

                fwrev
            })
            .max_by(|a, b| a.cmp(b))
            .unwrap();

        let output_path = merged_file(version, model_name, max_fwrev.as_str());
        let mut arguments = vec![
            "--result".to_string(),
            output_path.to_string_lossy().to_string(),
            "merge".to_string(),
        ];
        arguments.extend(
            fwrev_map
                .get(&max_fwrev)
                .unwrap()
                .iter()
                .map(|p| p.to_string_lossy().to_string()),
        );

        let mut output = format!(
            "Merging FW-specific results with: {}\n",
            arguments.join(" ")
        );
        output.push_str(&run_resctl(version, arguments.as_slice())?);
        println!("{}", output);

        let data_points = Self::get_data_points(&output_path)?;
        // If there are almost the same number of results for the generic merge as there are for the specific fwrev,
        // just use the generic one.
        if (data_points as i64 - common_data_points as i64).unsigned_abs()
            >= MINIMUM_DIFFERENT_RESULTS
            && data_points >= MINIMUM_DATA_POINTS
        {
            println!(
                "Model {} fwrev {} has enough data points: {}, generating specific solution.",
                model_name, max_fwrev, data_points
            );
            return Ok(Some(BenchFWMerge {
                fwrev: max_fwrev,
                path: output_path,
                data_points,
            }));
        }

        if data_points < MINIMUM_DATA_POINTS {
            println!(
                "Model {} fwrev {} has too few data points: {}, no specific solution generated.",
                model_name, max_fwrev, data_points
            );
        } else {
            println!("Model {} fwrev {} has almost the same input as the generic one, no specific solution generated.", model_name, max_fwrev);
        }

        std::fs::remove_file(output_path)?;
        Ok(None)
    }

    pub fn do_merge(version: &str, directory: &Path, output_path: &Path) -> Result<()> {
        let results = Self::result_paths_for(directory)?
            .into_iter()
            .map(|p| p.to_string_lossy().to_string());

        let mut arguments = vec![
            "--result".to_string(),
            output_path.to_string_lossy().to_string(),
            "merge".to_string(),
        ];
        arguments.extend(results);

        let mut output = format!("Merging results with: {}\n", arguments.join(" "));
        output.push_str(&run_resctl(version, arguments.as_slice())?);
        println!("{}", output);

        Ok(())
    }

    fn get_data_points(path: &Path) -> Result<usize> {
        // TODO: we probably want to move this processing to resctl-bench format output.
        let result = load_json(&path.to_string_lossy())?;
        let result = result
            .members()
            .find(|v| v["spec"]["kind"] == "iocost-tune")
            .expect("Could not find iocost-tune spec in merge file");

        Ok(result["result"]["data"]["MOF"]["data"].members().count()
            + result["result"]["data"]["MOF"]["outliers"]
                .members()
                .count())
    }

    fn result_paths_for(directory: &Path) -> Result<Vec<PathBuf>> {
        Ok(
            glob(&format!("{}/result-*.json.gz", directory.to_string_lossy()))
                .unwrap()
                .flatten()
                .collect(),
        )
    }

    pub fn save_pdf_in(&self, target_dir: &Path) -> Result<()> {
        let filename = self.build_descriptive_filename("pdf", None);
        save_pdf_to(&self.version_str, &self.path, target_dir, filename)
    }

    pub fn create_hwdb_in(&self, target_dir: &Path) -> Result<()> {
        fs::create_dir_all(target_dir).expect("Could not create the target hwdb directory");

        // The hwdb subcommand got introduced in 2.2.4. We use -0 here so that pre-released
        // versions from git are also considered to match.
        if !VersionReq::parse(">=2.2.4-0")
            .unwrap()
            .matches(&self.version.semver)
        {
            println!(
                "Skipping hwdb generation as this version does not have hwdb support: {}",
                self.version.semver
            );
            return Ok(());
        }

        let filename = self.build_descriptive_filename("hwdb", None);

        let mut file = fs::File::create(target_dir.join(filename))?;

        let output = run_resctl(
            &self.version_str,
            &[
                "--result",
                &self.path.to_string_lossy(),
                "format",
                "iocost-tune:hwdb",
            ],
        )?;

        write!(file, "{}", output)?;

        if let Some(fwmerge) = &self.fwmerge {
            let output = run_resctl(
                &self.version_str,
                &[
                    "--result",
                    &fwmerge.path.to_string_lossy(),
                    "format",
                    "iocost-tune:hwdb-fwrev",
                ],
            )?;

            write!(file, "\n{}", output)?;
        }

        Ok(())
    }

    pub fn build_descriptive_filename<'a, D: Into<Option<&'a str>>>(
        &self,
        extension: &str,
        detail: D,
    ) -> String {
        let extension = if extension.is_empty() {
            extension.to_string()
        } else {
            format!(".{}", extension)
        };

        let date = chrono::offset::Utc::now();

        let detail = match detail.into() {
            Some(d) => format!("{}-", d),
            None => "".to_owned(),
        };

        format!(
            "iocost-tune-{}-{}-{}{}{}",
            self.version_str, self.model_name, date, detail, extension
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

    println!("PDF Path: {:#?}\n", pdf_path);
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

#[allow(dead_code)]
pub fn load_json(filename: &str) -> Result<JsonValue> {
    let f = std::fs::File::open(&filename)?;

    let mut buf = vec![];
    libflate::gzip::Decoder::new(f)?.read_to_end(&mut buf)?;

    Ok(json::parse(&String::from_utf8(buf)?)?)
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

pub fn merged_file<'a, D: Into<Option<&'a str>>>(
    version: &str,
    model_name: &str,
    detail: D,
) -> PathBuf {
    fs::create_dir_all("merged-results").expect("Failed to create merged results dir");

    let detail = match detail.into() {
        Some(d) => format!("{}-", d),
        None => "".to_owned(),
    };

    PathBuf::from("merged-results").join(&format!(
        "{}-{}-{}merged-results.json.gz",
        version, model_name, detail
    ))
}
