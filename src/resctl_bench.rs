use anyhow::{anyhow, Error};
use std::{io::Write, path::PathBuf, process::Command};

pub async fn merge(resctl_bench: String, input_files: Vec<PathBuf>) -> Result<(), Error> {
    // ensure files exist
    input_files.iter().try_for_each(|x| -> Result<(), Error> {
        if !x.exists() {
            return Err(anyhow!("file does not exist"));
        }
        Ok(())
    })?;

    // call resctl-bench
    let bench = Command::new(resctl_bench).args(["--version"]).output()?;

    println!("status: {}", bench.status);
    std::io::stdout().write_all(&bench.stdout).unwrap();
    std::io::stderr().write_all(&bench.stderr).unwrap();

    /*
        use std::process::{self, Command};

        pub fn run_command(cmd: &mut Command, emsg: &str) -> Result<()> {
        let cmd_str = format!("{:?}", &cmd);

        match cmd.status() {
            Ok(rc) if rc.success() => Ok(()),
            Ok(rc) => bail!("{:?} ({:?}): {}", &cmd_str, &rc, emsg),
            Err(e) => bail!("{:?} ({:?}): {}", &cmd_str, &e, emsg),
        }
    }


                run_command(
                Command::new("convert")
                    .args(&[
                        "-font",
                        "Source-Code-Pro",
                        "-pointsize",
                        "7",
                        "-density",
                        "300",
                    ])
                    .arg(&text_arg)
                    .arg(&cover_pdf),
                "Are imagemagick and adobe-source-code-pro font available? \
                 Also, see https:
        */

    /*
    $ ./target/release/resctl-bench --result=out.json merge /home/obbardc/projects/fac0008/latest/iocost-tune-2.1/run0/970pro.json
    $ ./target/release/resctl-bench --result=out.json summary
    TODO use a format type to keep the images ?
    */

    // TODO return the output from resctl-bench
    Ok(())
}
