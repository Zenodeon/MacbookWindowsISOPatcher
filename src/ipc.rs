//! Short-lived, loopback-only worker transport with a random per-build capability.
//! The worker is the same executable, elevated explicitly through Windows UAC.
use crate::{
    domain::*,
    engine::{self, PreparedBuild},
    platform,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const MAX_MESSAGE: u64 = 1024 * 1024;
struct PreparedLease {
    cache: std::path::PathBuf,
    root: std::path::PathBuf,
    clean: bool,
}
impl Drop for PreparedLease {
    fn drop(&mut self) {
        if self.clean {
            let _ = crate::download::remove_extraction(&self.cache, &self.root);
        }
    }
}
fn line(reader: &mut BufReader<TcpStream>) -> Result<String> {
    let mut text = String::new();
    reader.take(MAX_MESSAGE + 1).read_line(&mut text)?;
    ensure!(
        !text.is_empty() && text.len() as u64 <= MAX_MESSAGE && text.ends_with('\n'),
        "Worker connection closed or message was invalid"
    );
    Ok(text)
}
fn send(stream: &mut TcpStream, message: &impl serde::Serialize) -> Result<()> {
    serde_json::to_writer(&mut *stream, message)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

pub fn start(request: BuildRequest, emit: Emit, cancel: Cancellation) -> Result<()> {
    let prepared = engine::prepare(request, &emit, &cancel)?;
    let mut lease = PreparedLease {
        cache: prepared.request.cache.clone(),
        root: prepared.assets_root.clone(),
        clean: true,
    };
    cancel.check()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    stage(
        &emit,
        "Administrator approval",
        "Approve Windows UAC to service the installation images",
    );
    platform::elevate_worker(listener.local_addr()?.port(), &token)?;
    let deadline = Instant::now() + Duration::from_secs(180);
    let (mut stream, mut reader) = loop {
        cancel.check()?;
        ensure!(
            Instant::now() < deadline,
            "The build worker did not connect. Retry and approve the administrator prompt"
        );
        match listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                let mut reader = BufReader::new(stream.try_clone()?);
                if line(&mut reader)
                    .ok()
                    .is_some_and(|text| text.trim() == token)
                {
                    break (stream, reader);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(error) => return Err(error.into()),
        }
    };
    stream.set_read_timeout(None)?;
    // Once handed to the elevated worker, preserve inputs on an uncertain disconnect.
    lease.clean = false;
    send(&mut stream, &prepared)?;
    let mut cancellation_stream = stream.try_clone()?;
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let monitor_done = done.clone();
    let monitor_cancel = cancel.clone();
    let monitor = thread::spawn(move || {
        while !monitor_done.load(std::sync::atomic::Ordering::SeqCst) {
            if monitor_cancel.is_cancelled() {
                let _ = cancellation_stream.write_all(b"cancel\n");
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let result = (|| -> Result<()> {
        loop {
            let event: Event = serde_json::from_str(&line(&mut reader)?)?;
            let terminal = matches!(
                event,
                Event::Finished(_) | Event::Failed(_) | Event::Cancelled
            );
            if terminal {
                if let Err(error) = crate::download::remove_extraction(
                    &prepared.request.cache,
                    &prepared.assets_root,
                ) {
                    emit(Event::Log(format!(
                        "Driver extraction cleanup failed: {error:#}"
                    )));
                }
                emit(event);
                return Ok(());
            }
            emit(event);
        }
    })();
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = monitor.join();
    result.context(
        "Lost connection to the build worker. Check the retained build workspace before retrying",
    )
}

pub fn worker(port: u16, token: &str) -> Result<()> {
    ensure!(
        token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid worker capability"
    );
    ensure!(platform::is_admin(), "Worker requires administrator access");
    let mut stream = TcpStream::connect_timeout(
        &std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_secs(10),
    )?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.write_all(format!("{token}\n").as_bytes())?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let prepared: PreparedBuild = serde_json::from_str(&line(&mut reader)?)?;
    stream.set_read_timeout(None)?;
    let cancel = Cancellation::default();
    let connection_cancel = cancel.clone();
    let output = Arc::new(Mutex::new(stream));
    let writer = output.clone();
    let failed_cancel = cancel.clone();
    let emit: Emit = Arc::new(move |event| {
        if let Ok(mut stream) = writer.lock()
            && send(&mut stream, &event).is_err()
        {
            failed_cancel.cancel();
        }
    });
    thread::spawn(move || {
        // Closing the GUI also requests cancellation at the next safe servicing boundary.
        let mut text = String::new();
        match reader.read_line(&mut text) {
            Ok(_) => connection_cancel.cancel(),
            Err(_) => connection_cancel.cancel(),
        }
    });
    match engine::run(prepared, emit.clone(), cancel.clone()) {
        Ok(path) => emit(Event::Finished(path)),
        Err(_) if cancel.is_cancelled() => emit(Event::Cancelled),
        Err(error) => emit(Event::Failed(format!("{error:#}"))),
    }
    Ok(())
}

pub fn parse_worker_args(args: &[String]) -> Result<Option<(u16, String)>> {
    if args.first().is_none_or(|a| a != "--worker") {
        return Ok(None);
    }
    if args.len() != 3 {
        bail!("Invalid worker arguments");
    }
    let port = args[1].parse()?;
    Ok(Some((port, args[2].clone())))
}
