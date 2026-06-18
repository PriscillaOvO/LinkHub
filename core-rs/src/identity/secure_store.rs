//! Platform-specific at-rest protection for the local identity's secret keys.
//!
//! Windows uses DPAPI (`CryptProtectData`). macOS uses the Keychain and Linux
//! uses the Secret Service; on those platforms the on-disk ciphertext is a JSON
//! reference to the OS-managed secret rather than the secret itself. Unknown
//! platforms return an `Unsupported` error at runtime.

use std::collections::HashMap;
use std::io;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::sha256_hex;
use super::{
    decode_hex, encode_hex, SECURE_LOCAL_IDENTITY_HEADER,
    SECURE_LOCAL_IDENTITY_PLATFORM_LINUX_SECRET_SERVICE,
    SECURE_LOCAL_IDENTITY_PLATFORM_MACOS_KEYCHAIN, SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI,
};

pub(super) fn format_secure_local_identity_file(encrypted_bytes: &[u8]) -> String {
    let platform = secure_platform_label();
    [
        SECURE_LOCAL_IDENTITY_HEADER.to_string(),
        format!("platform={platform}"),
        format!("ciphertext={}", encode_hex(encrypted_bytes)),
    ]
    .join("\n")
}

fn secure_platform_label() -> &'static str {
    #[cfg(windows)]
    {
        SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI
    }
    #[cfg(target_os = "macos")]
    {
        SECURE_LOCAL_IDENTITY_PLATFORM_MACOS_KEYCHAIN
    }
    #[cfg(target_os = "linux")]
    {
        SECURE_LOCAL_IDENTITY_PLATFORM_LINUX_SECRET_SERVICE
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI
    } // fallback for unknown platforms, will fail at runtime
}

pub(super) fn parse_secure_local_identity_file(value: &str) -> Result<Vec<u8>, String> {
    let mut lines = value.lines();
    let Some(header) = lines.next() else {
        return Err("missing secure local identity header".to_string());
    };

    if header.trim_start_matches('\u{feff}') != SECURE_LOCAL_IDENTITY_HEADER {
        return Err("unsupported secure local identity file".to_string());
    }

    let fields = lines
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect::<HashMap<_, _>>();
    let platform = fields
        .get("platform")
        .ok_or_else(|| "secure local identity missing platform".to_string())?;

    if platform != SECURE_LOCAL_IDENTITY_PLATFORM_WINDOWS_DPAPI
        && platform != SECURE_LOCAL_IDENTITY_PLATFORM_MACOS_KEYCHAIN
        && platform != SECURE_LOCAL_IDENTITY_PLATFORM_LINUX_SECRET_SERVICE
    {
        return Err(format!(
            "unsupported secure local identity platform: {platform}"
        ));
    }

    let ciphertext = fields
        .get("ciphertext")
        .ok_or_else(|| "secure local identity missing ciphertext".to_string())?;

    decode_hex(ciphertext)
}

#[cfg(windows)]
pub(super) fn protect_local_identity_bytes(plaintext: &[u8]) -> io::Result<Vec<u8>> {
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len().try_into().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "local identity is too large")
        })?,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let result = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if result == 0 {
        return Err(io::Error::last_os_error());
    }

    let encrypted = unsafe {
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let encrypted = slice.to_vec();
        LocalFree(output.pbData as *mut _);
        encrypted
    };

    Ok(encrypted)
}

#[cfg(target_os = "macos")]
pub(super) fn protect_local_identity_bytes(plaintext: &[u8]) -> io::Result<Vec<u8>> {
    let hash_prefix = &sha256_hex(plaintext)[..8];
    let service = format!("linkhub-identity-{hash_prefix}");
    let account = "linkhub-local-identity";
    security_framework::passwords::set_generic_password(&service, account, plaintext)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("keychain write: {err}")))?;
    let ref_json = serde_json::json!({"service": service, "account": account}).to_string();
    Ok(ref_json.into_bytes())
}

#[cfg(target_os = "macos")]
pub(super) fn unprotect_local_identity_bytes(encrypted: &[u8]) -> io::Result<Vec<u8>> {
    let ref_str = std::str::from_utf8(encrypted)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let v: serde_json::Value =
        serde_json::from_str(ref_str).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let service = v["service"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing service"))?;
    let account = v["account"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing account"))?;
    let (password, _) = security_framework::passwords::get_generic_password(service, account)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("keychain read: {err}")))?;
    Ok(password)
}

#[cfg(target_os = "linux")]
pub(super) fn protect_local_identity_bytes(plaintext: &[u8]) -> io::Result<Vec<u8>> {
    // Linux: store via Secret Service using async block_on
    let rt = tokio::runtime::Runtime::new().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    rt.block_on(async {
        let ss = secret_service::SecretService::connect(secret_service::EncryptionType::Dh)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let collection = ss
            .get_default_collection()
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let hash_prefix = &sha256_hex(plaintext)[..8];
        let label = format!("linkhub-identity-{hash_prefix}");
        let mut props = std::collections::HashMap::new();
        props.insert("application", "linkhub-desktop");
        let item = collection
            .create_item(&label, props, plaintext, false, "text/plain")
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let path = item.get_path().unwrap_or_default();
        let ref_json = serde_json::json!({"item_path": path}).to_string();
        Ok(ref_json.into_bytes())
    })
}

#[cfg(target_os = "linux")]
pub(super) fn unprotect_local_identity_bytes(encrypted: &[u8]) -> io::Result<Vec<u8>> {
    let ref_str = std::str::from_utf8(encrypted)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let v: serde_json::Value =
        serde_json::from_str(ref_str).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let item_path = v["item_path"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing item_path"))?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    rt.block_on(async {
        let ss = secret_service::SecretService::connect(secret_service::EncryptionType::Dh)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let mut props = std::collections::HashMap::new();
        props.insert("path", item_path);
        let items = ss
            .search_items(props)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let item = items
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "secret not found"))?;
        let secret = item
            .get_secret()
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(secret)
    })
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub(super) fn protect_local_identity_bytes(_plaintext: &[u8]) -> io::Result<Vec<u8>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure local identity storage is not available on this platform",
    ))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub(super) fn unprotect_local_identity_bytes(_encrypted: &[u8]) -> io::Result<Vec<u8>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure local identity storage is not available on this platform",
    ))
}

#[cfg(windows)]
pub(super) fn unprotect_local_identity_bytes(encrypted: &[u8]) -> io::Result<Vec<u8>> {
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len().try_into().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "secure local identity is too large",
            )
        })?,
        pbData: encrypted.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let result = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if result == 0 {
        return Err(io::Error::last_os_error());
    }

    let plaintext = unsafe {
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let plaintext = slice.to_vec();
        LocalFree(output.pbData as *mut _);
        plaintext
    };

    Ok(plaintext)
}
