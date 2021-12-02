use anyhow::{anyhow, Error};
use std::{io::Write, path::PathBuf, process::Command};

pub fn get_version() -> Result<(), Error> {
    // TODO return resctl version - create a struct
    /*
    let bench = Command::new(resctl_bench).args(["--version"]).output()?;

    println!("status: {}", bench.status);
    std::io::stdout().write_all(&bench.stdout).unwrap();
    std::io::stderr().write_all(&bench.stderr).unwrap();*/
    Ok(())
}

pub fn merge(resctl_bench: String, input_files: Vec<PathBuf>) -> Result<(), Error> {
    // ensure files exist
    input_files.iter().try_for_each(|x| -> Result<(), Error> {
        println!("input_file: {:?}", x);
        if !x.exists() {
            return Err(anyhow!("file does not exist"));
        }
        Ok(())
    })?;

    // call resctl-bench merge
    let mut args = Vec::<&str>::new();
    args.push("--result=out.json.gz");
    args.push("merge");
    input_files
        .iter()
        .for_each(|x| args.push(x.to_str().unwrap()));
    let bench = Command::new(resctl_bench.clone()).args(args).output()?;
    println!("merge status: {}", bench.status);
    std::io::stdout().write_all(&bench.stdout).unwrap();
    std::io::stderr().write_all(&bench.stderr).unwrap();

    // call resctl-bench summary
    let mut args = Vec::<&str>::new();
    args.push("--result=out.json.gz");
    args.push("summary");
    let bench = Command::new(resctl_bench.clone()).args(args).output()?;
    println!("summary status: {}", bench.status);
    std::io::stdout().write_all(&bench.stdout).unwrap();
    std::io::stderr().write_all(&bench.stderr).unwrap();

    // TODO get graphics

    // TODO create a wrapper to call resctl-bench
    /*
        pub fn run_command(cmd: &mut Command, emsg: &str) -> Result<()> {
        let cmd_str = format!("{:?}", &cmd);

        match cmd.status() {
            Ok(rc) if rc.success() => Ok(()),
            Ok(rc) => bail!("{:?} ({:?}): {}", &cmd_str, &rc, emsg),
            Err(e) => bail!("{:?} ({:?}): {}", &cmd_str, &e, emsg),
        }
    }
    */

    /*
    $ ./target/release/resctl-bench --result=out.json merge /home/obbardc/projects/fac0008/latest/iocost-tune-2.1/run0/970pro.json
    $ ./target/release/resctl-bench --result=out.json summary
    TODO use a format type to keep the images ?
    */

    // TODO return the output from resctl-bench
    Ok(())
}
