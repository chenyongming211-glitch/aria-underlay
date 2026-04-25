use std::sync::Arc;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::model::DeviceId;
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetconfCredentialInput {
    Password {
        username: String,
        password: String,
    },
    PrivateKey {
        username: String,
        key_pem: String,
        passphrase: Option<String>,
    },
    ExistingSecretRef {
        secret_ref: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredNetconfCredential {
    Password {
        username: String,
        password: String,
    },
    PrivateKey {
        username: String,
        key_pem: String,
        passphrase: Option<String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct InMemorySecretStore {
    inner: Arc<DashMap<String, StoredNetconfCredential>>,
}

impl InMemorySecretStore {
    pub fn create_for_device(
        &self,
        tenant_id: &str,
        site_id: &str,
        device_id: &DeviceId,
        credential: NetconfCredentialInput,
    ) -> UnderlayResult<String> {
        match credential {
            NetconfCredentialInput::ExistingSecretRef { secret_ref } => {
                if secret_ref.trim().is_empty() {
                    return Err(UnderlayError::InvalidDeviceState(
                        "secret_ref cannot be empty".into(),
                    ));
                }
                Ok(secret_ref)
            }
            NetconfCredentialInput::Password { username, password } => {
                validate_username(&username)?;
                if password.is_empty() {
                    return Err(UnderlayError::InvalidDeviceState(
                        "NETCONF password cannot be empty".into(),
                    ));
                }
                let secret_ref = device_secret_ref(tenant_id, site_id, device_id);
                self.inner.insert(
                    secret_ref.clone(),
                    StoredNetconfCredential::Password { username, password },
                );
                Ok(secret_ref)
            }
            NetconfCredentialInput::PrivateKey {
                username,
                key_pem,
                passphrase,
            } => {
                validate_username(&username)?;
                if key_pem.is_empty() {
                    return Err(UnderlayError::InvalidDeviceState(
                        "NETCONF private key cannot be empty".into(),
                    ));
                }
                let secret_ref = device_secret_ref(tenant_id, site_id, device_id);
                self.inner.insert(
                    secret_ref.clone(),
                    StoredNetconfCredential::PrivateKey {
                        username,
                        key_pem,
                        passphrase,
                    },
                );
                Ok(secret_ref)
            }
        }
    }

    pub fn get(&self, secret_ref: &str) -> Option<StoredNetconfCredential> {
        self.inner
            .get(secret_ref)
            .map(|entry| entry.value().clone())
    }
}

fn validate_username(username: &str) -> UnderlayResult<()> {
    if username.trim().is_empty() {
        return Err(UnderlayError::InvalidDeviceState(
            "NETCONF username cannot be empty".into(),
        ));
    }
    Ok(())
}

fn device_secret_ref(tenant_id: &str, site_id: &str, device_id: &DeviceId) -> String {
    format!("local/{tenant_id}/{site_id}/{}", device_id.0)
}
