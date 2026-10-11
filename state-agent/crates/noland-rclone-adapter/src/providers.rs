use crate::{
    AdapterCredential, AdapterError, AdapterInput, ProviderKind, RcloneProviderAdapter,
    RcloneRemoteConfig, Result,
};
use aes::cipher::{BlockEncrypt, KeyInit};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;

const RCLONE_OBSCURE_KEY: [u8; 32] = [
    0x9c, 0x93, 0x5b, 0x48, 0x73, 0x0a, 0x55, 0x4d, 0x6b, 0xfd, 0x7c, 0x63, 0xc8, 0x86, 0xa9, 0x2b,
    0xd3, 0x90, 0x19, 0x8e, 0xb8, 0x12, 0x8a, 0xfb, 0xf4, 0xde, 0x16, 0x2b, 0x8b, 0x95, 0xf6, 0x38,
];

pub fn adapter_for(kind: ProviderKind) -> Box<dyn RcloneProviderAdapter> {
    Box::new(Dispatcher(kind))
}

pub struct Dispatcher(pub ProviderKind);

impl RcloneProviderAdapter for Dispatcher {
    fn backend_type(&self) -> &'static str {
        self.0.backend_type()
    }

    fn create_config(&self, input: &AdapterInput) -> Result<RcloneRemoteConfig> {
        let name = input.remote_name.clone();
        let fields = &input.fields;
        match (self.0, &input.credentials) {
            (
                ProviderKind::BackblazeB2,
                AdapterCredential::BackblazeB2 {
                    key_id,
                    application_key,
                },
            ) => Ok(RcloneRemoteConfig::new(name, "b2")
                .entry("account", key_id)
                .entry("key", application_key)),
            (
                ProviderKind::AmazonS3,
                AdapterCredential::S3 {
                    access_key_id,
                    secret_access_key,
                    session_token,
                },
            ) => {
                let mut cfg = RcloneRemoteConfig::new(name, "s3")
                    .entry("provider", "AWS")
                    .entry("access_key_id", access_key_id)
                    .entry("secret_access_key", secret_access_key)
                    .entry(
                        "region",
                        fields
                            .get("region")
                            .cloned()
                            .unwrap_or_else(|| "us-east-1".into()),
                    );
                if let Some(token) = session_token.as_ref().filter(|v| !v.is_empty()) {
                    cfg = cfg.entry("session_token", token);
                }
                Ok(cfg)
            }
            (
                ProviderKind::CloudflareR2,
                AdapterCredential::S3 {
                    access_key_id,
                    secret_access_key,
                    ..
                },
            ) => {
                let account_id = fields.get("account_id").cloned().ok_or_else(|| {
                    AdapterError::Invalid("Cloudflare R2 account_id is missing.".into())
                })?;
                Ok(RcloneRemoteConfig::new(name, "s3")
                    .entry("provider", "Cloudflare")
                    .entry("access_key_id", access_key_id)
                    .entry("secret_access_key", secret_access_key)
                    .entry("region", "auto")
                    .entry(
                        "endpoint",
                        format!("https://{account_id}.r2.cloudflarestorage.com"),
                    ))
            }
            (
                ProviderKind::Wasabi,
                AdapterCredential::S3 {
                    access_key_id,
                    secret_access_key,
                    ..
                },
            ) => {
                let mut cfg = RcloneRemoteConfig::new(name, "s3")
                    .entry("provider", "Wasabi")
                    .entry("access_key_id", access_key_id)
                    .entry("secret_access_key", secret_access_key)
                    .entry(
                        "region",
                        fields
                            .get("region")
                            .cloned()
                            .unwrap_or_else(|| "us-east-1".into()),
                    );
                if let Some(endpoint) = fields.get("endpoint").filter(|v| !v.trim().is_empty()) {
                    cfg = cfg.entry("endpoint", endpoint);
                }
                Ok(cfg)
            }
            (
                ProviderKind::DigitalOceanSpaces,
                AdapterCredential::S3 {
                    access_key_id,
                    secret_access_key,
                    ..
                },
            ) => {
                let region = fields.get("region").cloned().ok_or_else(|| {
                    AdapterError::Invalid("DigitalOcean Spaces region is missing.".into())
                })?;
                Ok(RcloneRemoteConfig::new(name, "s3")
                    .entry("provider", "DigitalOcean")
                    .entry("access_key_id", access_key_id)
                    .entry("secret_access_key", secret_access_key)
                    .entry("region", &region)
                    .entry("endpoint", format!("{region}.digitaloceanspaces.com")))
            }
            (
                ProviderKind::GenericS3,
                AdapterCredential::S3 {
                    access_key_id,
                    secret_access_key,
                    ..
                },
            ) => {
                let mut cfg = RcloneRemoteConfig::new(name, "s3")
                    .entry("provider", "Other")
                    .entry("access_key_id", access_key_id)
                    .entry("secret_access_key", secret_access_key)
                    .entry(
                        "region",
                        fields
                            .get("region")
                            .cloned()
                            .unwrap_or_else(|| "auto".into()),
                    );
                if let Some(endpoint) = fields.get("endpoint") {
                    cfg = cfg.entry("endpoint", endpoint);
                }
                if let Some(v) = fields.get("force_path_style") {
                    cfg = cfg.entry(
                        "force_path_style",
                        if v == "true" || v == "1" {
                            "true"
                        } else {
                            "false"
                        },
                    );
                }
                Ok(cfg)
            }
            (ProviderKind::GoogleDrive, AdapterCredential::OAuth2 { .. }) => {
                oauth_config(name, "drive", input)
            }
            (ProviderKind::MicrosoftOneDrive, AdapterCredential::OAuth2 { .. }) => {
                let drive_id = required_config_field(fields, "drive_id", "OneDrive drive ID")?;
                let drive_type =
                    required_config_field(fields, "drive_type", "OneDrive drive type")?;
                Ok(oauth_config(name, "onedrive", input)?
                    .entry("drive_id", drive_id)
                    .entry("drive_type", drive_type))
            }
            (ProviderKind::Dropbox, AdapterCredential::OAuth2 { .. }) => {
                oauth_config(name, "dropbox", input)
            }
            (ProviderKind::Box, AdapterCredential::OAuth2 { .. }) => {
                oauth_config(name, "box", input)
            }
            (ProviderKind::GoogleCloudStorage, AdapterCredential::ServiceAccount { json }) => {
                let json = serde_json::from_str::<serde_json::Value>(json)
                    .map_err(|error| {
                        AdapterError::Invalid(format!(
                            "Invalid Google Cloud service account JSON: {error}"
                        ))
                    })?
                    .to_string();
                Ok(RcloneRemoteConfig::new(name, "google cloud storage")
                    .entry("service_account_credentials", json))
            }
            (
                ProviderKind::AzureBlob,
                AdapterCredential::UsernamePassword { username, password },
            ) => Ok(RcloneRemoteConfig::new(name, "azureblob")
                .entry("account", username)
                .entry("key", password)),
            (ProviderKind::Sftp, AdapterCredential::UsernamePassword { username, password }) => {
                let host = fields
                    .get("host")
                    .cloned()
                    .ok_or_else(|| AdapterError::Invalid("SFTP host is missing.".into()))?;
                let mut cfg = RcloneRemoteConfig::new(name, "sftp")
                    .entry("host", host)
                    .entry("user", username)
                    .entry("pass", obscure_password(password)?);
                if let Some(port) = fields.get("port").filter(|v| !v.trim().is_empty()) {
                    cfg = cfg.entry("port", port);
                }
                if let Some(host_keys) = fields.get("host_keys").filter(|v| !v.trim().is_empty()) {
                    cfg = cfg.entry("host_keys", host_keys);
                }
                Ok(cfg)
            }
            (
                ProviderKind::Sftp,
                AdapterCredential::SshKey {
                    username,
                    private_key,
                    passphrase,
                },
            ) => {
                let host = required_config_field(fields, "host", "SFTP host")?;
                let mut cfg = RcloneRemoteConfig::new(name, "sftp")
                    .entry("host", host)
                    .entry("user", username)
                    .entry(
                        "key_pem",
                        private_key.replace("\r\n", "\\n").replace('\n', "\\n"),
                    );
                if let Some(port) = fields.get("port").filter(|v| !v.trim().is_empty()) {
                    cfg = cfg.entry("port", port);
                }
                if let Some(passphrase) = passphrase.as_ref().filter(|v| !v.is_empty()) {
                    cfg = cfg.entry("key_file_pass", obscure_password(passphrase)?);
                }
                if let Some(host_keys) = fields.get("host_keys").filter(|v| !v.trim().is_empty()) {
                    cfg = cfg.entry("host_keys", host_keys);
                }
                Ok(cfg)
            }
            (ProviderKind::Webdav, AdapterCredential::UsernamePassword { username, password }) => {
                let url = fields
                    .get("url")
                    .cloned()
                    .ok_or_else(|| AdapterError::Invalid("WebDAV URL is missing.".into()))?;
                let vendor = fields
                    .get("vendor")
                    .cloned()
                    .unwrap_or_else(|| "other".into());
                let mut config = RcloneRemoteConfig::new(name, "webdav")
                    .entry("url", url)
                    .entry("vendor", &vendor)
                    .entry("user", username)
                    .entry("pass", obscure_password(password)?);
                if vendor == "nextcloud" {
                    config = config.entry("nextcloud_chunk_size", "64M");
                }
                Ok(config)
            }
            (ProviderKind::Local, AdapterCredential::LocalPath { path }) => {
                Ok(RcloneRemoteConfig::new(name, "local").entry("nounc", path))
            }
            _ => Err(AdapterError::Invalid(format!(
                "Provider {} is not compatible with the stored credential format.",
                self.0.label()
            ))),
        }
    }
}

fn required_config_field(
    fields: &std::collections::BTreeMap<String, String>,
    key: &str,
    label: &str,
) -> Result<String> {
    fields
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AdapterError::Invalid(format!("{label} is missing.")))
}

/// Produces the format required by rclone's password options. This is
/// compatibility encoding, not encryption; the containing config remains
/// operation-scoped and permission-restricted.
fn obscure_password(password: &str) -> Result<String> {
    let mut iv = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut iv);
    let cipher = aes::Aes256::new_from_slice(&RCLONE_OBSCURE_KEY).map_err(|error| {
        AdapterError::Invalid(format!("Initialize rclone password encoder: {error}"))
    })?;
    let mut output = Vec::with_capacity(iv.len() + password.len());
    output.extend_from_slice(&iv);

    let mut counter = u128::from_be_bytes(iv);
    for chunk in password.as_bytes().chunks(16) {
        let mut stream_block = aes::cipher::Block::<aes::Aes256>::from(counter.to_be_bytes());
        cipher.encrypt_block(&mut stream_block);
        output.extend(
            chunk
                .iter()
                .zip(stream_block.iter())
                .map(|(plain, key)| plain ^ key),
        );
        counter = counter.wrapping_add(1);
    }

    Ok(URL_SAFE_NO_PAD.encode(output))
}

fn oauth_config(name: String, backend: &str, input: &AdapterInput) -> Result<RcloneRemoteConfig> {
    let AdapterCredential::OAuth2 {
        access_token,
        refresh_token,
        expires_at,
    } = &input.credentials
    else {
        return Err(AdapterError::Invalid("OAuth credentials required".into()));
    };
    let mut cfg = RcloneRemoteConfig::new(name, backend);
    if let Some(client_id) = input
        .fields
        .get("client_id")
        .filter(|v| !v.trim().is_empty())
    {
        cfg = cfg.entry("client_id", client_id);
    }
    if let Some(client_secret) = input
        .fields
        .get("client_secret")
        .filter(|v| !v.trim().is_empty())
    {
        cfg = cfg.entry("client_secret", client_secret);
    }
    cfg = cfg.entry(
        "token",
        oauth_token_json(access_token, refresh_token.as_deref(), *expires_at),
    );
    Ok(cfg)
}

fn oauth_token_json(access_token: &str, refresh_token: Option<&str>, expires_at: i64) -> String {
    let expiry = chrono::DateTime::<chrono::Utc>::from_timestamp(expires_at, 0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();
    let mut value = serde_json::json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expiry": expiry,
    });
    if let Some(refresh) = refresh_token {
        value["refresh_token"] = serde_json::Value::String(refresh.to_string());
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{session_from_input, TokenMode};
    use std::collections::BTreeMap;

    fn b2_input() -> AdapterInput {
        AdapterInput {
            provider: ProviderKind::BackblazeB2,
            remote_name: "noland_b2".into(),
            credentials: AdapterCredential::BackblazeB2 {
                key_id: "kid".into(),
                application_key: "secret".into(),
            },
            fields: BTreeMap::from([("bucket".into(), "noland-backups".into())]),
            bucket: Some("noland-backups".into()),
            prefix: Some("repos".into()),
        }
    }

    #[test]
    fn b2_config_and_bucket_root() {
        let session = session_from_input(&b2_input(), "op-1", TokenMode::Ephemeral).unwrap();
        assert!(session.config_ini.contains("type = b2"));
        assert!(session.config_ini.contains("account = kid"));
        assert_eq!(session.backend_type, "b2");
        assert_eq!(session.root, "noland-backups/repos");
        assert_eq!(session.provider, "backblaze_b2");
    }

    #[test]
    fn r2_uses_cloudflare_endpoint() {
        let input = AdapterInput {
            provider: ProviderKind::CloudflareR2,
            remote_name: "noland_r2".into(),
            credentials: AdapterCredential::S3 {
                access_key_id: "ak".into(),
                secret_access_key: "sk".into(),
                session_token: None,
            },
            fields: BTreeMap::from([
                ("account_id".into(), "acct".into()),
                ("bucket".into(), "b".into()),
            ]),
            bucket: Some("b".into()),
            prefix: None,
        };
        let session = session_from_input(&input, "op", TokenMode::Ephemeral).unwrap();
        assert!(session.config_ini.contains("type = s3"));
        assert!(session.config_ini.contains("provider = Cloudflare"));
        assert!(session
            .config_ini
            .contains("endpoint = https://acct.r2.cloudflarestorage.com"));
        assert_eq!(session.root, "b");
    }

    #[test]
    fn drive_ephemeral_omits_refresh_token() {
        let input = AdapterInput {
            provider: ProviderKind::GoogleDrive,
            remote_name: "noland_drive".into(),
            credentials: AdapterCredential::OAuth2 {
                access_token: "ya29.access".into(),
                refresh_token: Some("1//refresh".into()),
                expires_at: 1_700_000_000,
            },
            fields: BTreeMap::from([("folder".into(), "Noland Shared Storage".into())]),
            bucket: None,
            prefix: None,
        };
        let ephemeral = session_from_input(&input, "op", TokenMode::Ephemeral).unwrap();
        assert!(ephemeral.config_ini.contains("type = drive"));
        assert!(ephemeral.config_ini.contains("ya29.access"));
        assert!(
            !ephemeral.config_ini.contains("1//refresh"),
            "ephemeral session must not contain the refresh token"
        );
        assert_eq!(ephemeral.root, "Noland Shared Storage");

        let operation = session_from_input(&input, "op", TokenMode::Operation).unwrap();
        assert!(operation.config_ini.contains("1//refresh"));
        assert!(TokenMode::Operation.is_ephemeral());

        let durable = session_from_input(&input, "op", TokenMode::Durable).unwrap();
        assert!(durable.config_ini.contains("1//refresh"));
        assert!(!TokenMode::Durable.is_ephemeral());
    }

    #[test]
    fn local_backend_for_tests() {
        let input = AdapterInput {
            provider: ProviderKind::Local,
            remote_name: "noland_local".into(),
            credentials: AdapterCredential::LocalPath {
                path: "/tmp/cloud".into(),
            },
            fields: BTreeMap::new(),
            bucket: None,
            prefix: Some("Noland Shared Storage".into()),
        };
        let session = session_from_input(&input, "op", TokenMode::Ephemeral).unwrap();
        assert!(session.config_ini.contains("type = local"));
        assert_eq!(session.backend_type, "local");
    }

    fn input(
        provider: ProviderKind,
        credentials: AdapterCredential,
        fields: &[(&str, &str)],
    ) -> AdapterInput {
        AdapterInput {
            provider,
            remote_name: format!("noland_{}", provider.as_str()),
            credentials,
            fields: fields
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect(),
            bucket: None,
            prefix: None,
        }
    }

    fn s3_credentials() -> AdapterCredential {
        AdapterCredential::S3 {
            access_key_id: "access".into(),
            secret_access_key: "secret".into(),
            session_token: None,
        }
    }

    fn oauth_credentials() -> AdapterCredential {
        AdapterCredential::OAuth2 {
            access_token: "access-token".into(),
            refresh_token: Some("refresh-token".into()),
            expires_at: 1_900_000_000,
        }
    }

    #[test]
    fn every_production_provider_generates_its_expected_backend() {
        let cases = vec![
            input(
                ProviderKind::AmazonS3,
                s3_credentials(),
                &[("bucket", "bucket")],
            ),
            b2_input(),
            input(
                ProviderKind::CloudflareR2,
                s3_credentials(),
                &[("account_id", "account"), ("bucket", "bucket")],
            ),
            input(
                ProviderKind::Wasabi,
                s3_credentials(),
                &[("region", "us-east-1"), ("bucket", "bucket")],
            ),
            input(
                ProviderKind::DigitalOceanSpaces,
                s3_credentials(),
                &[("region", "nyc3"), ("space_name", "space")],
            ),
            input(
                ProviderKind::GenericS3,
                s3_credentials(),
                &[("endpoint", "https://s3.example.com"), ("bucket", "bucket")],
            ),
            input(ProviderKind::GoogleDrive, oauth_credentials(), &[]),
            input(
                ProviderKind::GoogleCloudStorage,
                AdapterCredential::ServiceAccount { json: "{}".into() },
                &[("bucket", "bucket")],
            ),
            input(
                ProviderKind::MicrosoftOneDrive,
                oauth_credentials(),
                &[("drive_id", "drive-id"), ("drive_type", "personal")],
            ),
            input(ProviderKind::Dropbox, oauth_credentials(), &[]),
            input(ProviderKind::Box, oauth_credentials(), &[]),
            input(
                ProviderKind::AzureBlob,
                AdapterCredential::UsernamePassword {
                    username: "account".into(),
                    password: "key".into(),
                },
                &[("container", "container")],
            ),
            input(
                ProviderKind::Sftp,
                AdapterCredential::UsernamePassword {
                    username: "user".into(),
                    password: "password".into(),
                },
                &[("host", "sftp.example.com"), ("remote_path", "/srv/noland")],
            ),
            input(
                ProviderKind::Webdav,
                AdapterCredential::UsernamePassword {
                    username: "user".into(),
                    password: "password".into(),
                },
                &[("url", "https://dav.example.com"), ("vendor", "other")],
            ),
        ];

        for case in cases {
            let expected_backend = case.provider.backend_type();
            let session = session_from_input(&case, "all-providers", TokenMode::Operation)
                .unwrap_or_else(|error| panic!("{} config failed: {error}", case.provider.label()));
            assert_eq!(
                session.backend_type,
                expected_backend,
                "{}",
                case.provider.label()
            );
            assert!(session
                .config_ini
                .contains(&format!("type = {expected_backend}")));
        }
    }

    #[test]
    fn password_backends_obscure_secrets_and_sftp_preserves_absolute_root() {
        for provider in [ProviderKind::Sftp, ProviderKind::Webdav] {
            let fields = if provider == ProviderKind::Sftp {
                vec![("host", "sftp.example.com"), ("remote_path", "/srv/noland")]
            } else {
                vec![
                    ("url", "https://dav.example.com"),
                    ("remote_path", "noland"),
                ]
            };
            let case = input(
                provider,
                AdapterCredential::UsernamePassword {
                    username: "user".into(),
                    password: "plain-password".into(),
                },
                &fields,
            );
            let session = session_from_input(&case, "password", TokenMode::Operation).unwrap();
            assert!(!session.config_ini.contains("pass = plain-password"));
            let obscured = session
                .config_ini
                .lines()
                .find_map(|line| line.strip_prefix("pass = "))
                .unwrap();
            assert_eq!(reveal_password(obscured), "plain-password");
            if provider == ProviderKind::Sftp {
                assert_eq!(session.root, "/srv/noland");
            }
        }
    }

    #[test]
    fn onedrive_rejects_incomplete_noninteractive_config() {
        let case = input(ProviderKind::MicrosoftOneDrive, oauth_credentials(), &[]);
        assert!(session_from_input(&case, "onedrive", TokenMode::Operation).is_err());
    }

    #[test]
    fn nextcloud_webdav_uses_larger_upload_chunks() {
        let case = input(
            ProviderKind::Webdav,
            AdapterCredential::UsernamePassword {
                username: "user".into(),
                password: "secret".into(),
            },
            &[
                ("url", "https://cloud.example.test/dav"),
                ("vendor", "nextcloud"),
            ],
        );
        let session = session_from_input(&case, "webdav", TokenMode::Ephemeral).unwrap();
        assert!(session.config_ini.contains("nextcloud_chunk_size = 64M"));
    }

    fn reveal_password(value: &str) -> String {
        let mut ciphertext = URL_SAFE_NO_PAD.decode(value).unwrap();
        let iv: [u8; 16] = ciphertext[..16].try_into().unwrap();
        let cipher = aes::Aes256::new_from_slice(&RCLONE_OBSCURE_KEY).unwrap();
        let mut counter = u128::from_be_bytes(iv);
        for chunk in ciphertext[16..].chunks_mut(16) {
            let mut stream_block = aes::cipher::Block::<aes::Aes256>::from(counter.to_be_bytes());
            cipher.encrypt_block(&mut stream_block);
            for (value, key) in chunk.iter_mut().zip(stream_block.iter()) {
                *value ^= key;
            }
            counter = counter.wrapping_add(1);
        }
        String::from_utf8(ciphertext[16..].to_vec()).unwrap()
    }
}
