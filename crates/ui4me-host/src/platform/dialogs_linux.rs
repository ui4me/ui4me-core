use super::{DialogKind, DialogOptions, DialogResult};
use anyhow::{bail, Context};
use futures_lite::{future, StreamExt};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};
use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use winit::window::Window;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

#[derive(Default)]
pub(super) struct Cancellation(AtomicBool);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    fn requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Wayland handles can be used from the dialog worker; nothing to capture.
pub(super) struct CapturedParent;

pub(super) fn capture_parent(_parent: &Arc<dyn Window>) -> anyhow::Result<CapturedParent> {
    Ok(CapturedParent)
}

pub(super) fn run(
    parent: &Arc<dyn Window>,
    _captured: CapturedParent,
    options: &DialogOptions,
    cancellation: &Arc<Cancellation>,
) -> DialogResult {
    future::block_on(run_async(parent, options, cancellation))
}

async fn run_async(
    parent: &Arc<dyn Window>,
    options: &DialogOptions,
    cancellation: &Cancellation,
) -> DialogResult {
    if cancellation.requested() {
        return Ok(None);
    }
    let window_handle = parent.window_handle()?.as_raw();
    let display_handle = parent.display_handle()?.as_raw();
    if !matches!(window_handle, RawWindowHandle::Wayland(_)) {
        bail!("native Linux dialogs require the qualified Wayland window backend");
    }
    // Keep this identifier alive until the request ends. Its Drop unexports
    // xdg-foreign, and our parent's Arc outlives that Drop.
    let identifier =
        ashpd::WindowIdentifier::from_raw_handle(&window_handle, Some(&display_handle))
            .await
            .context("Wayland compositor does not support xdg-foreign parent export")?;
    if cancellation.requested() {
        return Ok(None);
    }

    // A separate session-bus connection makes each dialog independently
    // cancellable and lets the portal clean it up if this worker disconnects.
    let connection = zbus::connection::Builder::session()?
        .method_timeout(Duration::from_secs(5))
        .build()
        .await?;
    let unique = connection
        .unique_name()
        .context("session bus did not assign a unique name")?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
    let token = format!(
        "ui4me_{}_{}",
        std::process::id(),
        NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
    );
    let namespace = format!("/org/freedesktop/portal/desktop/request/{unique}");
    // Subscribe before invoking FileChooser. Matching the connection's whole
    // namespace also handles portal versions that return a different token.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.portal.Desktop")?
        .interface("org.freedesktop.portal.Request")?
        .member("Response")?
        .path_namespace(namespace)?
        .build();
    let mut responses = zbus::MessageStream::for_match_rule(rule, &connection, Some(8)).await?;
    let chooser = zbus::Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )
    .await?;

    let filters: Vec<(String, Vec<(u32, String)>)> = options
        .filters
        .iter()
        .map(|filter| {
            (
                filter.name.clone(),
                filter
                    .extensions
                    .iter()
                    .map(|extension| {
                        (
                            0,
                            if extension == "*" {
                                "*".to_owned()
                            } else {
                                format!("*.{extension}")
                            },
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    let mut arguments: HashMap<&str, Value<'_>> = HashMap::new();
    arguments.insert("handle_token", Value::from(token.as_str()));
    arguments.insert("modal", Value::from(true));
    if !filters.is_empty() {
        arguments.insert("filters", Value::new(filters));
    }
    if let Some(directory) = &options.directory {
        let mut bytes = directory.as_os_str().as_bytes().to_vec();
        if bytes.contains(&0) {
            bail!("dialog directory contains NUL");
        }
        bytes.push(0);
        arguments.insert("current_folder", Value::new(bytes));
    }
    let method = match options.kind {
        DialogKind::OpenFiles => {
            arguments.insert("multiple", Value::from(options.multiple));
            "OpenFile"
        }
        DialogKind::ChooseFolder => {
            arguments.insert("directory", Value::from(true));
            arguments.insert("multiple", Value::from(false));
            "OpenFile"
        }
        DialogKind::SaveFile => {
            if let Some(name) = &options.file_name {
                arguments.insert("current_name", Value::from(name.as_str()));
            }
            "SaveFile"
        }
    };
    if cancellation.requested() {
        return Ok(None);
    }
    let request_path: OwnedObjectPath = chooser
        .call(
            method,
            &(identifier.to_string(), options.title(), arguments),
        )
        .await
        .context("desktop file chooser portal rejected the request")?;
    let request = zbus::Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        request_path.clone(),
        "org.freedesktop.portal.Request",
    )
    .await?;

    loop {
        if cancellation.requested() {
            // Request.Close does not emit Response. Await its acknowledgement
            // before releasing the exported parent and invoking completion.
            let result: zbus::Result<()> = request.call("Close", &()).await;
            if let Err(error) = result {
                // A racing user response may already have removed the object.
                if !matches!(&error, zbus::Error::MethodError(name, _, _) if name.as_str() == "org.freedesktop.DBus.Error.UnknownObject")
                {
                    return Err(error).context("cancel native file dialog");
                }
            }
            return Ok(None);
        }
        enum Poll {
            Response(Option<zbus::Result<zbus::Message>>),
            Tick,
        }
        let next = future::race(async { Poll::Response(responses.next().await) }, async {
            async_io::Timer::after(Duration::from_millis(50)).await;
            Poll::Tick
        })
        .await;
        let message = match next {
            Poll::Tick => continue,
            Poll::Response(Some(message)) => message?,
            Poll::Response(None) => bail!("desktop portal disconnected before responding"),
        };
        if message.header().path().map(|path| path.as_str()) != Some(request_path.as_str()) {
            continue;
        }
        let (response, mut values): (u32, HashMap<String, OwnedValue>) =
            message.body().deserialize()?;
        match response {
            0 => {
                if cancellation.requested() {
                    return Ok(None);
                }
                let uris: Vec<String> = values
                    .remove("uris")
                    .context("portal response omitted selected files")?
                    .try_into()?;
                let mut paths = Vec::with_capacity(uris.len());
                for uri in uris {
                    let url = url::Url::parse(&uri)?;
                    paths.push(url.to_file_path().map_err(|_| {
                        anyhow::anyhow!("portal returned a URI that is not a local file")
                    })?);
                }
                return Ok(if paths.is_empty() { None } else { Some(paths) });
            }
            1 => return Ok(None),
            other => bail!("desktop portal ended the dialog with response {other}"),
        }
    }
}
