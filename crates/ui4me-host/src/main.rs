use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use ui4me_host::{config::AppConfig, host, platform};

#[derive(Parser)]
#[command(name = "ui4me-host", version, about = "UI4ME native application host")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Launch the backend described by a TOML application manifest.
    Run {
        #[arg(long)]
        app: PathBuf,
        /// Exercise protocol/lifecycle without claiming desktop integration.
        #[arg(long)]
        headless: bool,
        /// Explicit Linux diagnostic mode without systemd user containment.
        #[arg(long)]
        allow_unsupervised: bool,
    },
    /// Native input, clipboard, dialogs and accessibility probe without Python.
    Probe {
        #[arg(long)]
        allow_unsupervised: bool,
    },
    /// Render an HTML screen to a diagnostic PNG without a desktop or backend.
    Render {
        #[arg(long)]
        html: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Viewport width in CSS pixels.
        #[arg(long, default_value_t = 900)]
        width: u32,
        /// Viewport height in CSS pixels.
        #[arg(long, default_value_t = 600)]
        height: u32,
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
    },
    /// Print environment facts, not a claim of native qualification.
    Doctor,
}

fn unsupervised(command: &Command) -> bool {
    matches!(
        command,
        Command::Run {
            allow_unsupervised: true,
            ..
        } | Command::Probe {
            allow_unsupervised: true
        }
    )
}

/// The command line, parsed on the process main thread, plus the process-wide environment choices
/// that must be made before any other thread exists.
fn prepare() -> Command {
    let cli = Cli::parse();
    // Set only before starting threads (`std::env::set_var` is not thread safe on Unix). These are
    // explicit launcher choices, never inferred silently from a missing desktop/service manager.
    let headless = matches!(
        cli.command,
        Command::Run { headless: true, .. } | Command::Render { .. }
    );
    let unsupervised = unsupervised(&cli.command);
    if headless {
        std::env::set_var("UI4ME_TEST_MODE", "1");
    } else if !matches!(cli.command, Command::Doctor) {
        platform::detach_owned_console();
    }
    if unsupervised {
        std::env::set_var("UI4ME_ALLOW_UNSUPERVISED", "1");
    }
    cli.command
}

/// Everything after argument parsing, on the engine thread.
fn execute(command: Command) -> Result<i32> {
    let _logging = ui4me_host::logging::init()?;
    if matches!(command, Command::Doctor) {
        println!("{}", serde_json::to_string_pretty(&host::doctor())?);
        return Ok(0);
    }
    if let Command::Render {
        ref html,
        ref output,
        width,
        height,
        scale,
    } = command
    {
        println!(
            "{}",
            serde_json::to_string_pretty(&ui4me_host::snapshot::render(
                html, output, width, height, scale
            )?)?
        );
        return Ok(0);
    }
    if unsupervised(&command) {
        tracing::warn!("explicit diagnostic containment selected; this does not qualify hard-host-death cleanup on Linux");
    }
    if let Some(code) =
        platform::bootstrap_supervised(&std::env::args_os().skip(1).collect::<Vec<_>>())?
    {
        return Ok(code);
    }
    match command {
        Command::Run { app, headless, .. } => host::run(AppConfig::load(&app)?, headless),
        Command::Probe { .. } => host::run_probe(),
        Command::Doctor | Command::Render { .. } => unreachable!(),
    }
}

/// F-01: the engine (event loop, documents, layout, paint, accessibility) runs on one thread with
/// an explicit stack of [`ui4me_renderer::ENGINE_STACK_BYTES`] on every OS, instead of the process
/// main thread, whose stack is whatever the PE header (1 MiB by default on Windows) or
/// `ulimit -s` (8 MiB typically on Linux) happens to give. The renderer recurses over DOM depth;
/// `crates/ui4me-renderer/tests/stack_depth.rs` proves the documented maximum depth on half of
/// this stack. `build.rs` raises the Windows PE reserve to the same size as a backstop for anything
/// that still runs on the main thread. On 64-bit targets the size is reserved address space; pages
/// are committed only as the stack actually grows.
///
/// What was checked for running off the process main thread:
/// * winit: the event loop is built with `with_any_thread(true)` (Win32 and Wayland,
///   `host::event_loop`); windows are created and driven on this same thread, and they are
///   destroyed before it ends (the `Host` is dropped inside `run_app`).
/// * COM/OLE: winit-win32 initialises OLE (clipboard, drag and drop) per thread on the thread
///   that owns the windows, i.e. this one. File dialogs and shell operations already run on their
///   own worker threads with their own STA (`platform/dialogs_windows.rs`, `platform/external.rs`).
///   Nothing initialises COM on the process main thread.
/// * AccessKit: the Windows `SubclassingAdapter` is created for, and serviced on, the thread that
///   owns the HWND (this one); the Unix adapter runs its own async-io thread.
/// * Console and environment: `detach_owned_console` and every `set_var` stay on the main thread,
///   before any other thread is started. Signal and console-control handlers (HOST-07) are
///   process-wide; `host::run` installs them from this thread, after `bootstrap_supervised`, so
///   the supervising launcher keeps the default disposition. They only record the request and
///   wake this thread (`termination.rs`); the shutdown itself runs here.
/// * Linux supervision: `bootstrap_supervised` waits on `systemd-run` from this thread; the
///   backend child's `PR_SET_PDEATHSIG` follows the thread that forked it, which is this thread,
///   and that thread lives exactly as long as the host (the process exits when it ends).
/// * Windows job objects, the single-instance lock, headless mode and `render`/`doctor` are
///   thread-agnostic and run here too, so every command has the same stack.
fn main() {
    let command = prepare();
    let engine = std::thread::Builder::new()
        .name("ui4me-engine".into())
        .stack_size(ui4me_renderer::ENGINE_STACK_BYTES)
        .spawn(move || execute(command));
    let code = match engine.map(|thread| thread.join()) {
        Ok(Ok(Ok(code))) => code,
        Ok(Ok(Err(error))) => {
            eprintln!("ui4me-host: {error:#}");
            1
        }
        // The panic message was already printed by the panic hook on the engine thread; keep the
        // process outcome identical to a panic on the main thread.
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(error) => {
            eprintln!("ui4me-host: cannot start the engine thread: {error}");
            1
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_stack_reserve_matches_the_engine_stack() {
        let build = include_str!("../build.rs");
        let expected = format!(
            "const WINDOWS_STACK_RESERVE: u64 = {};",
            ui4me_renderer::ENGINE_STACK_BYTES
        );
        assert!(
            build.contains(&expected),
            "build.rs must contain `{expected}`"
        );
        assert_eq!(ui4me_renderer::ENGINE_STACK_BYTES, 33_554_432);
    }
}
