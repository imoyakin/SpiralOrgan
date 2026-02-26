use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::server::{self, AppState, EventSubscription};

static CORE_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static CORE_STATE: OnceLock<AppState> = OnceLock::new();
static EVENT_STREAMS: OnceLock<Mutex<HashMap<u64, EventSubscription>>> = OnceLock::new();
static NEXT_EVENT_STREAM_ID: AtomicU64 = AtomicU64::new(1);
const LOG_PREVIEW_MAX: usize = 240;

#[derive(Debug, Deserialize)]
struct CoreInvokeRequest {
    method: String,
    path: String,
    body: Option<Value>,
}

#[derive(Debug, Serialize)]
struct CoreInvokeResponse {
    ok: bool,
    data: Option<Value>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CoreEventSubscribeRequest {
    session_id: Option<String>,
    task_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CoreEventNextRequest {
    stream_id: u64,
    timeout_ms: Option<u64>,
}

fn runtime() -> &'static tokio::runtime::Runtime {
    CORE_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build core runtime")
    })
}

fn state() -> &'static AppState {
    CORE_STATE.get_or_init(AppState::default)
}

fn event_streams() -> &'static Mutex<HashMap<u64, EventSubscription>> {
    EVENT_STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn encode_response(response: CoreInvokeResponse) -> *mut c_char {
    let json = serde_json::to_string(&response).unwrap_or_else(|err| {
        format!(
            "{{\"ok\":false,\"error\":\"encode response failed: {}\"}}",
            err
        )
    });
    match CString::new(json) {
        Ok(c) => c.into_raw(),
        Err(_) => CString::new("{\"ok\":false,\"error\":\"invalid response payload\"}")
            .expect("static json should always be valid")
            .into_raw(),
    }
}

fn preview_text(text: &str) -> String {
    let mut preview = text
        .chars()
        .take(LOG_PREVIEW_MAX.saturating_sub(16))
        .collect::<String>()
        .replace('\n', "\\n");
    if text.chars().count() > LOG_PREVIEW_MAX {
        preview.push_str("...(truncated)");
    }
    preview
}

fn preview_value(value: &Value) -> String {
    preview_text(&value.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn spiral_organ_core_start() -> bool {
    eprintln!("ffi: core.start");
    runtime();
    state();
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn spiral_organ_core_event_subscribe(request_json: *const c_char) -> *mut c_char {
    if request_json.is_null() {
        return encode_response(CoreInvokeResponse {
            ok: false,
            data: None,
            error: Some("request_json is null".to_string()),
        });
    }

    let subscribe = || -> Result<CoreInvokeResponse, String> {
        let cstr = unsafe {
            // SAFETY: caller guarantees request_json points to a valid NUL-terminated C string.
            CStr::from_ptr(request_json)
        };
        let input = cstr
            .to_str()
            .map_err(|err| format!("request json is not valid utf-8: {err}"))?;
        let req: CoreEventSubscribeRequest =
            serde_json::from_str(input).map_err(|err| format!("decode request failed: {err}"))?;

        let stream_id = NEXT_EVENT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
        let subscription = state().subscribe_events(req.session_id, req.task_id);
        event_streams()
            .lock()
            .map_err(|_| "event stream mutex poisoned".to_string())?
            .insert(stream_id, subscription);

        Ok(CoreInvokeResponse {
            ok: true,
            data: Some(serde_json::json!({ "stream_id": stream_id })),
            error: None,
        })
    };

    match subscribe() {
        Ok(response) => encode_response(response),
        Err(err) => {
            eprintln!("ffi: kernel.event.subscribe error {}", preview_text(&err));
            encode_response(CoreInvokeResponse {
                ok: false,
                data: None,
                error: Some(err),
            })
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn spiral_organ_core_event_next(request_json: *const c_char) -> *mut c_char {
    if request_json.is_null() {
        return encode_response(CoreInvokeResponse {
            ok: false,
            data: None,
            error: Some("request_json is null".to_string()),
        });
    }

    let next = || -> Result<CoreInvokeResponse, String> {
        let cstr = unsafe {
            // SAFETY: caller guarantees request_json points to a valid NUL-terminated C string.
            CStr::from_ptr(request_json)
        };
        let input = cstr
            .to_str()
            .map_err(|err| format!("request json is not valid utf-8: {err}"))?;
        let req: CoreEventNextRequest =
            serde_json::from_str(input).map_err(|err| format!("decode request failed: {err}"))?;

        let mut subscription = event_streams()
            .lock()
            .map_err(|_| "event stream mutex poisoned".to_string())?
            .remove(&req.stream_id)
            .ok_or_else(|| format!("event stream not found: {}", req.stream_id))?;

        let timeout_ms = req.timeout_ms.unwrap_or(30_000).max(1_000);
        let next_payload = runtime().block_on(subscription.next_event(timeout_ms))?;

        event_streams()
            .lock()
            .map_err(|_| "event stream mutex poisoned".to_string())?
            .insert(req.stream_id, subscription);

        Ok(CoreInvokeResponse {
            ok: true,
            data: Some(match next_payload {
                Some(payload) => payload,
                None => serde_json::json!({ "timeout": true }),
            }),
            error: None,
        })
    };

    match next() {
        Ok(response) => encode_response(response),
        Err(err) => {
            eprintln!("ffi: kernel.event.next error {}", preview_text(&err));
            encode_response(CoreInvokeResponse {
                ok: false,
                data: None,
                error: Some(err),
            })
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn spiral_organ_core_event_unsubscribe(stream_id: u64) -> bool {
    let mut guard = match event_streams().lock() {
        Ok(guard) => guard,
        Err(_) => return false,
    };
    guard.remove(&stream_id).is_some()
}

#[unsafe(no_mangle)]
pub extern "C" fn spiral_organ_core_invoke(request_json: *const c_char) -> *mut c_char {
    if request_json.is_null() {
        return encode_response(CoreInvokeResponse {
            ok: false,
            data: None,
            error: Some("request_json is null".to_string()),
        });
    }

    let invoke = || -> Result<CoreInvokeResponse, String> {
        let cstr = unsafe {
            // SAFETY: caller guarantees request_json points to a valid NUL-terminated C string.
            CStr::from_ptr(request_json)
        };
        let input = cstr
            .to_str()
            .map_err(|err| format!("request json is not valid utf-8: {err}"))?;
        let req: CoreInvokeRequest =
            serde_json::from_str(input).map_err(|err| format!("decode request failed: {err}"))?;
        eprintln!(
            "ffi: kernel.invoke request method={} path={} body={}",
            req.method,
            req.path,
            req.body
                .as_ref()
                .map(preview_value)
                .unwrap_or_else(|| "null".to_string())
        );

        let data = runtime().block_on(server::invoke_http_like(
            Some(state().clone()),
            &req.method,
            &req.path,
            req.body,
        ))?;
        eprintln!("ffi: kernel.invoke response {}", preview_value(&data));

        Ok(CoreInvokeResponse {
            ok: true,
            data: Some(data),
            error: None,
        })
    };

    match invoke() {
        Ok(response) => encode_response(response),
        Err(err) => {
            eprintln!("ffi: kernel.invoke error {}", preview_text(&err));
            encode_response(CoreInvokeResponse {
                ok: false,
                data: None,
                error: Some(err),
            })
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn spiral_organ_core_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        // SAFETY: ptr must be allocated by CString::into_raw from this library.
        let _ = CString::from_raw(ptr);
    }
}
