//! Windows logical drives, queried from the OS rather than a hard-coded list.
//! Potentially slow volume-label queries run off the UI thread. A blocked
//! network/removable drive can hold at most one worker for that drive letter.
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use std::{ptr, thread};

#[link(name = "kernel32")]
extern "system" {
    fn GetLogicalDrives() -> u32;
    fn GetVolumeInformationW(root: *const u16, name: *mut u16, name_len: u32,
        serial: *mut u32, max_component: *mut u32, flags: *mut u32,
        filesystem: *mut u16, filesystem_len: u32) -> i32;
    fn SetThreadErrorMode(mode: u32, old_mode: *mut u32) -> i32;
}

struct Slot { epoch: u64, in_flight: bool, label: Option<String>, next_query: Instant }
struct Response { index: usize, epoch: u64, label: Option<String> }

pub(super) struct Drives {
    mask: u32,
    slots: [Slot; 26],
    sender: Sender<Response>,
    receiver: Receiver<Response>,
}
impl Drives {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { mask: 0, slots: std::array::from_fn(|_| Slot {
            epoch: 0, in_flight: false, label: None, next_query: Instant::now(),
        }), sender, receiver }
    }

    /// Update the logical-drive mask, receive finished label queries, and start
    /// overdue queries. Returns true only when visible information changed.
    pub fn poll(&mut self, with_labels: bool) -> bool {
        let mut changed = false;
        let mask = unsafe { GetLogicalDrives() };
        // Zero signifies an OS error, not a successful empty snapshot. Retain
        // the previous snapshot instead of inventing drives or erasing it.
        if mask != 0 && mask != self.mask {
            let delta = self.mask ^ mask;
            for index in 0..26 {
                if delta & (1u32 << index) != 0 {
                    let slot = &mut self.slots[index];
                    slot.epoch = slot.epoch.wrapping_add(1);
                    slot.label = None;
                    slot.next_query = Instant::now();
                }
            }
            self.mask = mask;
            changed = true;
        }
        while let Ok(response) = self.receiver.try_recv() {
            let slot = &mut self.slots[response.index];
            slot.in_flight = false;
            if slot.epoch == response.epoch && self.mask & (1 << response.index) != 0 {
                if slot.label != response.label { slot.label = response.label; changed = true; }
                slot.next_query = Instant::now() + Duration::from_secs(30);
            }
        }
        if with_labels {
            for index in 0..26 {
                let slot = &mut self.slots[index];
                if self.mask & (1 << index) == 0 || slot.in_flight || Instant::now() < slot.next_query { continue; }
                slot.in_flight = true;
                let sender = self.sender.clone();
                let epoch = slot.epoch;
                thread::spawn(move || {
                    // Avoid "No disk" popups for empty removable drives.
                    let mut old = 0;
                    let mode_set = unsafe { SetThreadErrorMode(1, &mut old) != 0 };
                    let root = [b'A' as u16 + index as u16, ':' as u16, '\\' as u16, 0];
                    let mut buffer = [0u16; 261];
                    let ok = unsafe { GetVolumeInformationW(root.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32,
                        ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), 0) != 0 };
                    if mode_set { unsafe { SetThreadErrorMode(old, ptr::null_mut()); } }
                    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
                    let label = if ok {
                        let value = String::from_utf16_lossy(&buffer[..end]);
                        let value: String = value.chars().filter(|c| !c.is_control()).take(80).collect();
                        let value = value.trim();
                        if value.is_empty() { None } else { Some(value.to_owned()) }
                    } else { None };
                    let _ = sender.send(Response { index, epoch, label });
                });
            }
        }
        changed
    }

    pub fn entries(&self, with_labels: bool) -> Vec<(String, String)> {
        (0..26).filter(|index| self.mask & (1 << index) != 0).map(|index| {
            let letter = (b'A' + index as u8) as char;
            let name = if with_labels {
                self.slots[index].label.as_ref().map(|label| format!("{letter}: — {label}"))
            } else { None }.unwrap_or_else(|| format!("{letter}:"));
            (name, format!("{letter}:\\"))
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn displays_only_reported_letters_and_available_volume_names() {
        let mut drives = Drives::new();
        drives.mask = (1 << 2) | (1 << 3);
        drives.slots[2].label = Some("System".into());
        assert_eq!(drives.entries(true), vec![("C: — System".into(), "C:\\".into()), ("D:".into(), "D:\\".into())]);
        assert_eq!(drives.entries(false)[0].0, "C:");
        drives.mask = 1 << 3;
        assert_eq!(drives.entries(true), vec![("D:".into(), "D:\\".into())]);
    }
}
