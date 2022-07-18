use anyhow::Result;
use glob::glob;
use std::path::PathBuf;

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

            merged.push(merge);
        }
    }

    Ok(())
}
