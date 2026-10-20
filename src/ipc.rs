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

fn message_reader(stream: &TcpStream, timeout: Option<Duration>) -> Result<BufReader<TcpStream>> {
    // Windows accepts inherit the polling listener's non-blocking mode.
    // Framed messages must wait for the complete line, including delayed events.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    Ok(BufReader::new(stream.try_clone()?))
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
                let mut reader = message_reader(&stream, Some(Duration::from_secs(3)))?;
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
    let mut reader = message_reader(&stream, Some(Duration::from_secs(30)))?;
    stream.write_all(format!("{token}\n").as_bytes())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::ErrorKind, net::Shutdown};

    #[test]
    fn accepted_worker_waits_for_fragmented_events_and_receives_cancellation() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let token = "a".repeat(64);
        let expected_token = token.clone();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
            let mut reader = message_reader(&stream, Some(Duration::from_secs(2))).unwrap();
            stream.write_all(format!("{token}\n").as_bytes()).unwrap();
            assert_eq!(line(&mut reader).unwrap().trim(), "\"prepared\"");
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
        let mut reader = message_reader(&stream, Some(Duration::from_secs(2))).unwrap();
        assert_eq!(line(&mut reader).unwrap().trim(), expected_token);
        stream.set_read_timeout(None).unwrap();
        send(&mut stream, &"prepared").unwrap();
        let event: Event = serde_json::from_str(&line(&mut reader).unwrap()).unwrap();
        assert!(matches!(event, Event::Stage { name, .. } if name == "Checking Windows media"));
        stream.try_clone().unwrap().write_all(b"cancel\n").unwrap();
        assert!(matches!(
            serde_json::from_str::<Event>(&line(&mut reader).unwrap()).unwrap(),
            Event::Cancelled
        ));
        client.join().unwrap();
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
        peer.shutdown(Shutdown::Both).unwrap();
        assert!(
            line(&mut reader)
                .unwrap_err()
                .to_string()
                .contains("Worker connection closed")
        );
    }
}
