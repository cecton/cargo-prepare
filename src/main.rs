use anyhow::{bail, Context, Result};
use cargo_metadata::{CargoOpt, Metadata, MetadataCommand};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use structopt::{clap::AppSettings, StructOpt};

#[derive(StructOpt)]
#[structopt(
    bin_name = "cargo prepare",
    about = env!("CARGO_PKG_DESCRIPTION"),
    settings = &[AppSettings::TrailingVarArg, AppSettings::AllowLeadingHyphen],
)]
struct Cli {
    /// Destination of the fake workspace directory. The directory must not exists.
    #[structopt(long = "dest", short = "o", conflicts_with = "args")]
    destination: Option<PathBuf>,

    /// Rest of the arguments passed to cargo if destination is not specified.
    args: Vec<String>,
}

fn main() -> Result<()> {
    let mut args = env::args().peekable();
    let command = args.next();
    // TODO: this can be replaced by next_if() when it is released
    //       https://doc.rust-lang.org/std/iter/struct.Peekable.html#method.next_if
    if matches!(args.peek().map(|x| x.as_str()), Some("prepare")) {
        args.next();
    }
    let cli = Cli::from_iter(command.into_iter().chain(args));

    let metadata = MetadataCommand::new()
        .features(CargoOpt::AllFeatures)
        .exec()
        .context("could not read cargo metadata")?;

    if let Some(destination) = cli.destination.as_ref() {
        fs::create_dir(destination).context("could not create destination directory")?;
        initialize_fake_workspace(&metadata, destination)?;
    } else {
        let dir = tempfile::tempdir().context("could not create temporary directory")?;
        let cargo = env::var("CARGO")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("cargo"));

        initialize_fake_workspace(&metadata, dir.path())?;

        let mut command = Command::new(&cargo);
        command.env("CARGO_TARGET_DIR", metadata.target_directory);
        command.current_dir(dir.path());
        command.args(cli.args);
        let status = command
            .status()
            .context("could not execute cargo command")?;

        if !status.success() {
            bail!("cargo command failed");
        }
    }

    Ok(())
}

fn initialize_fake_workspace(metadata: &Metadata, destination: &Path) -> Result<()> {
    let lock_file = metadata.workspace_root.join("Cargo.lock");
    let tmp_lock_file = destination.join("Cargo.lock");
    fs::copy(&lock_file, &tmp_lock_file).with_context(|| {
        format!(
            "could not copy Cargo.lock file: `{}` to `{}`",
            lock_file.display(),
            tmp_lock_file.display()
        )
    })?;

    let members: HashSet<_> = metadata.workspace_members.iter().collect();
    let members: Vec<_> = metadata
        .packages
        .iter()
        .filter(|x| members.contains(&x.id))
        .collect();

    for member in members {
        let package_path = member.manifest_path.parent().unwrap();
        let relative_path = member
            .manifest_path
            .strip_prefix(&metadata.workspace_root)
            .unwrap();
        let tmp_manifest = destination.join(&relative_path);
        let tmp_package = tmp_manifest.parent().unwrap();

        fs::create_dir_all(&tmp_package).with_context(|| {
            format!(
                "could not create package directory: `{}`",
                tmp_package.display()
            )
        })?;
        fs::copy(&member.manifest_path, &tmp_manifest).with_context(|| {
            format!(
                "could not copy manifest file: `{}` to `{}`",
                member.manifest_path.display(),
                tmp_manifest.display()
            )
        })?;

        for target in member.targets.iter() {
            let relative_src_file = target.src_path.strip_prefix(&package_path).unwrap();
            let tmp_src_file = tmp_package.join(&relative_src_file);
            let tmp_src_dir = tmp_src_file.parent().unwrap();

            fs::create_dir_all(&tmp_src_dir).with_context(|| {
                format!(
                    "could not create package's subdirectory: `{}`",
                    tmp_src_dir.display()
                )
            })?;
            fs::write(&tmp_src_file, "").with_context(|| {
                format!("could not create source file: `{}`", tmp_src_file.display())
            })?;
        }
    }

    Ok(())
}
