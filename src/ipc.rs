//! Short-lived, loopback-only worker transport with a random per-build capability.
//! The worker is the same executable, elevated explicitly through Windows UAC.
use crate::{
    domain::*,
    engine::{self, PreparedBuild},
    platform,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const MAX_MESSAGE: u64 = 1024 * 1024;
#[derive(serde::Serialize, serde::Deserialize)]
enum WorkerRequest {
    Build(Box<PreparedBuild>),
    Cleanup(crate::cleanup::CleanupRequest),
}
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

fn message_reader(stream: &TcpStream, timeout: Option<Duration>) -> Result<BufReader<TcpStream>> {
    // Windows accepts inherit the polling listener's non-blocking mode.
    // Framed messages must wait for the complete line, including delayed events.
    configure_socket(stream, timeout)?;
    let reader = BufReader::new(stream.try_clone()?);
    configure_socket(reader.get_ref(), timeout)?;
    Ok(reader)
}

fn configure_socket(stream: &TcpStream, timeout: Option<Duration>) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    Ok(())
}

fn finish_authentication(stream: &TcpStream, reader: &BufReader<TcpStream>) -> Result<()> {
    // On Windows the duplicated reader retains its own receive timeout.
    // Reset both handles directly; changing only the writer leaves a 3s/30s limit.
    configure_socket(stream, None)?;
    configure_socket(reader.get_ref(), None)?;
    ensure!(
        stream.read_timeout()?.is_none() && reader.get_ref().read_timeout()?.is_none(),
        "Worker connection still has an authentication read timeout"
    );
    Ok(())
}

struct ConnectionTrace {
    file: File,
    path: PathBuf,
    started: Instant,
    phase: &'static str,
    emit: Emit,
}
impl ConnectionTrace {
    fn new(workspace: &Path, emit: Emit) -> Result<Self> {
        let path = workspace.join(format!("worker-connection-{}.log", uuid::Uuid::new_v4()));
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .context("Could not create the worker connection log")?;
        let mut trace = Self {
            file,
            path,
            started: Instant::now(),
            phase: "starting",
            emit,
        };
        trace.record(&format!(
            "Connection log: {}",
            platform::display_path(&trace.path)
        ));
        Ok(trace)
    }
    fn retain(&mut self, message: &str) -> String {
        let text = format!(
            "[worker +{:.2}s] {}: {message}",
            self.started.elapsed().as_secs_f64(),
            self.phase
        );
        let _ = writeln!(self.file, "{text}");
        let _ = self.file.flush();
        text
    }
    fn record(&mut self, message: &str) {
        let text = self.retain(message);
        (self.emit)(Event::Log(text));
    }
    fn phase(&mut self, phase: &'static str) {
        self.phase = phase;
        self.record("entered phase");
    }
    fn sockets(&mut self, stream: &TcpStream, reader: &BufReader<TcpStream>) -> Result<()> {
        self.record(&format!(
            "read timeout: writer={:?}, reader={:?}; write timeout: writer={:?}, reader={:?}",
            stream.read_timeout()?,
            reader.get_ref().read_timeout()?,
            stream.write_timeout()?,
            reader.get_ref().write_timeout()?
        ));
        Ok(())
    }
}

fn monitor_cancellation(
    mut stream: TcpStream,
    cancel: Cancellation,
    done: Arc<std::sync::atomic::AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !done.load(std::sync::atomic::Ordering::SeqCst) {
            if cancel.is_cancelled() {
                let _ = stream.write_all(b"cancel\n");
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    })
}

pub fn start(request: BuildRequest, emit: Emit, cancel: Cancellation) -> Result<()> {
    let _cache_activity = crate::cleanup::cache_activity(&request.cache)?;
    let mut prepared = engine::prepare(request, &emit, &cancel)?;
    let mut lease = PreparedLease {
        cache: prepared.request.cache.clone(),
        root: prepared.assets_root.clone(),
        clean: true,
    };
    let mut trace = ConnectionTrace::new(&prepared.request.workspace, emit.clone())?;
    prepared.connection_log = Some(trace.path.clone());
    let result = run_connection(&prepared, &emit, &cancel, &mut lease, &mut trace);
    if let Err(error) = &result {
        trace.record(&format!("Connection failed: {error:#}"));
    }
    result.with_context(|| format!(
        "Worker connection failed during {} after {:.2}s. Connection log: {}. Check the retained build workspace before retrying",
        trace.phase, trace.started.elapsed().as_secs_f64(), platform::display_path(&trace.path)
    ))
}

fn run_connection(
    prepared: &PreparedBuild,
    emit: &Emit,
    cancel: &Cancellation,
    lease: &mut PreparedLease,
    trace: &mut ConnectionTrace,
) -> Result<()> {
    cancel.check()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    stage(
        emit,
        "Administrator approval",
        "Approve Windows UAC to service the installation images",
    );
    trace.phase("administrator approval");
    platform::elevate_worker(listener.local_addr()?.port(), &token)?;
    trace.phase("waiting for worker connection");
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
                let mut reader = message_reader(&stream, Some(Duration::from_secs(3)))?;
                trace.phase("authenticating worker");
                trace.sockets(&stream, &reader)?;
                if line(&mut reader)
                    .ok()
                    .is_some_and(|text| text.trim() == token)
                {
                    break (stream, reader);
                }
                trace.record("Rejected connection: authentication failed or timed out");
                trace.phase("waiting for worker connection");
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(error) => return Err(error.into()),
        }
    };
    finish_authentication(&stream, &reader)?;
    trace.phase("sending build request");
    trace.sockets(&stream, &reader)?;
    // Once handed to the elevated worker, preserve inputs on an uncertain disconnect.
    lease.clean = false;
    send(
        &mut stream,
        &WorkerRequest::Build(Box::new(prepared.clone())),
    )?;
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let monitor = monitor_cancellation(stream.try_clone()?, cancel.clone(), done.clone());
    trace.phase("receiving worker events");
    let result = (|| -> Result<()> {
        loop {
            let event: Event = serde_json::from_str(&line(&mut reader)?)?;
            // Retain the worker's events alongside transport diagnostics, even if
            // the worker fails before creating its separate build.log.
            trace.retain(&serde_json::to_string(&event)?);
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
    let mut reader = message_reader(&stream, Some(Duration::from_secs(30)))?;
    stream.write_all(format!("{token}\n").as_bytes())?;
    let request: WorkerRequest = serde_json::from_str(&line(&mut reader)?)?;
    finish_authentication(&stream, &reader)?;
    let prepared = match request {
        WorkerRequest::Build(prepared) => *prepared,
        WorkerRequest::Cleanup(request) => {
            let result = crate::cleanup::execute(request).map_err(|e| format!("{e:#}"));
            send(&mut stream, &result)?;
            return Ok(());
        }
    };
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

/// Run one bounded cleanup operation through the existing authenticated UAC worker.
pub fn cleanup_elevated(
    request: crate::cleanup::CleanupRequest,
) -> Result<crate::cleanup::CleanupReport> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    platform::elevate_worker(listener.local_addr()?.port(), &token)?;
    let deadline = Instant::now() + Duration::from_secs(180);
    let (mut stream, mut reader) = loop {
        ensure!(
            Instant::now() < deadline,
            "Cleanup worker did not connect. Retry and approve the Windows administrator prompt"
        );
        match listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let mut reader = message_reader(&stream, Some(Duration::from_secs(3)))?;
                if line(&mut reader).ok().is_some_and(|v| v.trim() == token) {
                    break (stream, reader);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(e.into()),
        }
    };
    finish_authentication(&stream, &reader)?;
    send(&mut stream, &WorkerRequest::Cleanup(request))?;
    let result: std::result::Result<crate::cleanup::CleanupReport, String> =
        serde_json::from_str(&line(&mut reader)?)?;
    result.map_err(anyhow::Error::msg)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::ErrorKind, net::Shutdown};

    #[test]
    fn authenticated_worker_survives_auth_timeout_and_silent_cancellation() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let token = "a".repeat(64);
        let expected_token = token.clone();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
            let mut reader = message_reader(&stream, Some(Duration::from_secs(10))).unwrap();
            stream.write_all(format!("{token}\n").as_bytes()).unwrap();
            assert_eq!(line(&mut reader).unwrap().trim(), "\"prepared\"");
            finish_authentication(&stream, &reader).unwrap();
            thread::sleep(Duration::from_millis(3500));
            let event = Event::Stage {
                name: "Checking Windows media".into(),
                detail: "Delayed worker event".into(),
            };
            let serialized = serde_json::to_string(&event).unwrap() + "\n";
            stream.write_all(&serialized.as_bytes()[..1]).unwrap();
            thread::sleep(Duration::from_millis(75));
            stream.write_all(&serialized.as_bytes()[1..]).unwrap();
            assert_eq!(line(&mut reader).unwrap().trim(), "cancel");
            send(&mut stream, &Event::Cancelled).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(pair) => break pair,
                Err(error)
                    if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut reader = message_reader(&stream, Some(Duration::from_secs(3))).unwrap();
        assert_eq!(line(&mut reader).unwrap().trim(), expected_token);
        assert_eq!(
            reader.get_ref().read_timeout().unwrap(),
            Some(Duration::from_secs(3))
        );
        finish_authentication(&stream, &reader).unwrap();
        assert!(reader.get_ref().read_timeout().unwrap().is_none());
        assert!(stream.read_timeout().unwrap().is_none());
        send(&mut stream, &"prepared").unwrap();
        let cancel = Cancellation::default();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let monitor =
            monitor_cancellation(stream.try_clone().unwrap(), cancel.clone(), done.clone());
        let event: Event = serde_json::from_str(&line(&mut reader).unwrap()).unwrap();
        assert!(matches!(event, Event::Stage { name, .. } if name == "Checking Windows media"));
        cancel.cancel();
        assert!(matches!(
            serde_json::from_str::<Event>(&line(&mut reader).unwrap()).unwrap(),
            Event::Cancelled
        ));
        client.join().unwrap();
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        monitor.join().unwrap();
    }

    #[test]
    fn authentication_reads_still_time_out() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let mut reader = message_reader(&stream, Some(Duration::from_millis(40))).unwrap();
        let delayed = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            let _ = peer.write_all(b"late\n");
        });
        let error = line(&mut reader).unwrap_err();
        let error = error.downcast_ref::<std::io::Error>().unwrap();
        assert!(matches!(
            error.kind(),
            ErrorKind::WouldBlock | ErrorKind::TimedOut
        ));
        delayed.join().unwrap();
    }

    #[test]
    fn actual_worker_disconnect_is_still_an_error() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let mut reader = message_reader(&stream, Some(Duration::from_secs(2))).unwrap();
        finish_authentication(&stream, &reader).unwrap();
        peer.shutdown(Shutdown::Both).unwrap();
        assert!(
            line(&mut reader)
                .unwrap_err()
                .to_string()
                .contains("Worker connection closed")
        );
    }

    #[test]
    fn connection_diagnostics_retain_phase_elapsed_and_events() {
        let mut trace = ConnectionTrace::new(&std::env::temp_dir(), Arc::new(|_| {})).unwrap();
        trace.phase("receiving worker events");
        trace.retain("test worker event");
        trace.record("test connection error");
        let path = trace.path.clone();
        drop(trace);
        let log = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(log.contains("[worker +"));
        assert!(log.contains("receiving worker events: test worker event"));
        assert!(log.contains("receiving worker events: test connection error"));
    }
}
