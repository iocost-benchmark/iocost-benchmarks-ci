use anyhow::Result;
use dashmap::DashMap;
use glob::glob;
use rayon::prelude::*;
use std::io::Write;
use std::{fs, path::PathBuf};

use crate::common::BenchMerge;

mod common;

#[tokio::main]
async fn main() -> Result<()> {
    let merges: DashMap<String, Vec<BenchMerge>> = DashMap::new();
    for entry in glob("database/*.*").unwrap().into_iter().flatten() {
        if !entry.is_dir() {
            continue;
        }

        let version = entry.file_name().unwrap().to_string_lossy().to_string();
        if version == "2.1" {
            println!("Ignoring 2.1 version, since it does not generate hwdb files.");
            continue;
        }

        let paths: Vec<PathBuf> = glob(&format!("database/{}/*", version))
            .unwrap()
            .into_iter()
            .flatten()
            .collect();

        paths.par_iter().for_each(|model_dir: &PathBuf| {
            let model_name = model_dir.file_name().unwrap().to_string_lossy().to_string();
            let merge = BenchMerge::merge(version.clone(), model_name).expect("Failed to merge");

            merge
                .save_pdf_in(&PathBuf::from("pdfs"))
                .expect("Failed to save PDF");

            merge
                .create_hwdb_in(&PathBuf::from("hwdb-inputs"))
                .expect("Failed to create a hwdb file");

            merges
                .entry(merge.model_name.clone())
                .or_insert(vec![])
                .push(merge);
        });
    }

    println!("Generating final hwdb file...");

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
    writeln!(
        hwdb_file,
        "# block:<devpath>:name:<model name>:fwrev:<firmware revision>:"
    )?;
    writeln!(hwdb_file)?;

    let models: Vec<String> = merges.iter().map(|m| m.key().clone()).collect();
    for model in models {
        // To override the hwdb file that is selected, you need to set the variable with the name
        // of the model with all dashes replaced with underscores to a value that is the preferred
        // filename. For instance:
        //
        // OVERRIDE_BEST_HFS256GD9TNG_62A0A_2022_09_19UTC=iocost-tune-2.2-HFS256GD9TNG-62A0A-2022-09-19UTC.hwdb
        let override_var = format!("OVERRIDE_BEST_{}", model.replace('-', "_"));

        let alternatives = merges.get(&model).unwrap();
        let alternatives = alternatives.value();

        // If override is available, select it, otherwise select the merge with the highest
        // number of data points.
        let best = match std::env::var(&override_var) {
            Err(std::env::VarError::NotPresent) => {
                let merge = alternatives.iter().max_by_key(|x| x.data_points).unwrap();
                let best = merge.build_descriptive_filename("hwdb", None);
                println!("{:>2} datapoints:\t{}", merge.data_points, best);
                best
            }
            Err(e) => panic!("Failed to intepret variable {}: {}", override_var, e),
            Ok(best) => {
                if !std::path::Path::exists(&PathBuf::from(&best)) {
                    panic!("Failed to find override file: {}", best);
                }
                println!("override:\t{}", best);
                best
            }
        };

        let best_hwdb = PathBuf::from("hwdb-inputs").join(best);
        let contents = fs::read_to_string(best_hwdb).expect("Failed to read input hwdb file");
        writeln!(hwdb_file, "{}", contents)?;
    }

    Ok(())
}
