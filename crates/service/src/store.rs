use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::error::AppError;

#[derive(Clone)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub async fn new(root: impl AsRef<Path>) -> Result<Self, AppError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).await?;
        Ok(Self { root })
    }

    pub async fn put(&self, bytes: &[u8]) -> Result<(String, String), AppError> {
        let digest = hex::encode(Sha256::digest(bytes));
        let relative = format!("sha256/{}/{digest}", &digest[..2]);
        let destination = self.root.join(&relative);
        if fs::try_exists(&destination).await? {
            return Ok((format!("sha256:{digest}"), relative));
        }
        let parent = destination.parent().expect("hashed paths have a parent");
        fs::create_dir_all(parent).await?;
        let temporary = parent.join(format!(".{digest}.{}.tmp", uuid::Uuid::new_v4()));
        fs::write(&temporary, bytes).await?;
        match fs::rename(&temporary, &destination).await {
            Ok(()) => {}
            Err(_error) if fs::try_exists(&destination).await? => {
                fs::remove_file(&temporary).await?;
            }
            Err(error) => return Err(error.into()),
        }
        Ok((format!("sha256:{digest}"), relative))
    }

    pub async fn read(&self, relative: &str) -> Result<Vec<u8>, AppError> {
        if relative.contains("..") || Path::new(relative).is_absolute() {
            return Err(AppError::validation(
                "invalid_artifact_path",
                "artifact path escapes store",
            ));
        }
        Ok(fs::read(self.root.join(relative)).await?)
    }
}
