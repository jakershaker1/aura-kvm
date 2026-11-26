//! Framed binary wire protocol between `aura-host` and `aura-puck-agent`.

use std::fmt;
use std::io::{self, Read, Write};

pub const PROTOCOL_MAGIC: [u8; 4] = *b"AKVM";
pub const PROTOCOL_VERSION: u16 = 1;
pub const DEFAULT_PORT: u16 = 27190;
pub const MAX_PAYLOAD_LEN: usize = 1024;

pub const CAP_KEYBOARD: u16 = 1 << 0;
pub const CAP_MOUSE: u16 = 1 << 1;
pub const CAP_CONSUMER: u16 = 1 << 2;
pub const CAP_FOCUS_WATCH: u16 = 1 << 3;
pub const CAP_SPATIAL_POSE: u16 = 1 << 4;
pub const CAP_SPATIAL_WARP: u16 = 1 << 5;

pub const WARP_EDGE_LEFT: u8 = 0;
pub const WARP_EDGE_RIGHT: u8 = 1;
pub const WARP_EDGE_TOP: u8 = 2;
pub const WARP_EDGE_BOTTOM: u8 = 3;

pub const SPATIAL_FLAG_HAS_HEAD: u8 = 1 << 0;
pub const SPATIAL_FLAG_HAS_PANEL: u8 = 1 << 1;
pub const SPATIAL_FLAG_PANEL_ACTIVE: u8 = 1 << 2;
pub const SPATIAL_FLAG_HAS_CURSOR: u8 = 1 << 3;
pub const SPATIAL_FLAG_CURSOR_ACTIVE: u8 = 1 << 4;

const TAG_HELLO: u8 = 0x00;
const TAG_KEY_REPORT: u8 = 0x01;
const TAG_MOUSE_REPORT: u8 = 0x02;
const TAG_CONSUMER_REPORT: u8 = 0x03;
const TAG_RELEASE_ALL: u8 = 0x04;
const TAG_PING: u8 = 0x05;
const TAG_PONG: u8 = 0x06;
const TAG_FOCUS_RETURN: u8 = 0x07;
const TAG_QUERY_SPATIAL_POSE: u8 = 0x08;
const TAG_SPATIAL_POSE: u8 = 0x09;
const TAG_WARP_TO_EDGE: u8 = 0x0A;
const TAG_HELLO_ACK: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpatialPose {
    pub flags: u8,
    pub task_id: u32,
    pub head_pos: [f32; 3],
    pub head_quat: [f32; 4],    // (w, x, y, z)
    pub panel_pos: [f32; 3],
    pub panel_quat: [f32; 4],   // (w, x, y, z)
    pub panel_size_m: [f32; 2], // (width_m, height_m)
    pub cursor_pos: [f32; 3],
}

impl SpatialPose {
    pub fn has_head(&self) -> bool {
        (self.flags & SPATIAL_FLAG_HAS_HEAD) != 0
    }

    pub fn has_panel(&self) -> bool {
        (self.flags & SPATIAL_FLAG_HAS_PANEL) != 0
    }

    pub fn is_panel_active(&self) -> bool {
        (self.flags & SPATIAL_FLAG_PANEL_ACTIVE) != 0
    }

    pub fn has_cursor(&self) -> bool {
        (self.flags & SPATIAL_FLAG_HAS_CURSOR) != 0
            && self.cursor_pos.iter().all(|v| v.is_finite())
    }

    pub fn is_cursor_active(&self) -> bool {
        (self.flags & SPATIAL_FLAG_CURSOR_ACTIVE) != 0 && self.has_cursor()
    }

    pub fn is_usable(&self) -> bool {
        self.has_head()
            && self.has_panel()
            && self.panel_size_m[0].is_finite()
            && self.panel_size_m[1].is_finite()
            && self.panel_size_m[0] > 0.05
            && self.panel_size_m[1] > 0.05
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    Hello {
        version: u16,
    },
    HelloAck {
        version: u16,
        capabilities: u16,
    },
    KeyReport([u8; 8]),
    MouseReport {
        buttons: u8,
        dx: i16,
        dy: i16,
        wheel_v: i8,
        wheel_h: i8,
    },
    ConsumerReport(u16),
    ReleaseAll,
    Ping(u64),
    Pong(u64),
    FocusReturn,
    QuerySpatialPose,
    SpatialPose(SpatialPose),
    WarpToEdge {
        edge: u8,
        norm_x: f32,
        norm_y: f32,
        gain: f32,
    },
}

#[derive(Debug)]
pub enum WireError {
    Io(io::Error),
    PayloadTooLarge(usize),
    InvalidMagic([u8; 4]),
    InvalidLength { tag: u8, len: usize },
    UnknownTag(u8),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Io(e) => write!(f, "I/O error: {e}"),
            WireError::PayloadTooLarge(len) => write!(f, "payload length {len} exceeds {MAX_PAYLOAD_LEN}"),
            WireError::InvalidMagic(m) => write!(f, "invalid protocol magic: {m:02x?}"),
            WireError::InvalidLength { tag, len } => {
                write!(f, "invalid payload length {len} for tag 0x{tag:02x}")
            }
            WireError::UnknownTag(tag) => write!(f, "unknown message tag 0x{tag:02x}"),
        }
    }
}

impl std::error::Error for WireError {}

impl From<io::Error> for WireError {
    fn from(err: io::Error) -> Self {
        WireError::Io(err)
    }
}

pub fn write_frame<W: Write>(writer: &mut W, msg: &Message) -> Result<(), WireError> {
    let mut buf = [0u8; 128];
    let (tag, payload_len) = match *msg {
        Message::Hello { version } => {
            buf[3..7].copy_from_slice(&PROTOCOL_MAGIC);
            buf[7..9].copy_from_slice(&version.to_le_bytes());
            (TAG_HELLO, 6)
        }
        Message::HelloAck {
            version,
            capabilities,
        } => {
            buf[3..7].copy_from_slice(&PROTOCOL_MAGIC);
            buf[7..9].copy_from_slice(&version.to_le_bytes());
            buf[9..11].copy_from_slice(&capabilities.to_le_bytes());
            (TAG_HELLO_ACK, 8)
        }
        Message::KeyReport(report) => {
            buf[3..11].copy_from_slice(&report);
            (TAG_KEY_REPORT, 8)
        }
        Message::MouseReport {
            buttons,
            dx,
            dy,
            wheel_v,
            wheel_h,
        } => {
            buf[3] = buttons;
            buf[4..6].copy_from_slice(&dx.to_le_bytes());
            buf[6..8].copy_from_slice(&dy.to_le_bytes());
            buf[8] = wheel_v as u8;
            buf[9] = wheel_h as u8;
            (TAG_MOUSE_REPORT, 7)
        }
        Message::ConsumerReport(usage) => {
            buf[3..5].copy_from_slice(&usage.to_le_bytes());
            (TAG_CONSUMER_REPORT, 2)
        }
        Message::ReleaseAll => (TAG_RELEASE_ALL, 0),
        Message::Ping(seq) => {
            buf[3..11].copy_from_slice(&seq.to_le_bytes());
            (TAG_PING, 8)
        }
        Message::Pong(seq) => {
            buf[3..11].copy_from_slice(&seq.to_le_bytes());
            (TAG_PONG, 8)
        }
        Message::FocusReturn => (TAG_FOCUS_RETURN, 0),
        Message::QuerySpatialPose => (TAG_QUERY_SPATIAL_POSE, 0),
        Message::SpatialPose(pose) => {
            buf[3] = pose.flags;
            buf[4..8].copy_from_slice(&pose.task_id.to_le_bytes());
            let mut offset = 8;
            for f in pose
                .head_pos
                .iter()
                .chain(pose.head_quat.iter())
                .chain(pose.panel_pos.iter())
                .chain(pose.panel_quat.iter())
                .chain(pose.panel_size_m.iter())
                .chain(pose.cursor_pos.iter())
            {
                buf[offset..offset + 4].copy_from_slice(&f.to_le_bytes());
                offset += 4;
            }
            (TAG_SPATIAL_POSE, 81)
        }
        Message::WarpToEdge {
            edge,
            norm_x,
            norm_y,
            gain,
        } => {
            buf[3] = edge;
            buf[4..8].copy_from_slice(&norm_x.to_le_bytes());
            buf[8..12].copy_from_slice(&norm_y.to_le_bytes());
            buf[12..16].copy_from_slice(&gain.to_le_bytes());
            (TAG_WARP_TO_EDGE, 13)
        }
    };

    buf[0] = tag;
    let len_bytes = (payload_len as u16).to_le_bytes();
    buf[1] = len_bytes[0];
    buf[2] = len_bytes[1];

    writer.write_all(&buf[..3 + payload_len])?;
    Ok(())
}

pub fn read_frame<R: Read>(reader: &mut R) -> Result<Message, WireError> {
    let mut header = [0u8; 3];
    reader.read_exact(&mut header)?;
    let tag = header[0];
    let len = u16::from_le_bytes([header[1], header[2]]) as usize;
    if len > MAX_PAYLOAD_LEN {
        return Err(WireError::PayloadTooLarge(len));
    }

    let mut payload = [0u8; MAX_PAYLOAD_LEN];
    if len > 0 {
        reader.read_exact(&mut payload[..len])?;
    }
    let p = &payload[..len];

    match tag {
        TAG_HELLO => {
            if len != 6 {
                return Err(WireError::InvalidLength { tag, len });
            }
            let magic: [u8; 4] = p[0..4].try_into().unwrap();
            if magic != PROTOCOL_MAGIC {
                return Err(WireError::InvalidMagic(magic));
            }
            let version = u16::from_le_bytes([p[4], p[5]]);
            Ok(Message::Hello { version })
        }
        TAG_HELLO_ACK => {
            if len != 8 {
                return Err(WireError::InvalidLength { tag, len });
            }
            let magic: [u8; 4] = p[0..4].try_into().unwrap();
            if magic != PROTOCOL_MAGIC {
                return Err(WireError::InvalidMagic(magic));
            }
            let version = u16::from_le_bytes([p[4], p[5]]);
            let capabilities = u16::from_le_bytes([p[6], p[7]]);
            Ok(Message::HelloAck {
                version,
                capabilities,
            })
        }
        TAG_KEY_REPORT => {
            if len != 8 {
                return Err(WireError::InvalidLength { tag, len });
            }
            let report: [u8; 8] = p.try_into().unwrap();
            Ok(Message::KeyReport(report))
        }
        TAG_MOUSE_REPORT => {
            if len != 7 {
                return Err(WireError::InvalidLength { tag, len });
            }
            Ok(Message::MouseReport {
                buttons: p[0],
                dx: i16::from_le_bytes([p[1], p[2]]),
                dy: i16::from_le_bytes([p[3], p[4]]),
                wheel_v: p[5] as i8,
                wheel_h: p[6] as i8,
            })
        }
        TAG_CONSUMER_REPORT => {
            if len != 2 {
                return Err(WireError::InvalidLength { tag, len });
            }
            Ok(Message::ConsumerReport(u16::from_le_bytes([p[0], p[1]])))
        }
        TAG_RELEASE_ALL => {
            if len != 0 {
                return Err(WireError::InvalidLength { tag, len });
            }
            Ok(Message::ReleaseAll)
        }
        TAG_PING => {
            if len != 8 {
                return Err(WireError::InvalidLength { tag, len });
            }
            Ok(Message::Ping(u64::from_le_bytes(p.try_into().unwrap())))
        }
        TAG_PONG => {
            if len != 8 {
                return Err(WireError::InvalidLength { tag, len });
            }
            Ok(Message::Pong(u64::from_le_bytes(p.try_into().unwrap())))
        }
        TAG_FOCUS_RETURN => {
            if len != 0 {
                return Err(WireError::InvalidLength { tag, len });
            }
            Ok(Message::FocusReturn)
        }
        TAG_QUERY_SPATIAL_POSE => {
            if len != 0 {
                return Err(WireError::InvalidLength { tag, len });
            }
            Ok(Message::QuerySpatialPose)
        }
        TAG_SPATIAL_POSE => {
            if len != 81 {
                return Err(WireError::InvalidLength { tag, len });
            }
            let read_f32 = |idx: usize| -> f32 {
                let start = 5 + idx * 4;
                f32::from_le_bytes([p[start], p[start + 1], p[start + 2], p[start + 3]])
            };
            let pose = SpatialPose {
                flags: p[0],
                task_id: u32::from_le_bytes([p[1], p[2], p[3], p[4]]),
                head_pos: [read_f32(0), read_f32(1), read_f32(2)],
                head_quat: [read_f32(3), read_f32(4), read_f32(5), read_f32(6)],
                panel_pos: [read_f32(7), read_f32(8), read_f32(9)],
                panel_quat: [read_f32(10), read_f32(11), read_f32(12), read_f32(13)],
                panel_size_m: [read_f32(14), read_f32(15)],
                cursor_pos: [read_f32(16), read_f32(17), read_f32(18)],
            };
            Ok(Message::SpatialPose(pose))
        }
        TAG_WARP_TO_EDGE => {
            if len != 13 {
                return Err(WireError::InvalidLength { tag, len });
            }
            let norm_x = f32::from_le_bytes([p[1], p[2], p[3], p[4]]);
            let norm_y = f32::from_le_bytes([p[5], p[6], p[7], p[8]]);
            let gain = f32::from_le_bytes([p[9], p[10], p[11], p[12]]);
            Ok(Message::WarpToEdge {
                edge: p[0],
                norm_x,
                norm_y,
                gain,
            })
        }
        other => Err(WireError::UnknownTag(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_all_message_types() {
        let samples = [
            Message::Hello { version: PROTOCOL_VERSION },
            Message::HelloAck {
                version: PROTOCOL_VERSION,
                capabilities: CAP_KEYBOARD | CAP_MOUSE | CAP_CONSUMER,
            },
            Message::KeyReport([0x05, 0x00, 0x04, 0x05, 0, 0, 0, 0]),
            Message::MouseReport {
                buttons: 0b0001_0101,
                dx: -1234,
                dy: 5678,
                wheel_v: -3,
                wheel_h: 2,
            },
            Message::ConsumerReport(0x0223),
            Message::ReleaseAll,
            Message::Ping(42),
            Message::Pong(42),
            Message::FocusReturn,
            Message::QuerySpatialPose,
            Message::SpatialPose(SpatialPose {
                flags: SPATIAL_FLAG_HAS_HEAD
                    | SPATIAL_FLAG_HAS_PANEL
                    | SPATIAL_FLAG_PANEL_ACTIVE
                    | SPATIAL_FLAG_HAS_CURSOR
                    | SPATIAL_FLAG_CURSOR_ACTIVE,
                task_id: 82,
                head_pos: [-0.068, -0.046, -0.15],
                head_quat: [0.712, -0.698, 0.003, -0.079],
                panel_pos: [4.795, 0.283, -1.792],
                panel_quat: [0.96, 0.0, -0.279, 0.0],
                panel_size_m: [1.09, 0.613],
                cursor_pos: [0.243, 0.467, -1.607],
            }),
            Message::WarpToEdge {
                edge: WARP_EDGE_RIGHT,
                norm_x: 1.0,
                norm_y: -0.35,
                gain: 1.0,
            },
        ];

        let mut buf = Vec::new();
        for msg in &samples {
            write_frame(&mut buf, msg).unwrap();
        }

        let mut cursor = Cursor::new(buf);
        for expected in &samples {
            let decoded = read_frame(&mut cursor).unwrap();
            assert_eq!(decoded, *expected);
        }
    }

    #[test]
    fn rejects_oversized_payload_and_bad_magic() {
        let oversized = [TAG_RELEASE_ALL, 0x01, 0x04]; // len = 1025
        let mut cursor = Cursor::new(oversized);
        assert!(matches!(
            read_frame(&mut cursor),
            Err(WireError::PayloadTooLarge(1025))
        ));

        let bad_magic = [TAG_HELLO, 6, 0, b'B', b'A', b'D', b'!', 1, 0];
        let mut cursor = Cursor::new(bad_magic);
        assert!(matches!(
            read_frame(&mut cursor),
            Err(WireError::InvalidMagic(_))
        ));
    }
}
