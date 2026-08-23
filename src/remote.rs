use crate::input::Command;
use crate::sim::kernel::KernelDefinition;
use crate::sim::rule::SimulationSpec;
use crate::sim::service::{ApplyAccepted, ApplyRejected, ApplyRequest, Diagnostic, DiagnosticPath};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::io::{self, Read, Write};

pub const PROTOCOL_VERSION: u8 = 9;
pub const MAX_FRAME_SIZE: u32 = 64 * 1024 * 1024;
const MAX_EXPERIMENT_SIZE: usize = 8 * 1024 * 1024;
const MAX_CHANNELS: usize = 64;
const MAX_BASIS_SITES: usize = 4096;
const MAX_RULE_SETS: usize = 4096;
const MAX_BINDINGS: usize = MAX_CHANNELS * MAX_BASIS_SITES;
const MAX_KERNELS_PER_RULE_SET: usize = 256;
const MAGIC: [u8; 4] = *b"CLRM";

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub width: u32,
    pub height: u32,
    pub tick: u64,
    pub paused: bool,
    pub simulation_rate: f64,
    pub render_rate: f64,
    pub last_step_ms: f64,
    pub average_step_ms: f64,
    pub step_samples: u64,
    pub applied_input_sequence: u64,
    pub backend: String,
    pub rule: String,
    pub spec: Box<SimulationSpec>,
    pub selected_kernel: Box<KernelDefinition>,
    pub selected_parameter: Option<String>,
    pub error: Option<String>,
    pub cells: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RemoteMessage {
    Hello,
    Input {
        sequence: u64,
        input: InputMessage,
    },
    Snapshot(Snapshot),
    Viewport {
        width: u16,
        height: u16,
        frame_width: u32,
        frame_height: u32,
    },
    Quit,
    ExperimentState {
        revision: u64,
        normalized_experiment: crate::sim::experiment_model::ExperimentSpec,
    },
    ApplyDraft(ApplyRequest),
    ApplyAccepted(ApplyAccepted),
    ApplyRejected(ApplyRejected),
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
        return Err(ProtocolError::Message(format!(
            "unsupported protocol version {}; expected version {}",
            header[4], PROTOCOL_VERSION
        )));
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
        RemoteMessage::Input { sequence, input } => {
            put_u64(payload, *sequence);
            encode_input(input, payload)?;
            Ok(2)
        }
        RemoteMessage::Snapshot(snapshot) => {
            encode_snapshot(snapshot, payload)?;
            Ok(3)
        }
        RemoteMessage::Viewport {
            width,
            height,
            frame_width,
            frame_height,
        } => {
            put_u16(payload, *width);
            put_u16(payload, *height);
            put_u32(payload, *frame_width);
            put_u32(payload, *frame_height);
            Ok(4)
        }
        RemoteMessage::Quit => Ok(5),
        RemoteMessage::ExperimentState {
            revision,
            normalized_experiment,
        } => {
            put_u64(payload, *revision);
            put_experiment(payload, normalized_experiment)?;
            Ok(6)
        }
        RemoteMessage::ApplyDraft(request) => {
            put_u64(payload, request.request_id);
            put_u64(payload, request.base_revision);
            put_experiment(payload, &request.draft)?;
            Ok(7)
        }
        RemoteMessage::ApplyAccepted(accepted) => {
            put_u64(payload, accepted.request_id);
            put_u64(payload, accepted.revision);
            put_experiment(payload, &accepted.normalized_experiment)?;
            Ok(8)
        }
        RemoteMessage::ApplyRejected(rejected) => {
            put_u64(payload, rejected.request_id);
            let count = u16::try_from(rejected.diagnostics.len())
                .map_err(|_| ProtocolError::Invalid("too many diagnostics"))?;
            put_u16(payload, count);
            for diagnostic in &rejected.diagnostics {
                put_string(payload, &diagnostic.code)?;
                put_string(payload, &diagnostic.message)?;
                let path_count = u16::try_from(diagnostic.path.0.len())
                    .map_err(|_| ProtocolError::Invalid("diagnostic path is too long"))?;
                put_u16(payload, path_count);
                for component in &diagnostic.path.0 {
                    put_string(payload, component)?;
                }
            }
            Ok(9)
        }
    }
}

fn decode_message(tag: u8, payload: &[u8]) -> Result<RemoteMessage, ProtocolError> {
    let mut cursor = Cursor::new(payload);
    match tag {
        1 if payload.is_empty() => Ok(RemoteMessage::Hello),
        2 => {
            let sequence = cursor.u64()?;
            let input = decode_input(&mut cursor)?;
            cursor.finish()?;
            Ok(RemoteMessage::Input { sequence, input })
        }
        3 => Ok(RemoteMessage::Snapshot(decode_snapshot(&mut cursor)?)),
        4 => {
            let width = cursor.u16()?;
            let height = cursor.u16()?;
            let frame_width = cursor.u32()?;
            let frame_height = cursor.u32()?;
            cursor.finish()?;
            Ok(RemoteMessage::Viewport {
                width,
                height,
                frame_width,
                frame_height,
            })
        }
        5 if payload.is_empty() => Ok(RemoteMessage::Quit),
        6 => {
            let revision = cursor.u64()?;
            let normalized_experiment = cursor.experiment()?;
            cursor.finish()?;
            Ok(RemoteMessage::ExperimentState {
                revision,
                normalized_experiment,
            })
        }
        7 => {
            let request_id = cursor.u64()?;
            let base_revision = cursor.u64()?;
            let draft = cursor.experiment()?;
            cursor.finish()?;
            Ok(RemoteMessage::ApplyDraft(ApplyRequest {
                request_id,
                base_revision,
                draft,
            }))
        }
        8 => {
            let request_id = cursor.u64()?;
            let revision = cursor.u64()?;
            let normalized_experiment = cursor.experiment()?;
            cursor.finish()?;
            Ok(RemoteMessage::ApplyAccepted(ApplyAccepted {
                request_id,
                revision,
                normalized_experiment,
            }))
        }
        9 => {
            let request_id = cursor.u64()?;
            let count = cursor.u16()? as usize;
            if count > 4096 {
                return Err(ProtocolError::Invalid("too many diagnostics"));
            }
            let mut diagnostics = Vec::with_capacity(count);
            for _ in 0..count {
                let code = cursor.string()?;
                let message = cursor.string()?;
                let path_count = cursor.u16()? as usize;
                if path_count > 256 {
                    return Err(ProtocolError::Invalid("diagnostic path is too long"));
                }
                let mut path = Vec::with_capacity(path_count);
                for _ in 0..path_count {
                    path.push(cursor.string()?);
                }
                diagnostics.push(Diagnostic {
                    code,
                    message,
                    path: DiagnosticPath(path),
                });
            }
            cursor.finish()?;
            Ok(RemoteMessage::ApplyRejected(ApplyRejected {
                request_id,
                diagnostics,
            }))
        }
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
    payload.extend_from_slice(&snapshot.last_step_ms.to_le_bytes());
    payload.extend_from_slice(&snapshot.average_step_ms.to_le_bytes());
    put_u64(payload, snapshot.step_samples);
    put_u64(payload, snapshot.applied_input_sequence);
    put_string(payload, &snapshot.backend)?;
    put_string(payload, &snapshot.rule)?;
    put_long_string(
        payload,
        &ron::to_string(&snapshot.spec)
            .map_err(|error| ProtocolError::Message(format!("cannot encode rule spec: {error}")))?,
    )?;
    put_long_string(
        payload,
        &ron::to_string(&snapshot.selected_kernel).map_err(|error| {
            ProtocolError::Message(format!("cannot encode selected kernel: {error}"))
        })?,
    )?;
    match &snapshot.selected_parameter {
        Some(parameter) => {
            payload.push(1);
            put_string(payload, parameter)?;
        }
        None => payload.push(0),
    }
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
    let last_step_ms = cursor.f64()?;
    let average_step_ms = cursor.f64()?;
    let step_samples = cursor.u64()?;
    let applied_input_sequence = cursor.u64()?;
    let backend = cursor.string()?;
    let rule = cursor.string()?;
    let spec = ron::from_str(&cursor.long_string()?)
        .map_err(|error| ProtocolError::Message(format!("cannot decode rule spec: {error}")))?;
    let selected_kernel = ron::from_str(&cursor.long_string()?).map_err(|error| {
        ProtocolError::Message(format!("cannot decode selected kernel: {error}"))
    })?;
    let selected_parameter = match cursor.u8()? {
        0 => None,
        1 => Some(cursor.string()?),
        _ => return Err(ProtocolError::Invalid("invalid selected parameter flag")),
    };
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
        last_step_ms,
        average_step_ms,
        step_samples,
        applied_input_sequence,
        backend,
        rule,
        spec,
        selected_kernel,
        selected_parameter,
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

fn put_long_string(payload: &mut Vec<u8>, value: &str) -> Result<(), ProtocolError> {
    let bytes = value.as_bytes();
    let length =
        u32::try_from(bytes.len()).map_err(|_| ProtocolError::Invalid("long string too long"))?;
    if length > MAX_FRAME_SIZE {
        return Err(ProtocolError::Invalid("long string too long"));
    }
    put_u32(payload, length);
    payload.extend_from_slice(bytes);
    Ok(())
}

fn put_experiment(
    payload: &mut Vec<u8>,
    experiment: &crate::sim::experiment_model::ExperimentSpec,
) -> Result<(), ProtocolError> {
    validate_protocol_experiment(experiment)?;
    let encoded = ron::to_string(experiment)
        .map_err(|error| ProtocolError::Message(format!("cannot encode experiment: {error}")))?;
    if encoded.len() > MAX_EXPERIMENT_SIZE {
        return Err(ProtocolError::Invalid(
            "experiment exceeds protocol size limit",
        ));
    }
    put_long_string(payload, &encoded)
}

fn validate_protocol_experiment(
    experiment: &crate::sim::experiment_model::ExperimentSpec,
) -> Result<(), ProtocolError> {
    if experiment.channels.len() > MAX_CHANNELS {
        return Err(ProtocolError::Invalid("too many experiment channels"));
    }
    if experiment.kernels.len() > MAX_RULE_SETS * MAX_KERNELS_PER_RULE_SET {
        return Err(ProtocolError::Invalid("too many legacy kernels"));
    }
    if experiment.growth.len() > MAX_CHANNELS {
        return Err(ProtocolError::Invalid("too many legacy growth programs"));
    }
    if let Some(tiling) = &experiment.tiling {
        if tiling.instances.len() > MAX_BASIS_SITES {
            return Err(ProtocolError::Invalid("too many basis sites"));
        }
        if tiling.prototypes.len() > MAX_BASIS_SITES {
            return Err(ProtocolError::Invalid("too many tile prototypes"));
        }
    }
    if experiment.rules.sets.len() > MAX_RULE_SETS {
        return Err(ProtocolError::Invalid("too many rule sets"));
    }
    if experiment.rules.bindings.len() > MAX_BINDINGS {
        return Err(ProtocolError::Invalid("too many rule bindings"));
    }
    for rule in &experiment.rules.sets {
        if rule.kernels.len() > MAX_KERNELS_PER_RULE_SET {
            return Err(ProtocolError::Invalid("too many kernels in rule set"));
        }
        for kernel in &rule.kernels {
            if let crate::sim::ruleset::KernelSpatialDefinition::Periodic(periodic) =
                &kernel.spatial
                && periodic.planes.len() > MAX_BASIS_SITES
            {
                return Err(ProtocolError::Invalid("too many periodic kernel planes"));
            }
        }
    }
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
        Command::ToggleHelp => 16,
        Command::ToggleWorkbench => 17,
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
        16 => Command::ToggleHelp,
        17 => Command::ToggleWorkbench,
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
    fn long_string(&mut self) -> Result<String, ProtocolError> {
        let length = self.u32()? as usize;
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| ProtocolError::Invalid("invalid utf-8 long string"))
    }
    fn experiment(
        &mut self,
    ) -> Result<crate::sim::experiment_model::ExperimentSpec, ProtocolError> {
        let length = self.u32()? as usize;
        if length > MAX_EXPERIMENT_SIZE {
            return Err(ProtocolError::Invalid(
                "experiment exceeds protocol size limit",
            ));
        }
        let source = std::str::from_utf8(self.take(length)?)
            .map_err(|_| ProtocolError::Invalid("invalid utf-8 experiment"))?;
        let experiment = ron::from_str(source).map_err(|error| {
            ProtocolError::Message(format!("cannot decode experiment: {error}"))
        })?;
        validate_protocol_experiment(&experiment)?;
        Ok(experiment)
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
    use crate::sim::basis_kernel::PeriodicKernelDefinition;
    use crate::sim::experiment_model::{ChannelId, ExperimentSpec, KernelId, KernelSlot};
    use crate::sim::ruleset::{BindingKey, KernelSpatialDefinition};
    use crate::sim::service::{
        ApplyAccepted, ApplyRejected, ApplyRequest, Diagnostic, DiagnosticPath,
    };
    use crate::sim::tiling::{BasisId, PeriodicTilingDraft};
    use std::io::Cursor as IoCursor;

    fn sample_snapshot() -> Snapshot {
        Snapshot {
            width: 2,
            height: 2,
            tick: 7,
            paused: true,
            simulation_rate: 12.5,
            render_rate: 30.0,
            last_step_ms: 1.25,
            average_step_ms: 1.5,
            step_samples: 9,
            applied_input_sequence: 17,
            backend: "CPU".into(),
            rule: "Conway".into(),
            spec: Box::new(SimulationSpec::conway()),
            selected_kernel: Box::new(crate::sim::kernel::KernelDefinition {
                name: "none".into(),
                width: 1,
                height: 1,
                anchor_x: 0,
                anchor_y: 0,
                mask: Some(vec![false]),
                normalization: crate::sim::kernel::Normalization::None,
                parameters: Default::default(),
                values: crate::sim::kernel::KernelValues::Explicit(vec![0.0]),
            }),
            selected_parameter: None,
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
            RemoteMessage::Input {
                sequence: 11,
                input: InputMessage::Command(Command::TogglePause),
            },
            RemoteMessage::Input {
                sequence: 12,
                input: InputMessage::ExpressionKey(ExpressionKey::Char('中')),
            },
            RemoteMessage::Input {
                sequence: 13,
                input: InputMessage::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollUp,
                    column: 3,
                    row: 4,
                    modifiers: KeyModifiers::ALT,
                }),
            },
            RemoteMessage::Input {
                sequence: 14,
                input: InputMessage::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Middle),
                    column: 8,
                    row: 9,
                    modifiers: KeyModifiers::NONE,
                }),
            },
            RemoteMessage::Input {
                sequence: 15,
                input: InputMessage::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Middle),
                    column: 10,
                    row: 9,
                    modifiers: KeyModifiers::NONE,
                }),
            },
            RemoteMessage::Input {
                sequence: 16,
                input: InputMessage::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Middle),
                    column: 10,
                    row: 9,
                    modifiers: KeyModifiers::NONE,
                }),
            },
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
    fn viewport_round_trip_preserves_pixel_frame_size() {
        let message = RemoteMessage::Viewport {
            width: 97,
            height: 41,
            frame_width: 1940,
            frame_height: 1640,
        };
        assert_eq!(roundtrip(message.clone()), message);
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

    fn roundtrip(message: RemoteMessage) -> RemoteMessage {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &message).unwrap();
        read_message(&mut IoCursor::new(bytes)).unwrap().unwrap()
    }

    fn basis_ruleset_fixture() -> ExperimentSpec {
        let mut spec = ExperimentSpec::single_channel_lenia(2, 2);
        let green = spec.add_channel("green", false);
        let blue = spec.add_channel("blue", false);
        assert_eq!((green, blue), (ChannelId(1), ChannelId(2)));
        spec.kernels.push(KernelSlot::identity(
            KernelId(1),
            "detail",
            ChannelId(0),
            ChannelId(0),
        ));
        let growth = spec
            .growth
            .iter_mut()
            .find(|growth| growth.target == ChannelId(0))
            .unwrap();
        growth.kernel_inputs.push(KernelId(1));
        growth.source = "potential + detail".into();
        spec.tiling = Some(
            ron::from_str::<PeriodicTilingDraft>(include_str!(
                "../tests/fixtures/tiling/t_junction.ron"
            ))
            .unwrap(),
        );
        let mut spec = spec.normalize_rules().unwrap();
        let default = spec.rules.defaults[&ChannelId(0)];
        spec.rules.get_mut(default).unwrap().shared_name = Some("shared-red".into());
        let local = spec
            .rules
            .detach(BindingKey {
                basis: BasisId(1),
                output: ChannelId(0),
            })
            .unwrap();
        spec.rules.get_mut(local).unwrap().kernels[0].spatial =
            KernelSpatialDefinition::Periodic(PeriodicKernelDefinition::identity(BasisId(0)));
        spec
    }

    #[test]
    fn basis_ruleset_apply_round_trip() {
        let experiment = basis_ruleset_fixture();
        assert_eq!(experiment.channels.len(), 3);
        assert_eq!(experiment.rules.sets.len(), 4);
        assert_eq!(
            experiment
                .rules
                .get(experiment.rules.defaults[&ChannelId(0)])
                .unwrap()
                .kernels
                .len(),
            2
        );

        for message in [
            RemoteMessage::ApplyDraft(ApplyRequest {
                request_id: 100,
                base_revision: 12,
                draft: experiment.clone(),
            }),
            RemoteMessage::ApplyAccepted(ApplyAccepted {
                request_id: 100,
                revision: 13,
                normalized_experiment: experiment.clone(),
            }),
            RemoteMessage::ExperimentState {
                revision: 13,
                normalized_experiment: experiment.clone(),
            },
        ] {
            assert_eq!(roundtrip(message.clone()), message);
        }
    }

    #[test]
    fn protocol_v8_peer_is_rejected_clearly() {
        let mut frame = Vec::new();
        frame.extend_from_slice(&MAGIC);
        frame.extend_from_slice(&[8, 1]);
        frame.extend_from_slice(&0_u32.to_le_bytes());
        let error = read_message(&mut IoCursor::new(frame)).unwrap_err();
        assert!(error.to_string().contains("version 8"));
        assert!(error.to_string().contains("version 9"));
    }

    #[test]
    fn apply_messages_roundtrip_complete_drafts_and_paths() {
        let message = RemoteMessage::ApplyDraft(ApplyRequest {
            request_id: 44,
            base_revision: 7,
            draft: ExperimentSpec::single_channel_lenia(2, 2),
        });
        assert_eq!(roundtrip(message.clone()), message);

        let accepted = RemoteMessage::ApplyAccepted(ApplyAccepted {
            request_id: 44,
            revision: 8,
            normalized_experiment: ExperimentSpec::single_channel_lenia(2, 2),
        });
        assert_eq!(roundtrip(accepted.clone()), accepted);

        let rejected = RemoteMessage::ApplyRejected(ApplyRejected {
            request_id: 45,
            diagnostics: vec![Diagnostic {
                code: "invalid_experiment".to_string(),
                message: "bad field".to_string(),
                path: DiagnosticPath(vec!["channels".to_string(), "0".to_string()]),
            }],
        });
        assert_eq!(roundtrip(rejected.clone()), rejected);
    }

    #[test]
    fn experiment_state_roundtrips_authoritative_revision() {
        let message = RemoteMessage::ExperimentState {
            revision: 12,
            normalized_experiment: ExperimentSpec::single_channel_lenia(3, 2),
        };
        assert_eq!(roundtrip(message.clone()), message);
    }

    #[test]
    fn oversized_apply_draft_is_rejected_before_allocation() {
        let mut header = Vec::new();
        header.extend_from_slice(&MAGIC);
        header.extend_from_slice(&[PROTOCOL_VERSION, 7]);
        header.extend_from_slice(&(MAX_FRAME_SIZE + 1).to_le_bytes());
        assert!(matches!(
            read_message(&mut IoCursor::new(header)),
            Err(ProtocolError::Invalid(_))
        ));
    }

    #[test]
    fn experiment_collection_limits_are_enforced_before_encoding() {
        let mut experiment = ExperimentSpec::single_channel_lenia(1, 1);
        for index in 1..=MAX_CHANNELS {
            experiment.add_channel(format!("channel-{index}"), false);
        }
        let message = RemoteMessage::ExperimentState {
            revision: 0,
            normalized_experiment: experiment,
        };
        assert!(matches!(
            write_message(&mut Vec::new(), &message),
            Err(ProtocolError::Invalid("too many experiment channels"))
        ));
    }
}
