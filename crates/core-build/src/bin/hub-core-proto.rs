#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

use std::{path::PathBuf, process::Command};

use anyhow::Context;

#[derive(Clone, Copy, clap::ValueEnum)]
enum Gen {
    Go,
}

#[derive(clap::Parser)]
struct Opts {
    /// Output directory to write Protobuf files to
    #[arg(short, long)]
    out_dir: PathBuf,

    /// If specified, generate code with the given generator
    #[arg(long, value_enum)]
    gen: Option<Gen>,

    /// TOML config path containing schema info
    config_path: PathBuf,
}

fn run(opts: Opts) -> anyhow::Result<()> {
    let Opts {
        out_dir,
        gen,
        config_path,
    } = opts;

    match std::fs::create_dir(&out_dir) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
    .context("Error creating output directory")?;

    let protos = holaplex_hub_core_build::sync_schemas(config_path, &out_dir)?;

    if let Some(gen) = gen {
        let mut cmd = Command::new("protoc");
        cmd.args(protos.keys());

        match gen {
            Gen::Go => {
                cmd.arg(format!("--go_out={}", out_dir.display()));

                for (proto, schema) in protos {
                    if let Some(m) = schema.spec.go_mod {
                        cmd.arg(format!("--go_opt=M{}={m}", proto.display()));
                    }
                }
            },
        }

        let status = cmd
            .spawn()
            .and_then(|mut s| s.wait())
            .context("Error spawning protoc")?;

        if !status.success() {
            anyhow::bail!("protoc exited with status {}", status);
        }
    }

    Ok(())
}

fn main() {
    match run(clap::Parser::parse()) {
        Ok(_protos) => (),
        Err(e) => {
            println!("ERROR: {e:?}");
            std::process::exit(-1);
        },
    }
}
