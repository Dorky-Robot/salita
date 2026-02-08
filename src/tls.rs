use std::net::IpAddr;
use std::path::{Path, PathBuf};

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};

/// Paths to TLS certificate files within the data directory.
#[derive(Debug, Clone)]
pub struct TlsPaths {
    pub ca_cert: PathBuf,
    pub ca_key: PathBuf,
    pub server_cert: PathBuf,
    pub server_key: PathBuf,
    pub instance_marker: PathBuf,
}

impl TlsPaths {
    pub fn new(data_dir: &Path) -> Self {
        let tls_dir = data_dir.join("tls");
        Self {
            ca_cert: tls_dir.join("ca.crt"),
            ca_key: tls_dir.join("ca.key"),
            server_cert: tls_dir.join("server.crt"),
            server_key: tls_dir.join("server.key"),
            instance_marker: tls_dir.join(".instance"),
        }
    }

    pub fn dir(&self) -> &Path {
        self.ca_cert.parent().unwrap()
    }

    pub fn certs_exist(&self) -> bool {
        self.ca_cert.exists()
            && self.ca_key.exists()
            && self.server_cert.exists()
            && self.server_key.exists()
    }

    /// Check if the marker file matches the current instance_name.
    pub fn instance_matches(&self, instance_name: &str) -> bool {
        self.instance_marker
            .exists()
            .then(|| std::fs::read_to_string(&self.instance_marker).ok())
            .flatten()
            .map(|stored| stored.trim() == instance_name)
            .unwrap_or(false)
    }
}

/// Ensure TLS certificates exist, generating them if missing. Returns the paths.
/// If the instance_name has changed since certs were last generated, regenerates them
/// so that the CA Organization field matches (fixes "from null" on Android).
pub fn ensure_certs(data_dir: &Path, instance_name: &str) -> anyhow::Result<TlsPaths> {
    let paths = TlsPaths::new(data_dir);

    if paths.certs_exist() && paths.instance_matches(instance_name) {
        tracing::info!("TLS certificates found at {}", paths.dir().display());
        return Ok(paths);
    }

    // Delete old certs if they exist but instance_name changed
    if paths.certs_exist() {
        tracing::info!(
            "Instance name changed, regenerating TLS certificates in {}",
            paths.dir().display()
        );
        let _ = std::fs::remove_file(&paths.ca_cert);
        let _ = std::fs::remove_file(&paths.ca_key);
        let _ = std::fs::remove_file(&paths.server_cert);
        let _ = std::fs::remove_file(&paths.server_key);
        let _ = std::fs::remove_file(&paths.instance_marker);
    }

    tracing::info!("Generating TLS certificates in {}", paths.dir().display());
    std::fs::create_dir_all(paths.dir())?;

    // Generate CA
    let (ca_cert, ca_key_pair) = generate_ca(instance_name)?;
    let ca_cert_pem = ca_cert.pem();
    let ca_key_pem = ca_key_pair.serialize_pem();
    std::fs::write(&paths.ca_cert, &ca_cert_pem)?;
    std::fs::write(&paths.ca_key, &ca_key_pem)?;

    // Restrict private key permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&paths.ca_key, std::fs::Permissions::from_mode(0o600))?;
    }

    // Generate server cert signed by CA
    let (server_cert_pem, server_key_pem) = generate_server_cert(&ca_cert, &ca_key_pair)?;
    std::fs::write(&paths.server_cert, &server_cert_pem)?;
    std::fs::write(&paths.server_key, &server_key_pem)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&paths.server_key, std::fs::Permissions::from_mode(0o600))?;
    }

    // Write instance_name marker for future comparisons
    std::fs::write(&paths.instance_marker, instance_name)?;

    tracing::info!("TLS certificates generated successfully");

    // Attempt to trust CA on macOS
    attempt_macos_trust(&paths.ca_cert);

    Ok(paths)
}

/// Generate a self-signed CA certificate, valid for 10 years.
fn generate_ca(instance_name: &str) -> anyhow::Result<(rcgen::Certificate, KeyPair)> {
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    // Organization must come before CommonName — Android reads the O field
    // from the cert to display in the install dialog, and some versions only
    // parse it correctly when O precedes CN in the DN sequence.
    params
        .distinguished_name
        .push(DnType::OrganizationName, instance_name);
    params
        .distinguished_name
        .push(DnType::CommonName, format!("{} Local CA", instance_name));
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2034, 1, 1);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    Ok((cert, key_pair))
}

/// Generate a server certificate signed by the CA, valid for 2 years.
/// Includes SANs for localhost, 127.0.0.1, all detected LAN IPs, and hostname.local.
fn generate_server_cert(
    ca_cert: &rcgen::Certificate,
    ca_key: &KeyPair,
) -> anyhow::Result<(String, String)> {
    let mut params = CertificateParams::default();
    // Note: server cert CN doesn't show on Android install dialog;
    // the CA's Organization field is what Android displays.
    params
        .distinguished_name
        .push(DnType::CommonName, "Salita Server");
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2026, 12, 31);
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];

    // Build SANs
    let mut sans = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::IpAddress(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
    ];

    // Add LAN IP
    if let Ok(ip) = local_ip_address::local_ip() {
        sans.push(SanType::IpAddress(ip));
    }

    // Add all network IPs
    if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (_name, ip) in &ifaces {
            if !ip.is_loopback() {
                let san = SanType::IpAddress(*ip);
                if !sans.contains(&san) {
                    sans.push(san);
                }
            }
        }
    }

    // Add hostname.local via system command
    if let Ok(output) = std::process::Command::new("hostname").output() {
        let hostname_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !hostname_str.is_empty() {
            let local_name = if hostname_str.ends_with(".local") {
                hostname_str
            } else {
                format!("{}.local", hostname_str)
            };
            if let Ok(dns_name) = local_name.try_into() {
                sans.push(SanType::DnsName(dns_name));
            }
        }
    }

    params.subject_alt_names = sans;

    let server_key = KeyPair::generate()?;
    let server_cert = params.signed_by(&server_key, ca_cert, ca_key)?;

    Ok((server_cert.pem(), server_key.serialize_pem()))
}

/// Load TLS certificates into an axum-server RustlsConfig.
pub async fn load_rustls_config(
    paths: &TlsPaths,
) -> anyhow::Result<axum_server::tls_rustls::RustlsConfig> {
    let config =
        axum_server::tls_rustls::RustlsConfig::from_pem_file(&paths.server_cert, &paths.server_key)
            .await?;
    Ok(config)
}

/// Generate an iOS .mobileconfig profile containing the CA certificate.
pub fn generate_mobileconfig(ca_cert_path: &Path, instance_name: &str) -> anyhow::Result<Vec<u8>> {
    let ca_pem = std::fs::read_to_string(ca_cert_path)?;

    // Extract the DER bytes from PEM
    let der_bytes = extract_der_from_pem(&ca_pem)?;

    // Build the mobileconfig plist
    let payload_uuid = uuid::Uuid::now_v7().to_string();
    let profile_uuid = uuid::Uuid::now_v7().to_string();

    let cert_payload = plist::Dictionary::from_iter([
        (
            "PayloadContent".to_string(),
            plist::Value::Data(der_bytes),
        ),
        (
            "PayloadDescription".to_string(),
            plist::Value::String(format!("Adds the {} Local CA to trusted roots", instance_name)),
        ),
        (
            "PayloadDisplayName".to_string(),
            plist::Value::String(format!("{} Local CA", instance_name)),
        ),
        (
            "PayloadIdentifier".to_string(),
            plist::Value::String(format!("com.salita.local-ca.{}", payload_uuid)),
        ),
        (
            "PayloadType".to_string(),
            plist::Value::String("com.apple.security.root".to_string()),
        ),
        (
            "PayloadUUID".to_string(),
            plist::Value::String(payload_uuid),
        ),
        (
            "PayloadVersion".to_string(),
            plist::Value::Integer(1.into()),
        ),
    ]);

    let profile = plist::Dictionary::from_iter([
        (
            "PayloadContent".to_string(),
            plist::Value::Array(vec![plist::Value::Dictionary(cert_payload)]),
        ),
        (
            "PayloadDisplayName".to_string(),
            plist::Value::String(format!("{} Secure Connection", instance_name)),
        ),
        (
            "PayloadIdentifier".to_string(),
            plist::Value::String("com.salita.local-tls".to_string()),
        ),
        (
            "PayloadRemovalDisallowed".to_string(),
            plist::Value::Boolean(false),
        ),
        (
            "PayloadType".to_string(),
            plist::Value::String("Configuration".to_string()),
        ),
        (
            "PayloadUUID".to_string(),
            plist::Value::String(profile_uuid),
        ),
        (
            "PayloadVersion".to_string(),
            plist::Value::Integer(1.into()),
        ),
        (
            "PayloadDescription".to_string(),
            plist::Value::String(format!(
                "Installs the {} CA certificate for secure local connections",
                instance_name
            )),
        ),
    ]);

    let mut buf = Vec::new();
    plist::to_writer_xml(&mut buf, &plist::Value::Dictionary(profile))?;
    Ok(buf)
}

/// Extract DER bytes from a PEM-encoded certificate.
fn extract_der_from_pem(pem: &str) -> anyhow::Result<Vec<u8>> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    let cert = certs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No certificate found in PEM"))?;
    Ok(cert.as_ref().to_vec())
}

/// On macOS, trust the CA in the user's login keychain (no admin password needed).
#[cfg(all(target_os = "macos", not(test)))]
fn attempt_macos_trust(ca_cert_path: &Path) {
    tracing::info!("Adding CA certificate to login keychain...");

    let home = std::env::var("HOME").unwrap_or_default();
    let login_keychain = format!("{}/Library/Keychains/login.keychain-db", home);

    let status = std::process::Command::new("security")
        .args([
            "add-trusted-cert",
            "-r",
            "trustRoot",
            "-k",
            &login_keychain,
        ])
        .arg(ca_cert_path)
        .status();

    match status {
        Ok(s) if s.success() => {
            tracing::info!("CA certificate trusted in login keychain");
        }
        Ok(s) => {
            tracing::warn!(
                "Could not trust CA certificate (exit code: {}). \
                 Browsers will show a security warning until the certificate is trusted.",
                s.code().unwrap_or(-1)
            );
        }
        Err(e) => {
            tracing::warn!("Failed to run 'security' command: {}", e);
        }
    }
}

#[cfg(not(all(target_os = "macos", not(test))))]
fn attempt_macos_trust(_ca_cert_path: &Path) {
    // No-op on non-macOS platforms and during tests
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_paths_structure() {
        let paths = TlsPaths::new(Path::new("/home/user/.salita"));
        assert_eq!(paths.ca_cert, PathBuf::from("/home/user/.salita/tls/ca.crt"));
        assert_eq!(paths.ca_key, PathBuf::from("/home/user/.salita/tls/ca.key"));
        assert_eq!(
            paths.server_cert,
            PathBuf::from("/home/user/.salita/tls/server.crt")
        );
        assert_eq!(
            paths.server_key,
            PathBuf::from("/home/user/.salita/tls/server.key")
        );
    }

    #[test]
    fn tls_paths_dir() {
        let paths = TlsPaths::new(Path::new("/data"));
        assert_eq!(paths.dir(), Path::new("/data/tls"));
    }

    #[test]
    fn generate_certs_and_idempotency() {
        let tmp = tempfile::tempdir().unwrap();
        let paths1 = ensure_certs(tmp.path(), "Salita").unwrap();
        assert!(paths1.certs_exist());
        assert!(paths1.instance_marker.exists());

        // Read the CA cert content
        let ca1 = std::fs::read_to_string(&paths1.ca_cert).unwrap();

        // Second call should be idempotent (not regenerate)
        let paths2 = ensure_certs(tmp.path(), "Salita").unwrap();
        let ca2 = std::fs::read_to_string(&paths2.ca_cert).unwrap();
        assert_eq!(ca1, ca2);
    }

    #[test]
    fn regenerates_certs_when_instance_name_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let paths1 = ensure_certs(tmp.path(), "Salita").unwrap();
        let ca1 = std::fs::read_to_string(&paths1.ca_cert).unwrap();

        // Change instance_name — should regenerate
        let paths2 = ensure_certs(tmp.path(), "My Home Server").unwrap();
        let ca2 = std::fs::read_to_string(&paths2.ca_cert).unwrap();
        assert_ne!(ca1, ca2);

        // Marker should now say "My Home Server"
        let marker = std::fs::read_to_string(&paths2.instance_marker).unwrap();
        assert_eq!(marker, "My Home Server");
    }

    #[test]
    fn generate_mobileconfig_produces_valid_plist() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ensure_certs(tmp.path(), "TestInstance").unwrap();
        let config_bytes = generate_mobileconfig(&paths.ca_cert, "TestInstance").unwrap();
        let config_str = String::from_utf8(config_bytes).unwrap();
        assert!(config_str.contains("PayloadType"));
        assert!(config_str.contains("com.apple.security.root"));
        assert!(config_str.contains("TestInstance Local CA"));
    }

    #[cfg(unix)]
    #[test]
    fn private_keys_have_restricted_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let paths = ensure_certs(tmp.path(), "Salita").unwrap();
        let ca_key_mode = std::fs::metadata(&paths.ca_key)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let server_key_mode = std::fs::metadata(&paths.server_key)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(ca_key_mode, 0o600);
        assert_eq!(server_key_mode, 0o600);
    }
}
