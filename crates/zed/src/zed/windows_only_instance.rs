use std::{
    sync::{Arc, mpsc},
    thread::JoinHandle,
    time::Duration,
};

use anyhow::Context;
use cli::{CliRequest, CliResponse, IpcHandshake, ipc::IpcOneShotServer};
use parking_lot::Mutex;
use release_channel::app_identifier;
use util::ResultExt;
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, ERROR_PIPE_BUSY, GENERIC_WRITE, GetLastError, HANDLE,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE, OPEN_EXISTING,
            PIPE_ACCESS_INBOUND, ReadFile, WriteFile,
        },
        System::{
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
                GetNamedPipeServerProcessId, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_WAIT,
                WaitNamedPipeW,
            },
            Threading::{
                CreateMutexW, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
        },
    },
    core::{HSTRING, PWSTR},
};

use crate::{Args, OpenListener, RawOpenRequest};

/// How long a fresh launch waits for the running copy to take over its
/// request. A live copy answers in milliseconds; one that is hung, or stuck
/// on the way out, never answers, and without a limit the new launch would
/// wait forever with nothing on screen.
const HANDOFF_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to wait for the running copy's pipe when it is busy with another
/// launch before treating it as unresponsive.
const PIPE_BUSY_TIMEOUT_MS: u32 = 2_000;

#[inline]
fn is_first_instance() -> bool {
    let created = unsafe {
        CreateMutexW(
            None,
            false,
            &HSTRING::from(format!("{}-Instance-Mutex", app_identifier())),
        )
    };
    match created {
        Ok(_) => unsafe { GetLastError() != ERROR_ALREADY_EXISTS },
        // The mutex can be refused (for example, one made by another user
        // session with stricter rights). Opening a window is better than
        // failing silently at the double-click.
        Err(error) => {
            log::error!("Unable to create the instance mutex, opening anyway: {error}");
            true
        }
    }
}

pub fn handle_single_instance(opener: OpenListener, args: &Args) -> bool {
    if is_first_instance() {
        listen_for_other_instances(opener);
        return true;
    }
    if args.foreground {
        return false;
    }
    // We are not the first instance, send args to the first instance.
    match send_args_to_instance_with_timeout(args) {
        Ok(Some(status)) => std::process::exit(status),
        Ok(None) => false,
        // The running copy is hung or a leftover that never finished quitting.
        // Its window is not coming, so open one here rather than leave the
        // person clicking. The listener below keeps retrying the pipe, so once
        // the old copy is gone, later launches reach this one.
        Err(error) => {
            log::warn!(
                "the running copy of noah did not respond ({error:#}); opening a new window"
            );
            listen_for_other_instances(opener);
            true
        }
    }
}

fn listen_for_other_instances(opener: OpenListener) {
    std::thread::Builder::new()
        .name("EnsureSingleton".to_owned())
        .spawn(move || {
            with_pipe(&|url| {
                opener.open(RawOpenRequest {
                    urls: vec![url],
                    ..Default::default()
                })
            })
        })
        .unwrap();
}

/// Runs the handoff on its own thread so the launch can give up on it. A
/// thread left waiting on a dead copy costs nothing and ends with the process.
fn send_args_to_instance_with_timeout(args: &Args) -> anyhow::Result<Option<i32>> {
    let (sender, receiver) = mpsc::channel();
    let args = args.clone();
    std::thread::Builder::new()
        .name("InstanceHandoff".to_owned())
        .spawn(move || {
            sender.send(send_args_to_instance(&args)).ok();
        })
        .unwrap();
    match receiver.recv_timeout(HANDOFF_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            anyhow::bail!("no reply within {} seconds", HANDOFF_TIMEOUT.as_secs())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => anyhow::bail!("the handoff thread ended"),
    }
}

fn with_pipe(f: &dyn Fn(String)) {
    let pipe = loop {
        let pipe = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(format!("\\\\.\\pipe\\{}-Named-Pipe", app_identifier())),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                1,
                128,
                128,
                0,
                None,
            )
        };
        if !pipe.is_invalid() {
            break pipe;
        }
        // A hung or half-quit copy may still own the pipe. Keep trying so
        // this window takes over once that copy is finally gone.
        log::error!("Failed to create named pipe, retrying: {:?}", unsafe {
            GetLastError()
        });
        std::thread::sleep(Duration::from_secs(5));
    };

    loop {
        if let Some(message) = retrieve_message_from_pipe(pipe)
            .context("Failed to read from named pipe")
            .log_err()
        {
            f(message);
        }
    }
}

fn retrieve_message_from_pipe(pipe: HANDLE) -> anyhow::Result<String> {
    unsafe { ConnectNamedPipe(pipe, None)? };
    let message = retrieve_message_from_pipe_inner(pipe);
    unsafe { DisconnectNamedPipe(pipe).log_err() };
    message
}

fn retrieve_message_from_pipe_inner(pipe: HANDLE) -> anyhow::Result<String> {
    let mut buffer = [0u8; 128];
    unsafe {
        ReadFile(pipe, Some(&mut buffer), None, None)?;
    }
    let message = std::ffi::CStr::from_bytes_until_nul(&buffer)?;
    Ok(message.to_string_lossy().into_owned())
}

// This part of code is mostly from crates/cli/src/main.rs
fn send_args_to_instance(args: &Args) -> anyhow::Result<Option<i32>> {
    if let Some(dock_menu_action_idx) = args.dock_action {
        let url = format!("zed-dock-action://{}", dock_menu_action_idx);
        write_message_to_instance_pipe(url.as_bytes())?;
        return Ok(None);
    }

    let (server, server_name) =
        IpcOneShotServer::<IpcHandshake>::new().context("Handshake before Zed spawn")?;
    let url = format!("zed-cli://{server_name}");

    let request = {
        let mut paths = vec![];
        let mut urls = vec![];
        let mut diff_paths = vec![];
        for path in args.paths_or_urls.iter() {
            match std::fs::canonicalize(&path) {
                Ok(path) => paths.push(path.to_string_lossy().into_owned()),
                Err(error) => {
                    if path.starts_with("zed://")
                        || path.starts_with("http://")
                        || path.starts_with("https://")
                        || path.starts_with("file://")
                        || path.starts_with("ssh://")
                    {
                        urls.push(path.clone());
                    } else {
                        log::error!("error parsing path argument: {}", error);
                    }
                }
            }
        }

        for path in args.diff.chunks(2) {
            let old = std::fs::canonicalize(&path[0]).log_err();
            let new = std::fs::canonicalize(&path[1]).log_err();
            if let Some((old, new)) = old.zip(new) {
                diff_paths.push([
                    old.to_string_lossy().into_owned(),
                    new.to_string_lossy().into_owned(),
                ]);
            }
        }

        CliRequest::Open {
            paths,
            urls,
            diff_paths,
            diff_all: false,
            wait: false,
            wsl: args.wsl.clone(),
            open_behavior: Default::default(),
            env: None,
            user_data_dir: args.user_data_dir.clone(),
            dev_container: args.dev_container,
            cwd: std::env::current_dir().ok(),
        }
    };

    let exit_status = Arc::new(Mutex::new(None));
    let sender: JoinHandle<anyhow::Result<()>> = std::thread::Builder::new()
        .name("CliReceiver".to_owned())
        .spawn({
            let exit_status = exit_status.clone();
            move || {
                let (_, handshake) = server.accept().context("Handshake after Zed spawn")?;
                let (tx, rx) = (handshake.requests, handshake.responses);

                tx.send(request)?;

                while let Ok(response) = rx.recv() {
                    match response {
                        CliResponse::Ping => {}
                        CliResponse::Stdout { message } => log::info!("{message}"),
                        CliResponse::Stderr { message } => log::error!("{message}"),
                        CliResponse::Exit { status } => {
                            exit_status.lock().replace(status);
                            return Ok(());
                        }
                        CliResponse::PromptOpenBehavior => {
                            tx.send(CliRequest::SetOpenBehavior {
                                behavior: cli::CliBehaviorSetting::ExistingWindow,
                            })?;
                        }
                    }
                }
                Ok(())
            }
        })
        .unwrap();

    write_message_to_instance_pipe(url.as_bytes())?;
    sender
        .join()
        .map_err(|_| anyhow::anyhow!("the CLI receiver thread panicked"))??;
    let exit_status = exit_status.lock().take();
    Ok(exit_status)
}

fn write_message_to_instance_pipe(message: &[u8]) -> anyhow::Result<()> {
    let name = HSTRING::from(format!("\\\\.\\pipe\\{}-Named-Pipe", app_identifier()));
    unsafe {
        let pipe = match open_instance_pipe(&name) {
            // The pipe takes one client at a time; another launch may hold it
            // for a moment. Wait for it, but not on a copy that never lets go.
            Err(error) if error.code() == ERROR_PIPE_BUSY.to_hresult() => {
                WaitNamedPipeW(&name, PIPE_BUSY_TIMEOUT_MS)
                    .ok()
                    .context("the running copy's pipe stayed busy")?;
                open_instance_pipe(&name)
            }
            other => other,
        }
        .context("could not open the running copy's pipe")?;
        // Any program can create a pipe by this name. Hand the launch over
        // only to a copy of this same noah; anything else would swallow the
        // request and leave the person with no window.
        if let Err(error) = ensure_pipe_is_served_by_noah(pipe) {
            CloseHandle(pipe).log_err();
            return Err(error);
        }
        WriteFile(pipe, Some(message), None, None)?;
        CloseHandle(pipe)?;
    }
    Ok(())
}

unsafe fn ensure_pipe_is_served_by_noah(pipe: HANDLE) -> anyhow::Result<()> {
    let server_exe = unsafe {
        let mut server_pid = 0u32;
        GetNamedPipeServerProcessId(pipe, &mut server_pid)
            .context("could not identify the pipe's owner")?;
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, server_pid)
            .context("could not open the pipe owner's process")?;
        let mut buffer = [0u16; 32 * 1024];
        let mut length = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        CloseHandle(process).log_err();
        queried.context("could not read the pipe owner's path")?;
        std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]))
    };
    let own_exe = std::env::current_exe().context("could not find this noah's path")?;
    let same_program = server_exe
        .file_name()
        .zip(own_exe.file_name())
        .is_some_and(|(server, own)| server.eq_ignore_ascii_case(own));
    anyhow::ensure!(
        same_program,
        "the instance pipe is served by {}, not by noah",
        server_exe.display()
    );
    Ok(())
}

unsafe fn open_instance_pipe(name: &HSTRING) -> windows::core::Result<HANDLE> {
    unsafe {
        CreateFileW(
            name,
            GENERIC_WRITE.0,
            FILE_SHARE_MODE::default(),
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES::default(),
            None,
        )
    }
}
