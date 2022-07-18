use anyhow::Result;
use glob::glob;
use std::io::Write;
use std::{fs, path::PathBuf};

use crate::common::BenchMerge;

mod common;

#[tokio::main]
async fn main() -> Result<()> {
    for entry in glob("database/*.*").unwrap().into_iter().flatten() {
        if !entry.is_dir() {
            continue;
        }

        let version = entry.file_name().unwrap().to_string_lossy().to_string();
        for model_dir in glob(&format!("database/{}/*", version))
            .unwrap()
            .into_iter()
            .flatten()
        {
            let model_name = model_dir.file_name().unwrap().to_string_lossy().to_string();
            let merge = BenchMerge::merge(version.clone(), model_name)?;

            merge
                .save_pdf_in(&PathBuf::from("pdfs"))
                .expect("Failed to save PDF");

            merge
                .create_hwdb_in(&PathBuf::from("hwdb-inputs"))
                .expect("Failed to create a hwdb file");
        }
    }

    let mut hwdb_file =
        fs::File::create("90-iocost-tune.hwdb").expect("Failed to create hwdb file");

    writeln!(
        hwdb_file,
        "# This file is auto-generated on {}.",
        chrono::Utc::now().to_rfc2822()
    )?;
    writeln!(hwdb_file, "# From the following commit:")?;

    let context = json::parse(&std::env::var("GITHUB_CONTEXT")?)?;
    writeln!(
        hwdb_file,
        "# https://github.com/iocost-benchmark/iocost-benchmarks/commit/{}",
        context["sha"]
    )?;

    writeln!(hwdb_file, "#")?;
    writeln!(hwdb_file, "# Match key format:")?;
    writeln!(hwdb_file, "# block:<devpath>:name:<model name>:")?;
    writeln!(hwdb_file)?;

    for input in glob("hwdb-inputs/*.hwdb").unwrap().into_iter().flatten() {
        let contents = fs::read_to_string(input).expect("Failed to read input hwdb file");
        writeln!(hwdb_file, "{}", contents)?;
    }

    Ok(())
}
