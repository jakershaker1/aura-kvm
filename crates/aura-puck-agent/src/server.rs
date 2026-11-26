use crate::servo::SpatialServo;
use crate::spatial::query_wmshell_spatial;
use crate::uhid::HidSink;
use aura_protocol::{
    read_frame, write_frame, Message, SpatialPose, WireError, CAP_CONSUMER, CAP_FOCUS_WATCH,
    CAP_KEYBOARD, CAP_MOUSE, CAP_SPATIAL_POSE, CAP_SPATIAL_WARP, PROTOCOL_VERSION,
};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub fn serve_connection<S: HidSink + ?Sized>(
    stream: TcpStream,
    sink: &mut S,
) -> Result<(), String> {
    serve_connection_with_spatial(stream, sink, None, None, None, None, false)
}

pub fn serve_connection_with_focus<S: HidSink + ?Sized>(
    stream: TcpStream,
    sink: &mut S,
    focus_rx: Option<&mpsc::Receiver<()>>,
) -> Result<(), String> {
    serve_connection_with_spatial(stream, sink, focus_rx, None, None, None, false)
}

pub fn serve_connection_full<S: HidSink + ?Sized>(
    stream: TcpStream,
    sink: &mut S,
    focus_rx: Option<&mpsc::Receiver<()>>,
    spatial_trigger_tx: Option<&mpsc::Sender<()>>,
    spatial_rx: Option<&mpsc::Receiver<SpatialPose>>,
) -> Result<(), String> {
    serve_connection_with_spatial(
        stream,
        sink,
        focus_rx,
        spatial_trigger_tx,
        spatial_rx,
        None,
        false,
    )
}

/// Serve one connected `aura-host` client until disconnect or protocol error.
/// Guarantees `sink.release_all()` is invoked before returning.
pub fn serve_connection_with_spatial<S: HidSink + ?Sized>(
    mut stream: TcpStream,
    sink: &mut S,
    focus_rx: Option<&mpsc::Receiver<()>>,
    spatial_trigger_tx: Option<&mpsc::Sender<()>>,
    spatial_rx: Option<&mpsc::Receiver<SpatialPose>>,
    mirror_package: Option<&str>,
    verbose: bool,
) -> Result<(), String> {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));

    let hello = match read_frame(&mut stream) {
        Ok(msg) => msg,
        Err(e) => {
            let _ = sink.release_all();
            return Err(format!("handshake read failed: {e}"));
        }
    };

    match hello {
        Message::Hello { version } => {
            if version != PROTOCOL_VERSION {
                let _ = sink.release_all();
                return Err(format!(
                    "protocol version mismatch: host={version}, agent={PROTOCOL_VERSION}"
                ));
            }
            let mut caps = CAP_KEYBOARD | CAP_MOUSE | CAP_CONSUMER;
            if focus_rx.is_some() {
                caps |= CAP_FOCUS_WATCH;
            }
            if spatial_rx.is_some() {
                caps |= CAP_SPATIAL_POSE;
            }
            if mirror_package.is_some() && spatial_rx.is_some() {
                caps |= CAP_SPATIAL_WARP;
            }
            let ack = Message::HelloAck {
                version: PROTOCOL_VERSION,
                capabilities: caps,
            };
            if let Err(e) = write_frame(&mut stream, &ack) {
                let _ = sink.release_all();
                return Err(format!("handshake ack write failed: {e}"));
            }
        }
        other => {
            let _ = sink.release_all();
            return Err(format!("expected Hello handshake, got {other:?}"));
        }
    }

    if focus_rx.is_some() || spatial_rx.is_some() {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
    }
    let mut last_rx = Instant::now();
    let mut servo = SpatialServo::new();

    let result = loop {
        sink.poll_drain();

        if let Some(rx) = focus_rx {
            let mut focused = false;
            while rx.try_recv().is_ok() {
                focused = true;
            }
            if focused {
                if let Some(pkg) = mirror_package {
                    if let Ok(fresh_pose) = query_wmshell_spatial(pkg) {
                        if fresh_pose.is_usable() && fresh_pose.has_cursor() {
                            if let Err(e) =
                                write_frame(&mut stream, &Message::SpatialPose(fresh_pose))
                            {
                                break Err(format!("focus spatial pose write failed: {e}"));
                            }
                        }
                    }
                }
                if let Err(e) = write_frame(&mut stream, &Message::FocusReturn) {
                    break Err(format!("focus return write failed: {e}"));
                }
            }
        }

        if let Some(rx) = spatial_rx {
            let mut latest_pose = None;
            while let Ok(p) = rx.try_recv() {
                latest_pose = Some(p);
            }
            if let Some(pose) = latest_pose {
                if let Err(e) = write_frame(&mut stream, &Message::SpatialPose(pose)) {
                    break Err(format!("spatial pose write failed: {e}"));
                }
            }
        }

        match read_frame(&mut stream) {
            Ok(msg) => {
                last_rx = Instant::now();
                match msg {
                    Message::KeyReport(report) => {
                        if let Err(e) = sink.send_keyboard(report) {
                            break Err(e);
                        }
                    }
                    Message::MouseReport {
                        buttons,
                        dx,
                        dy,
                        wheel_v,
                        wheel_h,
                    } => {
                        if let Err(e) = sink.send_mouse(buttons, dx, dy, wheel_v, wheel_h) {
                            break Err(e);
                        }
                    }
                    Message::ConsumerReport(usage) => {
                        if let Err(e) = sink.send_consumer(usage) {
                            break Err(e);
                        }
                    }
                    Message::ReleaseAll => {
                        if let Err(e) = sink.release_all() {
                            break Err(e);
                        }
                    }
                    Message::Ping(seq) => {
                        if let Err(e) = write_frame(&mut stream, &Message::Pong(seq)) {
                            break Err(format!("pong write failed: {e}"));
                        }
                    }
                    Message::QuerySpatialPose => {
                        if let Some(tx) = spatial_trigger_tx {
                            let _ = tx.send(());
                        }
                    }
                    Message::WarpToEdge {
                        edge,
                        norm_x,
                        norm_y,
                        gain,
                    } => {
                        if let Some(pkg) = mirror_package {
                            if let Some(pose) = servo.warp_to_edge(
                                sink, pkg, edge, norm_x, norm_y, gain, verbose,
                            ) {
                                if let Err(e) = write_frame(&mut stream, &Message::SpatialPose(pose))
                                {
                                    break Err(format!("post-warp pose write failed: {e}"));
                                }
                            }
                        }
                    }
                    Message::Hello { .. }
                    | Message::HelloAck { .. }
                    | Message::Pong(_)
                    | Message::FocusReturn
                    | Message::SpatialPose(_) => {}
                }
            }
            Err(WireError::Io(ref e))
                if e.kind() == ErrorKind::UnexpectedEof || e.kind() == ErrorKind::ConnectionReset =>
            {
                break Ok(());
            }
            Err(WireError::Io(ref e))
                if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock =>
            {
                if last_rx.elapsed() >= Duration::from_secs(3) {
                    break Err("client heartbeat timed out; releasing HID state".to_string());
                }
                continue;
            }
            Err(e) => break Err(format!("stream read error: {e}")),
        }
    };

    let _ = sink.release_all();
    result
}
