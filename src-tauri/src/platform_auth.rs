//! Asks the operating system to confirm the person at the computer (gap R29): Touch ID (or the
//! Mac's password) on macOS, Windows Hello (fingerprint, face or PIN) on Windows. Linux has no
//! common API for this, so it reports [`Kind::None`]. Design and the research behind it:
//! `docs/plans/passkeys.md`.
//!
//! This only answers "did the computer's owner just confirm?". What makes it a sign-in factor is
//! [`crate::mfa::device`]: a secret kept in this computer's credential store, read only after the
//! system said yes. No fingerprint, face or PIN ever reaches LocalBooks; the system checks them.
//!
//! The file has no dependencies on the rest of the crate, so it can be type-checked on its own for
//! the macOS and Windows targets.

use serde::Serialize;

/// What this computer offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    /// macOS: Touch ID, or the Mac's own password where there is no Touch ID.
    TouchId,
    /// Windows: Windows Hello (fingerprint, face or PIN).
    WindowsHello,
    /// Nothing LocalBooks can use (Linux).
    None,
}

/// The kind this build is made for.
pub const fn kind() -> Kind {
    if cfg!(target_os = "macos") {
        Kind::TouchId
    } else if cfg!(windows) {
        Kind::WindowsHello
    } else {
        Kind::None
    }
}

pub const UNSUPPORTED: &str = "MFA_DEVICE_UNSUPPORTED: this computer has no Touch ID or Windows Hello that Tillhouse can use; use your authenticator app";
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code, reason = "only the macOS and Windows prompts can refuse"))]
pub const NOT_CONFIRMED: &str = "MFA_DEVICE_NOT_CONFIRMED: the computer did not confirm it's you; try again, or use your authenticator app";

/// Whether the system can ask right now (set up, and not locked out). Blocking: run it off the
/// async workers.
pub fn available_blocking() -> bool {
    imp::available()
}

/// Shows the system's prompt with `reason` and waits for the answer: `Ok(())` when it confirmed
/// the person. `parent` is the window the prompt belongs to (a Windows `HWND`; unused on macOS).
/// Blocking: run it off the async workers.
pub fn confirm_blocking(parent: Option<isize>, reason: &str) -> Result<(), String> {
    imp::confirm(parent, reason)
}

#[cfg(target_os = "macos")]
mod imp {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAPolicy};

    // Touch ID where the Mac has it, otherwise (or after failed fingerprints) the Mac's password:
    // the same as unlocking System Settings.
    const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthentication;

    pub fn available() -> bool {
        // SAFETY: a fresh context, used on this thread only.
        unsafe { LAContext::new().canEvaluatePolicy_error(POLICY).is_ok() }
    }

    pub fn confirm(_parent: Option<isize>, reason: &str) -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let reply = RcBlock::new(move |ok: Bool, _error: *mut NSError| {
            let _ = tx.send(ok.as_bool());
        });
        // SAFETY: the context lives until the reply arrives (we wait for it below); the reply block
        // only sends on a channel and may run on any thread.
        let context = unsafe { LAContext::new() };
        unsafe {
            context.canEvaluatePolicy_error(POLICY).map_err(|_| super::UNSUPPORTED.to_string())?;
            context.evaluatePolicy_localizedReason_reply(POLICY, &NSString::from_str(reason), &reply);
        }
        let ok = rx.recv().unwrap_or(false);
        drop(context);
        if ok {
            Ok(())
        } else {
            Err(super::NOT_CONFIRMED.into())
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows::core::HSTRING;
    use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows_future::IAsyncOperation;

    pub fn available() -> bool {
        UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.join()).is_ok_and(|a| a == UserConsentVerifierAvailability::Available)
    }

    pub fn confirm(parent: Option<isize>, reason: &str) -> Result<(), String> {
        if !available() {
            return Err(super::UNSUPPORTED.into());
        }
        // A desktop app names the window the prompt belongs to, so it opens in front of it.
        let interop = windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>().map_err(|e| format!("MFA_DEVICE: Windows Hello can't be reached ({e})"))?;
        let hwnd = HWND(parent.unwrap_or(0) as *mut core::ffi::c_void);
        // SAFETY: `hwnd` is the app's own window handle (or null, which Windows accepts).
        let op: IAsyncOperation<UserConsentVerificationResult> =
            unsafe { interop.RequestVerificationForWindowAsync(hwnd, &HSTRING::from(reason)) }.map_err(|e| format!("MFA_DEVICE: Windows Hello can't be reached ({e})"))?;
        match op.join() {
            Ok(UserConsentVerificationResult::Verified) => Ok(()),
            _ => Err(super::NOT_CONFIRMED.into()),
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    pub fn available() -> bool {
        false
    }

    pub fn confirm(_parent: Option<isize>, _reason: &str) -> Result<(), String> {
        Err(super::UNSUPPORTED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn linux_offers_nothing_and_never_confirms() {
        assert_eq!(kind(), Kind::None);
        assert!(!available_blocking());
        assert_eq!(confirm_blocking(None, "test").unwrap_err(), UNSUPPORTED);
    }

    #[test]
    fn each_platform_names_its_kind() {
        let expected = if cfg!(target_os = "macos") {
            Kind::TouchId
        } else if cfg!(windows) {
            Kind::WindowsHello
        } else {
            Kind::None
        };
        assert_eq!(kind(), expected);
    }
}
