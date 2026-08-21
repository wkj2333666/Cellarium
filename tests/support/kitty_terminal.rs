#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KittyCommand {
    pub control: String,
    pub payload: String,
}

#[derive(Default)]
pub struct KittyStreamParser {
    buffered: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConsumedSharedFrame {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub fn consume_shared_frame(command: &KittyCommand) -> std::io::Result<ConsumedSharedFrame> {
    let fields = command
        .control
        .split(',')
        .filter_map(|field| field.split_once('='))
        .collect::<std::collections::HashMap<_, _>>();
    if fields.get("t") != Some(&"s") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Kitty command is not a shared-memory transmission",
        ));
    }
    let expected = fields
        .get("S")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing S"))?
        .parse::<usize>()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let decoded = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(&command.payload)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&command.payload))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let name = String::from_utf8(decoded)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if !name.starts_with('/') || name[1..].contains('/') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid POSIX shared-memory name",
        ));
    }
    let c_name = CString::new(name.as_bytes())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDONLY, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut bytes = vec![0_u8; expected];
    let read_result = file.read_exact(&mut bytes);
    drop(file);
    let unlink_result = unsafe { libc::shm_unlink(c_name.as_ptr()) };
    read_result?;
    if unlink_result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(ConsumedSharedFrame { name, bytes })
}

impl KittyStreamParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<KittyCommand> {
        const START: &[u8] = b"\x1b_G";
        const END: &[u8] = b"\x1b\\";

        self.buffered.extend_from_slice(bytes);
        let mut commands = Vec::new();
        loop {
            let Some(start) = find_bytes(&self.buffered, START) else {
                retain_possible_prefix(&mut self.buffered, START);
                break;
            };
            if start > 0 {
                self.buffered.drain(..start);
            }
            let Some(end) = find_bytes(&self.buffered[START.len()..], END) else {
                break;
            };
            let body_end = START.len() + end;
            let body = &self.buffered[START.len()..body_end];
            let separator = body.iter().position(|byte| *byte == b';');
            let (control, payload) = match separator {
                Some(index) => (&body[..index], &body[index + 1..]),
                None => (body, &[][..]),
            };
            if let (Ok(control), Ok(payload)) =
                (std::str::from_utf8(control), std::str::from_utf8(payload))
            {
                commands.push(KittyCommand {
                    control: control.to_string(),
                    payload: payload.to_string(),
                });
            }
            self.buffered.drain(..body_end + END.len());
        }
        commands
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn retain_possible_prefix(buffer: &mut Vec<u8>, marker: &[u8]) {
    let retain = (1..marker.len())
        .rev()
        .find(|length| buffer.ends_with(&marker[..*length]))
        .unwrap_or(0);
    if retain == 0 {
        buffer.clear();
    } else {
        buffer.drain(..buffer.len() - retain);
    }
}
use base64::Engine;
use std::ffi::CString;
use std::io::Read;
use std::os::fd::FromRawFd;
