use super::{ClientSettings, ServerSettings, validation::invalid};
use serde::{Serialize, de::DeserializeOwned};
use std::{io, path::Path};
use tokio::{fs, io::AsyncWriteExt};

async fn load<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let bytes = fs::read(path).await.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot read {}: {error}", path.display()),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid JSON in {}: {error}", path.display()),
        )
    })
}

async fn save<T: Serialize>(settings: &T, path: &Path, create_new: bool) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(settings).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let mut options = fs::OpenOptions::new();
    options.write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).await?;
    file.write_all(&bytes).await?;
    file.flush().await
}

macro_rules! config_file {
    ($ty:ident) => {
        impl $ty {
            pub async fn load(path: impl AsRef<Path>) -> io::Result<Self> {
                let settings: Self = load(path.as_ref()).await?;
                settings.validate().map_err(|problems| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "invalid settings in {}: {}",
                            path.as_ref().display(),
                            problems.join("; ")
                        ),
                    )
                })?;
                Ok(settings)
            }

            pub async fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
                self.validate().map_err(invalid)?;
                save(self, path.as_ref(), false).await
            }

            /// Existing malformed files are fatal. Only missing files get defaults.
            pub async fn load_or_create(path: impl AsRef<Path>) -> io::Result<Self> {
                let path = path.as_ref();
                match Self::load(path).await {
                    Ok(settings) => {
                        eprintln!("Loaded config from {}", path.display());
                        Ok(settings)
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        let settings = Self::default();
                        match save(&settings, path, true).await {
                            Ok(()) => {
                                eprintln!("Wrote default config to {}", path.display());
                                Ok(settings)
                            }
                            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                                Self::load(path).await
                            }
                            Err(error) => Err(error),
                        }
                    }
                    Err(error) => Err(error),
                }
            }
        }
    };
}

config_file!(ServerSettings);
config_file!(ClientSettings);
