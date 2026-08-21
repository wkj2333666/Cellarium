use crate::input::Command;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::io::{self, Read, Write};

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_FRAME_SIZE: u32 = 64 * 1024 * 1024;
const MAGIC: [u8; 4] = *b"CLRM";

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub width: u32,
    pub height: u32,
    pub tick: u64,
    pub paused: bool,
    pub simulation_rate: f64,
    pub render_rate: f64,
    pub backend: String,
    pub rule: String,
    pub error: Option<String>,
    pub cells: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RemoteMessage {
    Hello,
    Input(InputMessage),
    Snapshot(Snapshot),
    Viewport { width: u16, height: u16 },
    Quit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputMessage {
    Command(Command),
    ExpressionKey(ExpressionKey),
    Mouse(MouseEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpressionKey {
    Char(char),
    Backspace,
    Enter,
    Escape,
}

#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    Invalid(&'static str),
    Message(String),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "protocol I/O error: {error}"),
            Self::Invalid(error) => write!(f, "invalid protocol frame: {error}"),
            Self::Message(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn write_message<W: Write>(
    writer: &mut W,
    message: &RemoteMessage,
) -> Result<(), ProtocolError> {
    let mut payload = Vec::new();
    let tag = encode_message(message, &mut payload)?;
    if payload.len() > MAX_FRAME_SIZE as usize {
        return Err(ProtocolError::Invalid("payload exceeds maximum frame size"));
    }
    writer.write_all(&MAGIC)?;
    writer.write_all(&[PROTOCOL_VERSION, tag])?;
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub fn read_message<R: Read>(reader: &mut R) -> Result<Option<RemoteMessage>, ProtocolError> {
    let mut header = [0_u8; 10];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    if header[..4] != MAGIC {
        return Err(ProtocolError::Invalid("bad magic"));
    }
    if header[4] != PROTOCOL_VERSION {
        return Err(ProtocolError::Invalid("unsupported protocol version"));
    }
    let length = u32::from_le_bytes(header[6..10].try_into().expect("header length"));
    if length > MAX_FRAME_SIZE {
        return Err(ProtocolError::Invalid("payload exceeds maximum frame size"));
    }
    let mut payload = vec![0_u8; length as usize];
    reader.read_exact(&mut payload)?;
    decode_message(header[5], &payload).map(Some)
}

fn encode_message(message: &RemoteMessage, payload: &mut Vec<u8>) -> Result<u8, ProtocolError> {
    match message {
        RemoteMessage::Hello => Ok(1),
        RemoteMessage::Input(input) => {
            encode_input(input, payload)?;
            Ok(2)
        }
        RemoteMessage::Snapshot(snapshot) => {
            encode_snapshot(snapshot, payload)?;
            Ok(3)
        }
        RemoteMessage::Viewport { width, height } => {
            put_u16(payload, *width);
            put_u16(payload, *height);
            Ok(4)
        }
        RemoteMessage::Quit => Ok(5),
    }
}

fn decode_message(tag: u8, payload: &[u8]) -> Result<RemoteMessage, ProtocolError> {
    let mut cursor = Cursor::new(payload);
    match tag {
        1 if payload.is_empty() => Ok(RemoteMessage::Hello),
        2 => Ok(RemoteMessage::Input(decode_input(&mut cursor)?)),
        3 => Ok(RemoteMessage::Snapshot(decode_snapshot(&mut cursor)?)),
        4 => {
            let width = cursor.u16()?;
            let height = cursor.u16()?;
            cursor.finish()?;
            Ok(RemoteMessage::Viewport { width, height })
        }
        5 if payload.is_empty() => Ok(RemoteMessage::Quit),
        _ => Err(ProtocolError::Invalid(
            "unknown message tag or trailing payload",
        )),
    }
}

fn encode_input(input: &InputMessage, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
    match input {
        InputMessage::Command(command) => {
            payload.push(1);
            payload.push(command_code(*command));
        }
        InputMessage::ExpressionKey(key) => {
            payload.push(2);
            match key {
                ExpressionKey::Char(character) => {
                    payload.push(1);
                    put_u32(payload, *character as u32);
                }
                ExpressionKey::Backspace => payload.push(2),
                ExpressionKey::Enter => payload.push(3),
                ExpressionKey::Escape => payload.push(4),
            }
        }
        InputMessage::Mouse(event) => {
            payload.push(3);
            payload.push(mouse_kind_code(event.kind));
            payload.push(mouse_button_code(event.kind));
            put_u16(payload, event.column);
            put_u16(payload, event.row);
            payload.push(modifier_code(event.modifiers));
        }
    }
    Ok(())
}

fn decode_input(cursor: &mut Cursor<'_>) -> Result<InputMessage, ProtocolError> {
    match cursor.u8()? {
        1 => Ok(InputMessage::Command(command_from_code(cursor.u8()?)?)),
        2 => {
            let key = match cursor.u8()? {
                1 => ExpressionKey::Char(
                    char::from_u32(cursor.u32()?)
                        .ok_or(ProtocolError::Invalid("invalid unicode scalar"))?,
                ),
                2 => ExpressionKey::Backspace,
                3 => ExpressionKey::Enter,
                4 => ExpressionKey::Escape,
                _ => return Err(ProtocolError::Invalid("unknown expression key")),
            };
            Ok(InputMessage::ExpressionKey(key))
        }
        3 => {
            let kind = mouse_kind_from_code(cursor.u8()?, cursor.u8()?)?;
            let column = cursor.u16()?;
            let row = cursor.u16()?;
            let modifiers = modifiers_from_code(cursor.u8()?);
            cursor.finish()?;
            Ok(InputMessage::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers,
            }))
        }
        _ => Err(ProtocolError::Invalid("unknown input tag")),
    }
}

fn encode_snapshot(snapshot: &Snapshot, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let expected = (snapshot.width as usize).saturating_mul(snapshot.height as usize);
    if expected != snapshot.cells.len() {
        return Err(ProtocolError::Invalid(
            "snapshot dimensions do not match cells",
        ));
    }
    put_u32(payload, snapshot.width);
    put_u32(payload, snapshot.height);
    put_u64(payload, snapshot.tick);
    payload.push(u8::from(snapshot.paused));
    payload.extend_from_slice(&snapshot.simulation_rate.to_le_bytes());
    payload.extend_from_slice(&snapshot.render_rate.to_le_bytes());
    put_string(payload, &snapshot.backend)?;
    put_string(payload, &snapshot.rule)?;
    match &snapshot.error {
        Some(error) => {
            payload.push(1);
            put_string(payload, error)?;
        }
        None => payload.push(0),
    }
    put_u32(payload, snapshot.cells.len() as u32);
    for value in &snapshot.cells {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn decode_snapshot(cursor: &mut Cursor<'_>) -> Result<Snapshot, ProtocolError> {
    let width = cursor.u32()?;
    let height = cursor.u32()?;
    let tick = cursor.u64()?;
    let paused = match cursor.u8()? {
        0 => false,
        1 => true,
        _ => return Err(ProtocolError::Invalid("invalid paused flag")),
    };
    let simulation_rate = cursor.f64()?;
    let render_rate = cursor.f64()?;
    let backend = cursor.string()?;
    let rule = cursor.string()?;
    let error = match cursor.u8()? {
        0 => None,
        1 => Some(cursor.string()?),
        _ => return Err(ProtocolError::Invalid("invalid error flag")),
    };
    let length = cursor.u32()? as usize;
    let expected = (width as usize)
        .checked_mul(height as usize)
        .ok_or(ProtocolError::Invalid("snapshot dimensions overflow"))?;
    if length != expected || length > MAX_FRAME_SIZE as usize / 4 {
        return Err(ProtocolError::Invalid("invalid snapshot cell count"));
    }
    let mut cells = Vec::with_capacity(length);
    for _ in 0..length {
        cells.push(cursor.f32()?);
    }
    cursor.finish()?;
    Ok(Snapshot {
        width,
        height,
        tick,
        paused,
        simulation_rate,
        render_rate,
        backend,
        rule,
        error,
        cells,
    })
}

fn put_string(payload: &mut Vec<u8>, value: &str) -> Result<(), ProtocolError> {
    let bytes = value.as_bytes();
    if bytes.len() > u16::MAX as usize {
        return Err(ProtocolError::Invalid("string too long"));
    }
    put_u16(payload, bytes.len() as u16);
    payload.extend_from_slice(bytes);
    Ok(())
}

fn command_code(command: Command) -> u8 {
    match command {
        Command::Quit => 0,
        Command::TogglePause => 1,
        Command::Step => 2,
        Command::Reset => 3,
        Command::Randomize => 4,
        Command::Clear => 5,
        Command::Conway => 6,
        Command::Lenia => 7,
        Command::NextKernel => 8,
        Command::NextKernelParameter => 9,
        Command::IncreaseKernelParameter => 10,
        Command::DecreaseKernelParameter => 11,
        Command::RegenerateKernel => 12,
        Command::ToggleKernelPreview => 13,
        Command::NextPanel => 14,
        Command::ToggleExpressionEditor => 15,
    }
}

fn command_from_code(code: u8) -> Result<Command, ProtocolError> {
    Ok(match code {
        0 => Command::Quit,
        1 => Command::TogglePause,
        2 => Command::Step,
        3 => Command::Reset,
        4 => Command::Randomize,
        5 => Command::Clear,
        6 => Command::Conway,
        7 => Command::Lenia,
        8 => Command::NextKernel,
        9 => Command::NextKernelParameter,
        10 => Command::IncreaseKernelParameter,
        11 => Command::DecreaseKernelParameter,
        12 => Command::RegenerateKernel,
        13 => Command::ToggleKernelPreview,
        14 => Command::NextPanel,
        15 => Command::ToggleExpressionEditor,
        _ => return Err(ProtocolError::Invalid("unknown command")),
    })
}

fn mouse_kind_code(kind: MouseEventKind) -> u8 {
    match kind {
        MouseEventKind::Down(_) => 1,
        MouseEventKind::Up(_) => 2,
        MouseEventKind::Drag(_) => 3,
        MouseEventKind::Moved => 4,
        MouseEventKind::ScrollUp => 5,
        MouseEventKind::ScrollDown => 6,
        _ => 0,
    }
}

fn mouse_button_code(kind: MouseEventKind) -> u8 {
    match kind {
        MouseEventKind::Down(button)
        | MouseEventKind::Up(button)
        | MouseEventKind::Drag(button) => match button {
            MouseButton::Left => 1,
            MouseButton::Right => 2,
            MouseButton::Middle => 3,
        },
        _ => 0,
    }
}

fn mouse_kind_from_code(kind: u8, button: u8) -> Result<MouseEventKind, ProtocolError> {
    let button = match button {
        1 => MouseButton::Left,
        2 => MouseButton::Right,
        3 => MouseButton::Middle,
        0 => MouseButton::Left,
        _ => return Err(ProtocolError::Invalid("unknown mouse button")),
    };
    Ok(match kind {
        1 => MouseEventKind::Down(button),
        2 => MouseEventKind::Up(button),
        3 => MouseEventKind::Drag(button),
        4 => MouseEventKind::Moved,
        5 => MouseEventKind::ScrollUp,
        6 => MouseEventKind::ScrollDown,
        _ => return Err(ProtocolError::Invalid("unknown mouse kind")),
    })
}

fn modifier_code(modifiers: KeyModifiers) -> u8 {
    let mut code = 0;
    if modifiers.contains(KeyModifiers::SHIFT) {
        code |= 1;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        code |= 2;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        code |= 4;
    }
    code
}

fn modifiers_from_code(code: u8) -> KeyModifiers {
    let mut modifiers = KeyModifiers::NONE;
    if code & 1 != 0 {
        modifiers |= KeyModifiers::SHIFT;
    }
    if code & 2 != 0 {
        modifiers |= KeyModifiers::CONTROL;
    }
    if code & 4 != 0 {
        modifiers |= KeyModifiers::ALT;
    }
    modifiers
}

fn put_u16(target: &mut Vec<u8>, value: u16) {
    target.extend_from_slice(&value.to_le_bytes());
}
fn put_u32(target: &mut Vec<u8>, value: u32) {
    target.extend_from_slice(&value.to_le_bytes());
}
fn put_u64(target: &mut Vec<u8>, value: u64) {
    target.extend_from_slice(&value.to_le_bytes());
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ProtocolError::Invalid("cursor overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(ProtocolError::Invalid("truncated payload"))?;
        self.offset = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(*self.take(1)?.first().expect("one byte"))
    }
    fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }
    fn f32(&mut self) -> Result<f32, ProtocolError> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    fn f64(&mut self) -> Result<f64, ProtocolError> {
        Ok(f64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }
    fn string(&mut self) -> Result<String, ProtocolError> {
        let length = self.u16()? as usize;
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| ProtocolError::Invalid("invalid utf-8 string"))
    }
    fn finish(&self) -> Result<(), ProtocolError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ProtocolError::Invalid("trailing payload"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor as IoCursor;

    fn sample_snapshot() -> Snapshot {
        Snapshot {
            width: 2,
            height: 2,
            tick: 7,
            paused: true,
            simulation_rate: 12.5,
            render_rate: 30.0,
            backend: "CPU".into(),
            rule: "Conway".into(),
            error: Some("oops".into()),
            cells: vec![0.0, 1.0, 0.25, 0.75],
        }
    }

    #[test]
    fn snapshot_round_trip_preserves_state() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &RemoteMessage::Snapshot(sample_snapshot())).unwrap();
        assert_eq!(
            read_message(&mut IoCursor::new(bytes)).unwrap(),
            Some(RemoteMessage::Snapshot(sample_snapshot()))
        );
    }

    #[test]
    fn command_and_mouse_round_trip() {
        let messages = [
            RemoteMessage::Input(InputMessage::Command(Command::TogglePause)),
            RemoteMessage::Input(InputMessage::ExpressionKey(ExpressionKey::Char('中'))),
            RemoteMessage::Input(InputMessage::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 3,
                row: 4,
                modifiers: KeyModifiers::ALT,
            })),
        ];
        for message in messages {
            let mut bytes = Vec::new();
            write_message(&mut bytes, &message).unwrap();
            assert_eq!(
                read_message(&mut IoCursor::new(bytes)).unwrap(),
                Some(message)
            );
        }
    }

    #[test]
    fn malformed_magic_and_truncated_payload_are_rejected() {
        let mut malformed = b"BAD!".to_vec();
        malformed.extend_from_slice(&[PROTOCOL_VERSION, 1, 0, 0, 0, 0]);
        assert!(read_message(&mut IoCursor::new(malformed)).is_err());

        let mut truncated = Vec::new();
        truncated.extend_from_slice(&MAGIC);
        truncated.extend_from_slice(&[PROTOCOL_VERSION, 1, 4, 0, 0, 0]);
        truncated.extend_from_slice(&[1, 2]);
        assert!(read_message(&mut IoCursor::new(truncated)).is_err());
    }

    #[test]
    fn eof_is_a_clean_disconnect() {
        assert_eq!(
            read_message(&mut IoCursor::new(Vec::<u8>::new())).unwrap(),
            None
        );
    }
}
