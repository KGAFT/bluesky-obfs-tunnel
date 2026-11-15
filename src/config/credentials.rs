use bsobfs::{
    codec::fake_codec::ClientCredentialProvider, the_fourth_utils::async_trait::async_trait,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClientCredential {
    pub identity: String,
    pub password: String,
}

impl Default for ClientCredential {
    fn default() -> Self {
        Self {
            identity: "test-client".into(),
            password: "HelloPasswordForHandshake".into(),
        }
    }
}

#[async_trait]
impl ClientCredentialProvider for ClientCredential {
    async fn get_client_credentials(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        Some((
            self.identity.as_bytes().to_vec(),
            self.password.as_bytes().to_vec(),
        ))
    }
}
