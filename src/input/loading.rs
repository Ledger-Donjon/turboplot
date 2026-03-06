//! Shared trace-loading logic used by both native `FileManager` and web `WebFileManager`.

use super::Args;
use crate::filtering::Filtering;
use crate::loaders::{TraceFormat, guess_format, load_csv, load_npy, load_tek_wfm};
use biquad::ToHertz;
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use std::sync::Mutex;

/// Shared slot for receiving fetched bytes from an async task.
#[cfg(target_arch = "wasm32")]
pub type FetchSlot = Arc<Mutex<Option<Result<(Vec<u8>, String), String>>>>;

/// Create a new empty fetch slot.
#[cfg(target_arch = "wasm32")]
pub fn new_fetch_slot() -> FetchSlot {
    Arc::new(Mutex::new(None))
}

/// Result of a file manager update cycle.
pub enum LoadResult {
    /// No files loaded yet, continue showing the selection screen.
    Pending,
    /// Traces were loaded successfully.
    Loaded {
        labels: Vec<String>,
        traces: Vec<Arc<Vec<f32>>>,
        args: Args,
    },
}

/// Parse one file into one or more traces from a reader.
pub fn parse_traces<R: std::io::BufRead>(
    reader: R,
    format: TraceFormat,
    name: &str,
    args: &Args,
) -> Vec<Vec<f32>> {
    match format {
        TraceFormat::TekWfm => load_tek_wfm(reader, name),
        TraceFormat::Numpy => load_npy(reader, name),
        TraceFormat::Csv => vec![load_csv(reader, args.skip_lines, args.column)],
    }
}

/// Apply frame selection and filtering, then push results into the output vectors.
pub fn collect_frames(
    name: &str,
    mut frames: Vec<Vec<f32>>,
    args: &Args,
    labels: &mut Vec<String>,
    traces: &mut Vec<Arc<Vec<f32>>>,
) {
    let n = frames.len();
    let selection = args.frame_selection();
    for (i, mut frame) in frames.drain(..).enumerate() {
        if let Some(ref sel) = selection {
            if !sel.contains(&i) {
                continue;
            }
        }

        if let Some(filter) = args.filter {
            frame.apply_filter(
                filter,
                args.sampling_rate.mhz(),
                args.cutoff_freq.khz(),
            );
        }

        if n > 1 {
            labels.push(format!("{} [frame {}]", name, i));
        } else {
            labels.push(name.to_string());
        }
        traces.push(Arc::new(frame));
    }
}

/// Load traces from raw bytes with a given filename (used by web fetch and drag-and-drop).
pub fn load_from_bytes(
    bytes: &[u8],
    name: &str,
    args: &Args,
) -> Result<(Vec<String>, Vec<Arc<Vec<f32>>>), String> {
    let format = args
        .format
        .or_else(|| guess_format(name))
        .ok_or_else(|| format!("Unrecognized file extension: {name}"))?;

    let cursor = std::io::Cursor::new(bytes);
    let frames = parse_traces(cursor, format, name, args);

    let mut labels = Vec::new();
    let mut traces = Vec::new();
    collect_frames(name, frames, args, &mut labels, &mut traces);

    if traces.is_empty() {
        return Err("No traces found in file".into());
    }
    Ok((labels, traces))
}

/// Derive a filename from a URL path segment.
#[cfg(target_arch = "wasm32")]
pub fn filename_from_url(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or("trace")
        .split('?')
        .next()
        .unwrap_or("trace")
        .to_string()
}

/// Spawn an async fetch for the given URL, writing the result into the slot.
#[cfg(target_arch = "wasm32")]
pub fn spawn_fetch(url: String, slot: FetchSlot) {
    let filename = filename_from_url(&url);
    wasm_bindgen_futures::spawn_local(async move {
        let result = fetch_url(&url).await;
        *slot.lock().unwrap() = Some(result.map(|bytes| (bytes, filename)));
    });
}

/// Fetch a URL over HTTP and return the response body as bytes.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
    use js_sys::{ArrayBuffer, Uint8Array};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::Response;

    let window = web_sys::window().ok_or("No window")?;
    let resp_value = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| {
            let detail = js_sys::Error::from(e).message();
            let detail = detail.as_string().unwrap_or_default();
            format!(
                "Fetch failed: {detail}. This is likely a CORS issue \u{2014} \
                 the remote server must include Access-Control-Allow-Origin \
                 headers, or the trace must be served from the same origin."
            )
        })?;

    let resp: Response = resp_value.dyn_into().map_err(|_| "Response cast failed")?;

    if !resp.ok() {
        return Err(format!("HTTP {} {}", resp.status(), resp.status_text()));
    }

    let buf_promise = resp
        .array_buffer()
        .map_err(|e| format!("array_buffer() failed: {e:?}"))?;
    let buf_value = JsFuture::from(buf_promise)
        .await
        .map_err(|e| format!("Reading body failed: {e:?}"))?;

    let buffer: ArrayBuffer = buf_value.dyn_into().map_err(|_| "ArrayBuffer cast failed")?;
    let array = Uint8Array::new(&buffer);
    Ok(array.to_vec())
}
