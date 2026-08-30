use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::RwLock,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::BridgeError;

const CREDENTIAL_FILE_NAME: &str = "credential.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialDocument {
    pub version: u8,
    pub credential_id: String,
    pub secret: String,
}

impl CredentialDocument {
    pub fn bearer_token(&self) -> String {
        format!("{}.{}", self.credential_id, self.secret)
    }
}

#[derive(Clone)]
struct ActiveCredential {
    id: String,
    token_digest: [u8; 32],
}

pub struct CredentialStore {
    directory: PathBuf,
    active: RwLock<ActiveCredential>,
}

impl CredentialStore {
    pub fn load_or_create(directory: impl Into<PathBuf>) -> Result<Self, BridgeError> {
        let directory = directory.into();
        prepare_private_directory(&directory)?;
        let path = directory.join(CREDENTIAL_FILE_NAME);
        let document = if path.exists() {
            read_document(&path)?
        } else {
            let document = new_document()?;
            write_document(&directory, &document)?;
            document
        };
        validate_document(&document)?;
        Ok(Self {
            directory,
            active: RwLock::new(active_credential(&document)),
        })
    }

    pub fn path(&self) -> PathBuf {
        self.directory.join(CREDENTIAL_FILE_NAME)
    }

    pub fn credential_id(&self) -> Result<String, BridgeError> {
        self.active
            .read()
            .map(|active| active.id.clone())
            .map_err(|_| BridgeError::internal("The Agent credential state is unavailable."))
    }

    pub fn authenticate(&self, supplied: &str) -> bool {
        let digest = Sha256::digest(supplied.as_bytes());
        let Ok(active) = self.active.read() else {
            return false;
        };
        active.token_digest.ct_eq(digest.as_slice()).into()
    }

    pub fn revoke_and_replace(&self) -> Result<String, BridgeError> {
        let document = new_document()?;
        let mut active = self
            .active
            .write()
            .map_err(|_| BridgeError::internal("The Agent credential state is unavailable."))?;
        write_document(&self.directory, &document)?;
        *active = active_credential(&document);
        Ok(document.credential_id)
    }
}

pub fn read_credential_token(path: &Path) -> Result<String, BridgeError> {
    Ok(read_document(path)?.bearer_token())
}

fn active_credential(document: &CredentialDocument) -> ActiveCredential {
    let digest = Sha256::digest(document.bearer_token().as_bytes());
    let mut token_digest = [0_u8; 32];
    token_digest.copy_from_slice(&digest);
    ActiveCredential {
        id: document.credential_id.clone(),
        token_digest,
    }
}

fn new_document() -> Result<CredentialDocument, BridgeError> {
    let mut id = [0_u8; 12];
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut id)
        .map_err(|_| BridgeError::internal("A local Agent credential could not be generated."))?;
    getrandom::fill(&mut secret)
        .map_err(|_| BridgeError::internal("A local Agent credential could not be generated."))?;
    Ok(CredentialDocument {
        version: 1,
        credential_id: URL_SAFE_NO_PAD.encode(id),
        secret: URL_SAFE_NO_PAD.encode(secret),
    })
}

fn validate_document(document: &CredentialDocument) -> Result<(), BridgeError> {
    if document.version != 1
        || document.credential_id.len() < 12
        || document.secret.len() < 32
        || !document
            .credential_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || !document
            .secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(BridgeError::forbidden(
            "The local Agent credential file is invalid.",
        ));
    }
    Ok(())
}

fn prepare_private_directory(path: &Path) -> Result<(), BridgeError> {
    fs::create_dir_all(path).map_err(|_| {
        BridgeError::internal("The Agent credential directory could not be created.")
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::symlink_metadata(path).map_err(|_| {
            BridgeError::internal("The Agent credential directory could not be inspected.")
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(BridgeError::forbidden(
                "The Agent credential directory must be a private local directory.",
            ));
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| {
            BridgeError::internal("The Agent credential directory could not be protected.")
        })?;
    }
    Ok(())
}

fn read_document(path: &Path) -> Result<CredentialDocument, BridgeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(path).map_err(|_| BridgeError::unauthorized())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.mode() & 0o077 != 0
        {
            return Err(BridgeError::forbidden(
                "The Agent credential file must not be accessible by group or other users.",
            ));
        }
    }
    let bytes = fs::read(path).map_err(|_| BridgeError::unauthorized())?;
    let document = serde_json::from_slice::<CredentialDocument>(&bytes)
        .map_err(|_| BridgeError::forbidden("The local Agent credential file is invalid."))?;
    validate_document(&document)?;
    Ok(document)
}

fn write_document(directory: &Path, document: &CredentialDocument) -> Result<(), BridgeError> {
    let path = directory.join(CREDENTIAL_FILE_NAME);
    let temporary = directory.join(format!(".credential-{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec(document)
        .map_err(|_| BridgeError::internal("The Agent credential could not be encoded."))?;
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|_| {
            BridgeError::internal("The Agent credential file could not be prepared.")
        })?;
        file.write_all(&bytes)
            .map_err(|_| BridgeError::internal("The Agent credential could not be saved."))?;
        file.sync_all()
            .map_err(|_| BridgeError::internal("The Agent credential could not be synced."))?;
        fs::rename(&temporary, &path)
            .map_err(|_| BridgeError::internal("The Agent credential could not be published."))?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_rotation_rejects_the_old_token_without_returning_a_secret() {
        let root = tempfile::tempdir().unwrap();
        let store = CredentialStore::load_or_create(root.path()).unwrap();
        let old = read_credential_token(&store.path()).unwrap();
        assert!(store.authenticate(&old));

        let replacement_id = store.revoke_and_replace().unwrap();
        assert!(!replacement_id.contains('.'));
        assert!(!store.authenticate(&old));
        let current = read_credential_token(&store.path()).unwrap();
        assert!(store.authenticate(&current));
    }

    #[cfg(unix)]
    #[test]
    fn credential_directory_cannot_be_a_symlink() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        let linked = root.path().join("linked");
        fs::create_dir(&real).unwrap();
        symlink(&real, &linked).unwrap();

        let error = CredentialStore::load_or_create(&linked)
            .err()
            .expect("a symlinked credential directory must be rejected");
        assert_eq!(error.code, "CAPABILITY_DENIED");
    }
}
