# Passkeys, Touch ID and Windows Hello for two-step sign-in (R29)

Status: **built natively for macOS and Windows; not on Linux.** Optional, and off unless a user
adds a computer, beside an authenticator app (R26, [`multi-factor-sign-in.md`](multi-factor-sign-in.md)).
Gap R29 in [`docs/gaps.md`](../gaps.md). Migration 0175.

## The goal

R29's acceptance test: a user can add a platform authenticator (a passkey, Touch ID or Windows
Hello) beside the authenticator app and sign in with it on that computer; the app stays for copies
of the file on other computers. Two-step sign-in stays optional and off by default.

## Research: what works inside a Tauri v2 window

LocalBooks' screens run in the system webview: WKWebView on macOS, WebView2 on Windows, WebKitGTK
on Linux. The page is served from Tauri's own scheme (`tauri://localhost` on macOS and Linux,
`http://tauri.localhost` on Windows), not from a website LocalBooks controls.

### WebAuthn (passkeys) in the webview

- **macOS, WKWebView.** WebAuthn works in an app's WKWebView only when the app holds the
  *Associated Domains* entitlement with a `webcredentials:` entry for the relying party's domain,
  and that domain serves an `apple-app-site-association` file naming the app. Apple engineers say
  so on the developer forums ("WKWebView supports WebAuthn as long as your app is using Associated
  Domains … with the RPID that you're using"). The entitlement needs a provisioning profile, and the
  domain needs a web server. LocalBooks has no server and must not depend on one (AGENTS.md rule
  13: nothing leaves the computer), and Tauri's `tauri://localhost` origin can't be associated. The
  alternative entitlement for browsers (`com.apple.developer.web-browser.public-key-credential`) is
  granted by Apple to web browsers only. **Not usable.**
- **Windows, WebView2.** WebView2 is Chromium and passes WebAuthn to the Windows WebAuthn API
  (`webauthn.dll`), which offers Windows Hello as a platform authenticator. Whether it accepts
  `tauri.localhost` as a relying-party id isn't documented for embedded apps, and Microsoft's own
  answers report embedded WebViews that don't fully support WebAuthn. Even where it works, it would
  be Windows only, so it buys nothing over calling Windows directly. **Possible on Windows only,
  not relied on.**
- **Linux, WebKitGTK.** WebKitGTK doesn't implement WebAuthn: WebKit bug 205350 ("[WPE][GTK]
  Support WebAuthn", open since 2019) tracks it, and as of WebKitGTK 2.50 sites report it missing.
  Linux also has no common platform authenticator. **Not usable.**

### Native APIs from Rust

- **macOS: LocalAuthentication** (`LAContext evaluatePolicy`). Asks for Touch ID, or the Mac's
  password where there is no Touch ID or after failed fingerprints
  (`deviceOwnerAuthentication`). Needs no entitlement or provisioning profile; works from an
  unsigned development build. Reached through the `objc2-local-authentication` crate (pure Rust
  bindings, the same `objc2` family Tauri already uses).
- **macOS: the platform passkey API** (`ASAuthorizationPlatformPublicKeyCredentialProvider`) has
  the same Associated Domains requirement as WKWebView. **Not usable.** Community Tauri plugins
  that wrap it (`tauri-plugin-macos-passkey`, `tauri-plugin-webauthn`) inherit that requirement.
- **macOS: a Secure Enclave key** with an access control requiring user presence would bind the
  factor to the hardware, but on macOS it lives in the data-protection keychain, which needs the
  `keychain-access-groups` entitlement and so a provisioning profile. **Later**, once the release
  build is signed that way.
- **Windows: `UserConsentVerifier`** (Windows Hello: fingerprint, face or PIN). A desktop app calls
  it through `IUserConsentVerifierInterop::RequestVerificationForWindowAsync` with its window, so the
  prompt opens in front of LocalBooks. Reached through the `windows` crate LocalBooks already uses.
- **Windows: `KeyCredentialManager`** (a Windows Hello key pair, in the TPM where there is one) or
  `webauthn.dll` with no browser would bind the factor to the hardware too. Both have known rough
  edges for desktop (non-packaged) apps, such as the prompt opening behind the window, and need
  RSA or COSE/CBOR handling. **Later**, with the macOS Secure Enclave key.
- **Linux:** fingerprint readers go through `fprintd` and polkit, which differ between
  distributions and aren't on most desktops by default. **Not built**; the option isn't offered.

## Design

One shape on both platforms, so one piece of Rust holds the rules and one set of tests covers them:

1. **The system confirms the person** ([`platform_auth.rs`](../../src-tauri/src/platform_auth.rs)):
   LocalAuthentication on macOS, `UserConsentVerifier` on Windows, nothing on Linux. It answers only
   "confirmed or not"; LocalBooks never sees a fingerprint, face or PIN.
2. **A secret on this computer** ([`mfa/device.rs`](../../src-tauri/src/mfa/device.rs)): adding a
   computer makes a random 256-bit secret and a random 16-byte id. The secret goes into the
   computer's credential store ([`secrets.rs`](../../src-tauri/src/secrets.rs): the macOS Keychain
   or Windows Credential Manager) under the id. The company file keeps the id, a label (the
   computer's name), and SHA-256 of the secret bound to the user and the id
   (`user_device_factors`, migration 0175). The secret is never in the company file, its backups,
   the audit trail or an error.
3. **Signing in**, after a right password, at the code step: if this computer holds a secret that
   matches one of the user's rows, the screen offers **Use Touch ID** / **Use Windows Hello**. The
   system confirms the person, LocalBooks reads the secret, compares its hash, and finishes the held
   sign-in. The code box and backup codes stay on the same step.

Why a secret and not just the system's yes: a yes alone would let any computer with Touch ID pass,
including someone else's computer with a copy of the file. The secret ties the factor to the
computer it was added on, which is what "on that computer" in the acceptance test means. Why a hash
and not a key pair: everything is checked on the same computer, from the same file, so a public
key would add nothing over a hash of a 256-bit random secret.

### The rules

- **Beside the app, never instead of it.** A computer can only be added by a user with an
  authenticator app (checked in Rust and by a trigger), because the app is what works on every other
  computer and copy of the file, and what the backup codes belong to.
- **What removes the app removes the computers** (a trigger on `user_factors`): turning it off, an
  Admin's reset, a temporary password from an Admin, the recovery key, the company setting going to
  Off, and removing all users. So every way back from R26 still works and a lost laptop stops
  signing in. **Set Up Again…** replaces the app in place and keeps the computers.
- **Adding asks for the password again** (with the sign-in wait, gap R27) and then the system's
  check. Adding the same computer again replaces its row and secret. Removing a computer needs no
  password: the app's code is still required there afterwards, so it never weakens sign-in.
- **The audit trail** records `MFA_DEVICE_ADDED` and `MFA_DEVICE_REMOVED` with the computer's
  label, never the secret.
- **Secrets left behind** (a computer removed from another computer, or by an Admin) stay in that
  computer's store but match no row, so they sign nobody in. Turning the app off on this computer
  clears this computer's secrets for that user.

## What it protects, and what it doesn't

The same limits as R26 apply: it guards the LocalBooks sign-in, not the company file, and a
compromised computer sees what the user sees. In addition:

- **The fallback is the computer's own sign-in.** Touch ID falls back to the Mac's password, and
  Windows Hello always offers a PIN. Someone who knows both the LocalBooks password and the
  computer's password or PIN can sign in on that computer. The guide says so, and a business that
  wants a phone for everyone keeps to the app.
- **The secret is protected by the operating system's account**, not by hardware. Another program
  running as the same user could read it from the credential store. That program could equally
  edit the company file, which is already outside what two-step sign-in can stop.
- **On Linux** nothing is offered; users there keep the app.

## Left for later

- A hardware-bound key (Secure Enclave on macOS, TPM via `KeyCredentialManager` on Windows) once
  the release build carries the entitlements and the desktop prompt issues are worked through.
- Linux, if a common API appears (or WebKitGTK gains WebAuthn with a platform authenticator).

## Sources

- Apple Developer Forums, "Using WKWebView and a yubikey?" (thread 726943) and "Clarifying WebAuthN
  web API availability on Web Views" (thread 714785): WebAuthn in WKWebView needs Associated Domains.
- Apple Developer Forums, thread 829169: the web-browser public-key-credential entitlement.
- passkeys.dev, macOS reference: the `apple-app-site-association` requirement.
- Microsoft Learn, "WebAuthn APIs for passwordless authentication on Windows".
- Microsoft Learn, `UserConsentVerifier` and `IUserConsentVerifierInterop`.
- WebKit bug 205350, "[WPE][GTK] Support WebAuthn".
