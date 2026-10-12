//! Native-only credential boundary. No plaintext-file or in-memory-vault fallback.
use zeroize::Zeroizing;

const SERVICE: &str = "com.projectos.openrouter";
const ACCOUNT: &str = "api-key";
const FAILURE: &str = "OS credential storage is unavailable or access was denied.";

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn entry() -> Result<keyring::Entry, String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| FAILURE.to_string())?;
    #[cfg(target_os = "macos")]
    let native = entry.get_credential().is::<keyring::macos::MacCredential>();
    #[cfg(target_os = "linux")]
    let native = entry.get_credential().is::<keyring::secret_service::SsCredential>();
    if !native { return Err(FAILURE.into()); }
    Ok(entry)
}

pub fn configured() -> Result<bool, String> {
    match load() {
        Ok(_) => Ok(true),
        Err(error) if error == "No OpenRouter API key is stored." => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn store(api_key: String) -> Result<(), String> {
    let api_key = Zeroizing::new(api_key);
    if !(16..=512).contains(&api_key.len())
        || !api_key.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err("Enter a valid OpenRouter API key.".into());
    }
    store_native(&api_key)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn store_native(api_key: &str) -> Result<(), String> {
    entry()?.set_password(api_key).map_err(|_| FAILURE.to_string())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn load() -> Result<Zeroizing<String>, String> {
    match entry()?.get_password() {
        Ok(key) => Ok(Zeroizing::new(key)),
        Err(keyring::Error::NoEntry) => Err("No OpenRouter API key is stored.".into()),
        Err(_) => Err(FAILURE.into()),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn forget() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(FAILURE.into()),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn store_native(_: &str) -> Result<(), String> { Err(FAILURE.into()) }
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn load() -> Result<Zeroizing<String>, String> { Err(FAILURE.into()) }
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn forget() -> Result<(), String> { Err(FAILURE.into()) }

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_credentials_fail_before_native_vault_access() {
        for invalid in ["", "secret\nheader", "short", "sk-or-xxxxxxxxxxx\r\n"] {
            assert_eq!(super::store(invalid.into()).unwrap_err(), "Enter a valid OpenRouter API key.");
        }
    }

    /// Isolated synthetic item only. Never reads or writes the production namespace.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit OS Keychain opt-in: PROJECTOS_TEST_KEYRING=1"]
    fn native_isolated_keyring_conformance() {
        assert_eq!(std::env::var("PROJECTOS_TEST_KEYRING").as_deref(), Ok("1"));
        let service = format!("com.projectos.test.{}", uuid::Uuid::new_v4());
        assert_ne!(service, super::SERVICE);
        let entry = keyring::Entry::new(&service, "synthetic-test-item").expect("Create isolated credential handle.");
        assert!(entry.get_credential().is::<keyring::macos::MacCredential>(), "The configured backend must be macOS Keychain, never the mock backend.");
        struct Cleanup<'a>(&'a keyring::Entry);
        impl Drop for Cleanup<'_> { fn drop(&mut self) { let _ = self.0.delete_credential(); } }
        let _cleanup = Cleanup(&entry);
        let synthetic = zeroize::Zeroizing::new("projectos-synthetic-conformance-not-an-api-key".to_string());
        entry.set_password(&synthetic).expect("Store synthetic item in native Keychain.");
        let reopened = keyring::Entry::new(&service, "synthetic-test-item").expect("Reopen isolated native item.");
        let actual = zeroize::Zeroizing::new(reopened.get_password().expect("Read isolated synthetic item."));
        assert!(actual.as_str() == synthetic.as_str(), "The reopened native item must retain its exact synthetic value.");
        reopened.delete_credential().expect("Remove isolated synthetic item.");
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    }
}
