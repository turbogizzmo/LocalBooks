//! QuickBooks Desktop for Windows through the QuickBooks SDK's request processor,
//! `QBXMLRP2.RequestProcessor`, a COM object Intuit's free SDK installs. LocalBooks opens a
//! connection as "LocalBooks", begins a session on the company file open in QuickBooks, sends its
//! queries, and ends the session.
//!
//! The first connection makes QuickBooks ask whether to let LocalBooks read the company file (its
//! Application Certificate window); refusals come back as the HRESULTs mapped in [`explain`].

use std::mem::ManuallyDrop;

use ::windows::core::{BSTR, GUID, HRESULT, PCWSTR};
use ::windows::Win32::System::Com::{
    CLSIDFromProgID, CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, CLSCTX_ALL, COINIT_APARTMENTTHREADED, DISPATCH_METHOD, DISPPARAMS, EXCEPINFO,
};
use ::windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BSTR, VT_I4};

use super::{Status, Transport};

const PROG_ID: &str = "QBXMLRP2.RequestProcessor";
const APP_NAME: &str = "LocalBooks";
/// `QBXMLRPConnectionType.localQBD`: QuickBooks Desktop on this computer.
const LOCAL_QBD: i32 = 1;
/// `QBFileMode.qbFileOpenDoNotCare`: whichever mode the company file is open in.
const FILE_OPEN_DO_NOT_CARE: i32 = 2;
const SDK_URL: &str = "https://developer.intuit.com/app/developer/qbdesktop/docs/get-started/download-and-install-the-sdk";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// COM, initialised for this thread while the value lives.
struct Com;

impl Com {
    fn init() -> Result<Com, String> {
        // S_FALSE (already initialised) is success too; it still needs its CoUninitialize.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().map_err(|e| format!("QB_SDK_FAILED: Windows could not start COM ({e})"))?;
        Ok(Com)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn class_id() -> Option<GUID> {
    let id = wide(PROG_ID);
    unsafe { CLSIDFromProgID(PCWSTR(id.as_ptr())) }.ok()
}

pub fn status() -> Status {
    let sdk_installed = Com::init().map(|_com| class_id().is_some()).unwrap_or(false);
    Status { platform: Some("windows"), sdk_installed, ..Status::default() }
}

/// What to do about an error the request processor reported, by its HRESULT (from the SDK's
/// documented error codes), with its own description.
fn explain(code: HRESULT, description: &str) -> String {
    let certificate = "In QuickBooks, open the company file as the Admin user, then try again: when QuickBooks asks whether to let LocalBooks \
                       read this company file, choose \"Yes, whenever this QuickBooks company file is open\" and Continue";
    let what = match code.0 as u32 {
        0x8004_0408 => "QB_NOT_RUNNING: QuickBooks could not be started. Open QuickBooks and your company file, then try again".to_string(),
        0x8004_0416 | 0x8004_0417 => "QB_NO_COMPANY: no company file is open in QuickBooks. Open your company file, then try again".to_string(),
        0x8004_0414 => "QB_BUSY: QuickBooks is showing a message or dialog. Close it, then try again".to_string(),
        0x8004_0418 => format!("QB_NOT_ALLOWED: LocalBooks has not been allowed to read this company file yet. {certificate}"),
        0x8004_041A => "QB_NOT_ALLOWED: LocalBooks is not allowed to read this company file. In QuickBooks, choose Edit › Preferences › Integrated Applications › \
                        Company Preferences, select LocalBooks and allow it access, then try again"
            .to_string(),
        0x8004_0420 => format!("QB_ACCESS_DENIED: QuickBooks was told not to let LocalBooks in. {certificate}"),
        _ => format!("QB_SDK_FAILED: QuickBooks did not answer LocalBooks ({:#010x})", code.0 as u32),
    };
    match description.trim() {
        "" => what,
        d => format!("{what} (QuickBooks said: {d})"),
    }
}

fn bstr_variant(text: &str) -> VARIANT {
    let mut v = VARIANT::default();
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_BSTR;
        inner.Anonymous.bstrVal = ManuallyDrop::new(BSTR::from(text));
    }
    v
}

fn i4_variant(value: i32) -> VARIANT {
    let mut v = VARIANT::default();
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = value;
    }
    v
}

/// Calls a method by name with the arguments given (first to last), returning its result as text
/// when it is a string.
fn call(object: &IDispatch, method: &str, mut args: Vec<VARIANT>) -> Result<Option<String>, String> {
    let name = wide(method);
    let names = [PCWSTR(name.as_ptr())];
    let mut id = 0i32;
    unsafe { object.GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, 0, &mut id) }.map_err(|e| explain(e.code(), &format!("{method}: {}", e.message())))?;
    // IDispatch takes its arguments last to first.
    args.reverse();
    let params = DISPPARAMS { rgvarg: args.as_mut_ptr(), rgdispidNamedArgs: std::ptr::null_mut(), cArgs: args.len() as u32, cNamedArgs: 0 };
    let mut result = VARIANT::default();
    let mut info = EXCEPINFO::default();
    let outcome = unsafe { object.Invoke(id, &GUID::zeroed(), 0, DISPATCH_METHOD, &params, Some(&mut result), Some(&mut info), None) };
    let text = unsafe {
        let inner = &*result.Anonymous.Anonymous;
        (inner.vt == VT_BSTR).then(|| inner.Anonymous.bstrVal.to_string())
    };
    unsafe {
        for a in args.iter_mut() {
            let _ = VariantClear(a);
        }
        let _ = VariantClear(&mut result);
    }
    let description = info.bstrDescription.to_string();
    let code = if info.scode != 0 { HRESULT(info.scode) } else { outcome.as_ref().err().map(|e| e.code()).unwrap_or(HRESULT(0)) };
    unsafe {
        ManuallyDrop::drop(&mut info.bstrSource);
        ManuallyDrop::drop(&mut info.bstrDescription);
        ManuallyDrop::drop(&mut info.bstrHelpFile);
    }
    match outcome {
        Ok(()) => Ok(text),
        Err(e) => Err(explain(code, &if description.is_empty() { e.message() } else { description })),
    }
}

/// A session with the company file open in QuickBooks.
pub struct Sdk {
    // Dropped in this order, after `Drop::drop` ends the session: the processor, then COM.
    processor: IDispatch,
    ticket: String,
    connected: bool,
    _com: Com,
}

/// Connects to QuickBooks and begins a session on the open company file. QuickBooks asks the
/// user to allow LocalBooks the first time.
pub fn connect() -> Result<Sdk, String> {
    let com = Com::init()?;
    let Some(clsid) = class_id() else {
        return Err(format!(
            "QB_SDK_MISSING: the QuickBooks SDK is not installed on this computer. LocalBooks asks QuickBooks for your data through it. \
             Download the free QuickBooks Desktop SDK from Intuit ({SDK_URL}), install it, then try again"
        ));
    };
    let processor: IDispatch = unsafe { CoCreateInstance(&clsid, None, CLSCTX_ALL) }.map_err(|e| {
        format!(
            "QB_SDK_MISSING: the QuickBooks SDK's request processor could not be started ({}). Install the latest QuickBooks Desktop SDK from Intuit ({SDK_URL}), then try again",
            e.message()
        )
    })?;
    let mut sdk = Sdk { processor, ticket: String::new(), connected: false, _com: com };
    call(&sdk.processor, "OpenConnection2", vec![bstr_variant(""), bstr_variant(APP_NAME), i4_variant(LOCAL_QBD)])?;
    sdk.connected = true;
    // An empty file name means the company file open in QuickBooks.
    sdk.ticket = call(&sdk.processor, "BeginSession", vec![bstr_variant(""), i4_variant(FILE_OPEN_DO_NOT_CARE)])?
        .ok_or("QB_SDK_FAILED: QuickBooks began a session without a ticket")?;
    Ok(sdk)
}

impl Transport for Sdk {
    fn execute(&mut self, request: &str) -> Result<String, String> {
        call(&self.processor, "ProcessRequest", vec![bstr_variant(&self.ticket), bstr_variant(request)])?
            .ok_or_else(|| "QB_SDK_FAILED: QuickBooks answered with something other than text".into())
    }
}

impl Drop for Sdk {
    fn drop(&mut self) {
        if !self.ticket.is_empty() {
            let _ = call(&self.processor, "EndSession", vec![bstr_variant(&self.ticket)]);
        }
        if self.connected {
            let _ = call(&self.processor, "CloseConnection", Vec::new());
        }
    }
}
