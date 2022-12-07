#![feature(adt_const_params)]

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{env, sync::Arc};

mod authenticate;
mod authorize;
mod error;

pub use authenticate::*;
pub use authorize::*;
pub use error::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionResponse {
    pub policy: Vec<Vec<String>>,
    pub group_policy: Vec<Vec<String>>,
}

#[derive(Clone)]
pub struct SecretKey(pub Arc<Hmac<Sha256>>);

impl SecretKey {
    pub fn from_env() -> Self {
        let secret_key: String = env::var("IDP_SECRET_KEY").expect("IDP_SECRET_KEY not set");
        let secret_key = base64::decode(&secret_key).unwrap();
        let secret_key = Hmac::<Sha256>::new_from_slice(&secret_key)
            .expect("Failed to create HMAC from secret key");

        SecretKey(Arc::new(secret_key))
    }
}

#[derive(Default, Clone)]
pub struct IdpClient(pub reqwest::Client);

pub struct Variables {
    pub idp_refresh_address: String,
    pub idp_login_address: String,
    pub token_duration_seconds: u32,
    pub service_name: String,
}

impl Variables {
    pub fn from_env() -> Self {
        Variables {
            idp_login_address: env::var("IDP_LOGIN_ADDR").expect("IDP_LOGIN_ADDR not set"),
            idp_refresh_address: env::var("IDP_REFRESH_ADDR").expect("IDP_REFRESH_ADDR not set"),
            token_duration_seconds: env::var("TOKEN_DURATION_SECONDS").expect("TOKEN_DURATION_SECONDS not set").parse().expect("Expected integer"),
            service_name: env::var("SERVICE_NAME").expect("SERVICE_NAME not set"),
        }
    }
}
