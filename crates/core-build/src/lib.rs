//! Core build script tools for holaplex-hub

#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    clippy::clone_on_ref_ptr,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
    io::prelude::*,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use futures_util::TryFutureExt;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Config {
    registry: RegistryConfig,
    schemas: HashMap<String, SchemaSpecConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RegistryConfig {
    endpoint: url::Url,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, untagged)]
enum SchemaSpecConfig {
    Simple(usize),
    Complex(SchemaSpec),
}

impl SchemaSpecConfig {
    fn into_schema(self, subject: String) -> Schema {
        let spec = match self {
            Self::Simple(version) => SchemaSpec {
                version,
                go_mod: None,
            },
            SchemaSpecConfig::Complex(c) => c,
        };

        Schema { subject, spec }
    }
}

/// Version and configuration information for a single schema
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SchemaSpec {
    /// The version of the schema to retrieve in the registry
    pub version: usize,
    /// The module path to use when compiling the schema to Go
    pub go_mod: Option<String>,
}

/// A single entry in the TOML schema configuration
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Schema {
    /// The subject name of the schema in the registry
    pub subject: String,
    /// The version and configuration of the schema
    #[serde(flatten)]
    pub spec: SchemaSpec,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Lock<'a> {
    #[serde(default)]
    schemas: BTreeSet<Cow<'a, LockedSchema>>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct LockedSchema {
    subject: String,
    version: usize,
    #[serde(with = "hex::serde")]
    sha512: Vec<u8>,
}

type LockMap<'a> = HashMap<Cow<'a, str>, Cow<'a, LockedSchema>>;

fn not_found_opt<T>(err: std::io::Error) -> Result<Option<T>, std::io::Error> {
    if err.kind() == std::io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(err)
    }
}

fn read_lock<'a>(
    lock_map: &'a LockMap<'a>,
    schema: &'a Schema,
) -> Option<&'a Cow<'a, LockedSchema>> {
    let locked = lock_map
        .get(&Cow::Borrowed(&*schema.subject))
        .and_then(|locked| {
            let LockedSchema {
                subject,
                version,
                sha512: _,
            } = locked.as_ref();
            (subject == &schema.subject && version == &schema.spec.version).then_some(locked)
        });

    locked
}

async fn check_schema(
    path: impl AsRef<Path>,
    schema: &Schema,
    lock_map: &RwLock<LockMap<'_>>,
) -> bool {
    use sha2::Digest;
    use tokio::io::AsyncReadExt;

    let path = path.as_ref();
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };

    let mut buf = [0u8; 8192];
    let mut digest = sha2::Sha512::default();

    loop {
        let n = match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return false,
        };

        if digest.write_all(&buf[0..n]).is_err() {
            return false;
        }
    }

    let sum = digest.finalize();
    let lock_map_read = lock_map.read().await;
    let locked = read_lock(&lock_map_read, schema);

    locked.map_or(false, |l| l.sha512 == sum.as_slice())
}

async fn fetch_schema(
    out_dir: &Path,
    mut endpoint: url::Url,
    schema: Schema,
    lock_map: &RwLock<LockMap<'_>>,
) -> Result<PathBuf> {
    use futures_util::StreamExt;
    use sha2::Digest;
    use tokio::io::AsyncWriteExt;

    let path = out_dir.join(format!("{}.proto", schema.subject));

    if check_schema(&path, &schema, lock_map).await {
        return Ok(path);
    }

    endpoint
        .path_segments_mut()
        .map_err(|()| anyhow!("Invalid registry endpoint"))?
        .push("subjects")
        .push(&schema.subject)
        .push("versions")
        .push(&schema.spec.version.to_string())
        .push("schema");

    let res = reqwest::get(endpoint.clone())
        .await
        .context("HTTP request failed")?;

    if !res.status().is_success() {
        anyhow::bail!(
            "request to {:?} returned {}",
            endpoint.as_str(),
            res.status().as_u16()
        );
    }

    let mut outf = tokio::fs::File::create(&path)
        .await
        .with_context(|| format!("Failed to create {path:?}"))?;
    let mut bytes = res.bytes_stream();
    let mut digest = sha2::Sha512::default();

    while let Some(chunk) = bytes.next().await {
        let chunk = chunk.context("Reading HTTP body failed")?;
        digest
            .write_all(chunk.as_ref())
            .context("Failed to update checksum")?;
        outf.write_all(chunk.as_ref())
            .await
            .with_context(|| format!("Failed to write to {path:?}"))?;
    }

    let sum = digest.finalize();
    let lock_map_read = lock_map.read().await;
    let locked = read_lock(&lock_map_read, &schema);

    if let Some(locked) = locked {
        anyhow::ensure!(
            locked.sha512 == sum.as_slice(),
            "Checksum mismatch for {}@{}",
            schema.subject,
            schema.spec.version
        );
    } else {
        drop(lock_map_read);
        let mut lock_map_write = lock_map.write().await;
        let Schema {
            subject,
            spec: SchemaSpec { version, go_mod: _ },
        } = schema;
        lock_map_write.insert(
            Cow::Owned(subject.clone()),
            Cow::Owned(LockedSchema {
                subject,
                version,
                sha512: sum.to_vec(),
            }),
        );
    }

    Ok(path)
}

/// Download Protobuf schemas requested by the TOML config file at the given
/// config path to the specified output directory
///
/// # Errors
/// Fails if the schemas cannot successfully be downloaded and compiled or if
/// a lockfile validation error occurs.
pub fn sync_schemas(
    config_path: impl AsRef<Path>,
    out_dir: impl AsRef<Path>,
) -> Result<HashMap<PathBuf, Schema>> {
    let config_path = config_path.as_ref();
    let lock_path = config_path.with_extension("lock");
    let mut config = String::new();
    std::fs::File::open(config_path)
        .with_context(|| format!("Failed to open {config_path:?}"))?
        .read_to_string(&mut config)
        .with_context(|| format!("Failed to read {config_path:?}"))?;
    let Config { registry, schemas } = toml::from_str(&config)?;

    let mut lock = String::new();
    std::fs::File::open(&lock_path)
        .map(Some)
        .or_else(not_found_opt)
        .with_context(|| format!("Failed to open {lock_path:?}"))?
        .map(|mut f| {
            f.read_to_string(&mut lock)
                .with_context(|| format!("Failed to read {lock_path:?}"))
        })
        .transpose()?;
    let Lock {
        schemas: locked_schemas,
    } = toml::from_str(&lock).context("Failed to deserialize lockfile")?;

    let lock_map: LockMap = locked_schemas
        .iter()
        .map(|s| (Cow::Borrowed(&*s.subject), Cow::Borrowed(s.as_ref())))
        .collect();

    let (protos, new_lock_map) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Initializing Tokio failed")?
        .block_on(async {
            let lock_map = RwLock::new(lock_map.clone());

            futures_util::future::join_all(schemas.into_iter().map(|(subj, spec)| {
                let schema = spec.into_schema(subj);

                fetch_schema(
                    out_dir.as_ref(),
                    registry.endpoint.clone(),
                    schema.clone(),
                    &lock_map,
                )
                .map_ok(|p| (p, schema))
            }))
            .await
            .into_iter()
            .collect::<Result<HashMap<_, _>>>()
            .map(|p| (p, lock_map.into_inner()))
        })
        .context("Couldn't fetch all requested schemas")?;

    if new_lock_map != lock_map {
        let lock = toml::to_string(&Lock {
            schemas: new_lock_map
                .values()
                .map(|c| Cow::Borrowed(c.as_ref()))
                .collect(),
        })
        .context("Failed to serialize new lockfile")?;

        let mut tmp = tempfile::NamedTempFile::new_in(
            lock_path
                .parent()
                .context("Couldn't locate containing directory of lockfile")?,
        )
        .context("Failed to open lockfile for writing")?;
        tmp.write_all(lock.as_bytes())
            .context("Failed to write new lockfile")?;

        tmp.persist(&lock_path)
            .with_context(|| format!("Failed to save new lockfile to {lock_path:?}"))?;
    }

    Ok(protos)
}

/// Load and compile Protobuf schemas requested by the TOML config file at the
/// given path
///
/// # Errors
/// Fails if the schemas cannot successfully be downloaded and compiled or if
/// a lockfile validation error occurs.
pub fn run(config_path: impl AsRef<Path>) -> Result<()> {
    println!(
        "cargo:rerun-if-changed={}",
        config_path.as_ref().to_string_lossy()
    );

    let out_dir = PathBuf::try_from(std::env::var("OUT_DIR")?)?;
    let protos = sync_schemas(config_path, &out_dir)?;

    if !protos.is_empty() {
        prost_build::compile_protos(&protos.into_keys().collect::<Box<[_]>>(), &[out_dir])
            .context("Error compiling schemas")?;
    }

    Ok(())
}
