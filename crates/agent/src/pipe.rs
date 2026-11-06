//! Named-pipe server for the overlay app. One client at a time; reconnects freely.
//! Only an explicit `Detach` ends it: transient pipe errors are logged and retried, so
//! the agent outlives app restarts.

use std::time::Duration;

use game_maps_protocol::{decode_line, encode_line, pipe_name, AgentMessage, AppMessage, LogLevel, PROTOCOL_VERSION};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::mpsc;

use crate::agent::{Command, Shared};
use crate::filelog;

const RETRY_DELAY: Duration = Duration::from_millis(500);

/// Serves until the app asks the agent to detach.
pub async fn serve(shared: &'static Shared, mut outbox: mpsc::Receiver<AgentMessage>) {
    let name = pipe_name(std::process::id());
    let mut first = true;
    loop {
        // The first instance is created exclusively so no other process can squat the name.
        let server = match ServerOptions::new().first_pipe_instance(first).create(&name) {
            Ok(s) => s,
            Err(e) => {
                filelog::write(&format!("create pipe failed: {e}"));
                tokio::time::sleep(RETRY_DELAY).await;
                continue;
            }
        };
        first = false;
        if let Err(e) = server.connect().await {
            filelog::write(&format!("pipe connect failed: {e}"));
            tokio::time::sleep(RETRY_DELAY).await;
            continue;
        }

        crate::focus::set_app_pid(client_pid(&server));
        // Discard anything queued while no one was listening.
        while outbox.try_recv().is_ok() {}
        let detach = session(shared, server, &mut outbox).await;
        crate::focus::set_app_pid(0);
        shared.set_connected(false);
        if detach {
            return;
        }
    }
}

/// Process id of the connected app (0 if unknown).
fn client_pid(server: &NamedPipeServer) -> u32 {
    use std::os::windows::io::AsRawHandle;
    let mut pid = 0u32;
    let handle = windows::Win32::Foundation::HANDLE(server.as_raw_handle());
    unsafe {
        let _ = windows::Win32::System::Pipes::GetNamedPipeClientProcessId(handle, &mut pid);
    }
    pid
}

/// Runs one client session. Returns true if the app asked the agent to detach.
async fn session(shared: &'static Shared, server: NamedPipeServer, outbox: &mut mpsc::Receiver<AgentMessage>) -> bool {
    let (read_half, mut write_half) = tokio::io::split(server);
    let mut lines = BufReader::new(read_half).lines();
    if write_half.write_all(&encode_line(&shared.hello())).await.is_err() {
        return false;
    }
    shared.set_connected(true);

    loop {
        tokio::select! {
            msg = outbox.recv() => {
                let Some(msg) = msg else { return true };
                if write_half.write_all(&encode_line(&msg)).await.is_err() {
                    return false;
                }
            }
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(l)) => l,
                    _ => return false,
                };
                match decode_line::<AppMessage>(line.as_bytes()) {
                    Ok(AppMessage::Hello { protocol_version }) if protocol_version != PROTOCOL_VERSION => {
                        shared.log(LogLevel::Error, format!("app protocol {protocol_version} != agent protocol {PROTOCOL_VERSION}"));
                    }
                    Ok(AppMessage::Hello { .. }) => {}
                    Ok(AppMessage::SetHotkeys { hotkeys }) => shared.push_command(Command::SetHotkeys(hotkeys)),
                    Ok(AppMessage::SetStreamRate { max_hz }) => shared.push_command(Command::SetStreamRate(max_hz)),
                    Ok(AppMessage::MapOpened) => shared.push_command(Command::MapOpened),
                    Ok(AppMessage::Teleport { request_id, target }) => shared.push_command(Command::Teleport { request_id, target }),
                    Ok(AppMessage::ExportWorld { request_id }) => shared.push_command(Command::ExportWorld { request_id }),
                    Ok(AppMessage::FindPath { request_id, to }) => shared.push_command(Command::FindPath { request_id, to }),
                    Ok(AppMessage::CheckRoute { request_id, points }) => shared.push_command(Command::CheckRoute { request_id, points }),
                    Ok(AppMessage::ProbeGround { request_id, points }) => shared.push_command(Command::ProbeGround { request_id, points }),
                    Ok(AppMessage::CompanionOrders { request_id, companion }) => shared.push_command(Command::CompanionOrders { request_id, companion }),
                    Ok(AppMessage::CompanionOrder { request_id, companion, order }) => shared.push_command(Command::CompanionOrder { request_id, companion, order }),
                    Ok(AppMessage::ReadWorldMap { request_id, dir, terrain }) => shared.push_command(Command::ReadWorldMap { request_id, dir, terrain }),
                    Ok(AppMessage::SetGamePins { pins }) => shared.push_command(Command::SetGamePins(pins)),
                    Ok(AppMessage::GamePing { position }) => shared.push_command(Command::GamePing(position)),
                    Ok(AppMessage::SetGameOptions { options }) => shared.push_command(Command::SetGameOptions(options)),
                    Ok(AppMessage::Detach) => return true,
                    Err(e) => shared.log(LogLevel::Warn, format!("bad message from app: {e}")),
                }
            }
        }
    }
}
