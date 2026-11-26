//! HID output sinks: Linux `/dev/uhid` virtual devices and cross-platform mock sink.

use std::sync::{Arc, Mutex};

/// Abstraction over kernel UHID injection (on Android/Linux) and test mocks.
pub trait HidSink {
    fn send_keyboard(&mut self, report: [u8; 8]) -> Result<(), String>;
    fn send_mouse(
        &mut self,
        buttons: u8,
        dx: i16,
        dy: i16,
        wheel_v: i8,
        wheel_h: i8,
    ) -> Result<(), String>;
    fn send_consumer(&mut self, usage: u16) -> Result<(), String>;
    fn release_all(&mut self) -> Result<(), String>;
    fn poll_drain(&mut self) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedEvent {
    Keyboard([u8; 8]),
    Mouse {
        buttons: u8,
        dx: i16,
        dy: i16,
        wheel_v: i8,
        wheel_h: i8,
    },
    Consumer(u16),
    ReleaseAll,
}

#[derive(Debug, Default, Clone)]
pub struct MockHidSink {
    pub events: Arc<Mutex<Vec<RecordedEvent>>>,
    pub verbose: bool,
}

impl MockHidSink {
    pub fn new(verbose: bool) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            verbose,
        }
    }

    pub fn recorded(&self) -> Vec<RecordedEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl HidSink for MockHidSink {
    fn send_keyboard(&mut self, report: [u8; 8]) -> Result<(), String> {
        if self.verbose {
            eprintln!("[mock-uhid] keyboard={report:02x?}");
        }
        self.events
            .lock()
            .unwrap()
            .push(RecordedEvent::Keyboard(report));
        Ok(())
    }

    fn send_mouse(
        &mut self,
        buttons: u8,
        dx: i16,
        dy: i16,
        wheel_v: i8,
        wheel_h: i8,
    ) -> Result<(), String> {
        if self.verbose {
            eprintln!(
                "[mock-uhid] mouse buttons=0x{buttons:02x} dx={dx} dy={dy} wv={wheel_v} wh={wheel_h}"
            );
        }
        self.events.lock().unwrap().push(RecordedEvent::Mouse {
            buttons,
            dx,
            dy,
            wheel_v,
            wheel_h,
        });
        Ok(())
    }

    fn send_consumer(&mut self, usage: u16) -> Result<(), String> {
        if self.verbose {
            eprintln!("[mock-uhid] consumer=0x{usage:04x}");
        }
        self.events
            .lock()
            .unwrap()
            .push(RecordedEvent::Consumer(usage));
        Ok(())
    }

    fn release_all(&mut self) -> Result<(), String> {
        if self.verbose {
            eprintln!("[mock-uhid] release_all");
        }
        self.events.lock().unwrap().push(RecordedEvent::ReleaseAll);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub mod linux {
    use super::HidSink;
    use aura_protocol::{KEYBOARD_REPORT_DESCRIPTOR, MOUSE_REPORT_DESCRIPTOR};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::mem::size_of;
    use std::os::unix::fs::OpenOptionsExt;
    use std::slice;

    const UHID_DESTROY: u32 = 1;
    const UHID_CREATE2: u32 = 11;
    const UHID_INPUT2: u32 = 12;

    const BUS_VIRTUAL: u16 = 0x06;
    const VENDOR_GOOGLE: u32 = 0x18d1;
    const PRODUCT_KEYBOARD: u32 = 0xA001;
    const PRODUCT_MOUSE: u32 = 0xA002;

    // Identical across x86_64 and aarch64 Linux.
    const O_NONBLOCK_LINUX: i32 = 0o4000;
    const O_CLOEXEC_LINUX: i32 = 0o2000000;

    #[repr(C, packed)]
    pub struct UhidCreate2Event {
        event_type: u32,
        name: [u8; 128],
        phys: [u8; 64],
        uniq: [u8; 64],
        rd_size: u16,
        bus: u16,
        vendor: u32,
        product: u32,
        version: u32,
        country: u32,
        rd_data: [u8; 4096],
    }

    #[repr(C, packed)]
    pub struct UhidInput2Event {
        event_type: u32,
        size: u16,
        data: [u8; 4096],
    }

    #[repr(C, packed)]
    pub struct UhidDestroyEvent {
        event_type: u32,
    }

    fn as_bytes<T>(val: &T) -> &[u8] {
        unsafe { slice::from_raw_parts((val as *const T).cast::<u8>(), size_of::<T>()) }
    }

    fn copy_str(dst: &mut [u8], src: &str) {
        let bytes = src.as_bytes();
        let n = bytes.len().min(dst.len().saturating_sub(1));
        dst[..n].copy_from_slice(&bytes[..n]);
    }

    fn open_uhid_device(
        name: &str,
        uniq: &str,
        product: u32,
        descriptor: &[u8],
    ) -> Result<File, String> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_NONBLOCK_LINUX | O_CLOEXEC_LINUX)
            .open("/dev/uhid")
            .map_err(|e| {
                format!(
                    "failed to open /dev/uhid for '{name}': {e}. \
                     Ensure USB debugging is running as shell/root and SELinux/permissions allow /dev/uhid access."
                )
            })?;

        let mut ev = UhidCreate2Event {
            event_type: UHID_CREATE2,
            name: [0; 128],
            phys: [0; 64],
            uniq: [0; 64],
            rd_size: descriptor.len() as u16,
            bus: BUS_VIRTUAL,
            vendor: VENDOR_GOOGLE,
            product,
            version: 1,
            country: 0,
            rd_data: [0; 4096],
        };
        copy_str(&mut ev.name, name);
        copy_str(&mut ev.phys, "aura-kvm/0");
        copy_str(&mut ev.uniq, uniq);
        ev.rd_data[..descriptor.len()].copy_from_slice(descriptor);

        file.write_all(as_bytes(&ev))
            .map_err(|e| format!("UHID_CREATE2 failed for '{name}': {e}"))?;
        Ok(file)
    }

    fn write_input(file: &mut File, report: &[u8]) -> Result<(), String> {
        let mut ev = UhidInput2Event {
            event_type: UHID_INPUT2,
            size: report.len() as u16,
            data: [0; 4096],
        };
        ev.data[..report.len()].copy_from_slice(report);
        file.write_all(as_bytes(&ev))
            .map_err(|e| format!("UHID_INPUT2 failed: {e}"))
    }

    fn drain_uhid_events(file: &mut File) {
        let mut buf = [0u8; 4380];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }

    pub struct LinuxUhidSink {
        keyboard: File,
        mouse: File,
        verbose: bool,
    }

    impl LinuxUhidSink {
        pub fn open(verbose: bool) -> Result<Self, String> {
            let keyboard = open_uhid_device(
                "Aura KVM Keyboard",
                "aura-kvm-kbd-1",
                PRODUCT_KEYBOARD,
                KEYBOARD_REPORT_DESCRIPTOR,
            )?;
            let mouse = open_uhid_device(
                "Aura KVM Mouse",
                "aura-kvm-mouse-1",
                PRODUCT_MOUSE,
                MOUSE_REPORT_DESCRIPTOR,
            )?;
            if verbose {
                eprintln!("[uhid] created Aura KVM Keyboard and Aura KVM Mouse");
            }
            Ok(Self {
                keyboard,
                mouse,
                verbose,
            })
        }
    }

    impl HidSink for LinuxUhidSink {
        fn send_keyboard(&mut self, report: [u8; 8]) -> Result<(), String> {
            let mut framed = [0u8; 9];
            framed[0] = 1; // Report ID 1 (Keyboard)
            framed[1..9].copy_from_slice(&report);
            if self.verbose {
                eprintln!("[uhid] keyboard={report:02x?}");
            }
            write_input(&mut self.keyboard, &framed)
        }

        fn send_mouse(
            &mut self,
            buttons: u8,
            dx: i16,
            dy: i16,
            wheel_v: i8,
            wheel_h: i8,
        ) -> Result<(), String> {
            let dx_bytes = dx.to_le_bytes();
            let dy_bytes = dy.to_le_bytes();
            let report = [
                buttons,
                dx_bytes[0],
                dx_bytes[1],
                dy_bytes[0],
                dy_bytes[1],
                wheel_v as u8,
                wheel_h as u8,
            ];
            write_input(&mut self.mouse, &report)
        }

        fn send_consumer(&mut self, usage: u16) -> Result<(), String> {
            let b = usage.to_le_bytes();
            let report = [2, b[0], b[1]]; // Report ID 2 (Consumer)
            write_input(&mut self.keyboard, &report)
        }

        fn release_all(&mut self) -> Result<(), String> {
            let _ = self.send_keyboard([0u8; 8]);
            let _ = self.send_consumer(0);
            let _ = self.send_mouse(0, 0, 0, 0, 0);
            Ok(())
        }

        fn poll_drain(&mut self) {
            drain_uhid_events(&mut self.keyboard);
            drain_uhid_events(&mut self.mouse);
        }
    }

    impl Drop for LinuxUhidSink {
        fn drop(&mut self) {
            let _ = self.release_all();
            let destroy = UhidDestroyEvent {
                event_type: UHID_DESTROY,
            };
            let _ = self.keyboard.write_all(as_bytes(&destroy));
            let _ = self.mouse.write_all(as_bytes(&destroy));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn uhid_kernel_struct_sizes_match_abi() {
            assert_eq!(size_of::<UhidCreate2Event>(), 4376);
            assert_eq!(size_of::<UhidInput2Event>(), 4102);
            assert_eq!(size_of::<UhidDestroyEvent>(), 4);
        }
    }
}
