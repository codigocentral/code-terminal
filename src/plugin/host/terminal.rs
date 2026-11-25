//! Host Functions - Terminal
//!
//! Funções para interação com o terminal (input, output, buffer, cursor).

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};
use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};

static TERMINAL_STATE: LazyLock<Mutex<TerminalHostState>> =
    LazyLock::new(|| Mutex::new(TerminalHostState::default()));

#[derive(Debug, Clone)]
struct TerminalHostState {
    cols: i32,
    rows: i32,
    cursor_col: i32,
    cursor_row: i32,
    buffer: Vec<String>,
    input_queue: VecDeque<Vec<u8>>,
    output: Vec<u8>,
    scroll_offset: i32,
    selection: String,
    last_key_event: Option<(i32, i32, i32)>,
}

impl Default for TerminalHostState {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            cursor_col: 0,
            cursor_row: 0,
            buffer: vec![String::new(); 24],
            input_queue: VecDeque::new(),
            output: Vec::new(),
            scroll_offset: 0,
            selection: String::new(),
            last_key_event: None,
        }
    }
}

fn lock_state() -> Result<std::sync::MutexGuard<'static, TerminalHostState>, HostFunctionError> {
    TERMINAL_STATE.lock().map_err(|_| {
        HostFunctionError::ExecutionError("Failed to lock terminal host state".to_string())
    })
}

fn i32_bytes(value: i32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn i64_bytes(value: i64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn read_i32(params: &[u8], offset: usize) -> Result<i32, HostFunctionError> {
    let bytes: [u8; 4] = params
        .get(offset..offset + 4)
        .ok_or(HostFunctionError::InvalidParams)?
        .try_into()
        .map_err(|_| HostFunctionError::InvalidParams)?;
    Ok(i32::from_le_bytes(bytes))
}

fn ensure_buffer_rows(state: &mut TerminalHostState) {
    let rows = state.rows.max(1) as usize;
    state.buffer.resize(rows, String::new());
}

/// Offset em bytes da coluna `col` (contada em caracteres) em `line`.
fn byte_offset(line: &str, col: usize) -> usize {
    line.char_indices()
        .nth(col)
        .map(|(offset, _)| offset)
        .unwrap_or(line.len())
}

/// Trecho `[start_col, end_col)` de `line`, com colunas em caracteres
/// (fatiar por bytes entrava em pânico no meio de caracteres UTF-8).
fn column_slice(line: &str, start_col: usize, end_col: usize) -> &str {
    if start_col >= end_col {
        return "";
    }
    &line[byte_offset(line, start_col)..byte_offset(line, end_col)]
}

fn write_text_at(state: &mut TerminalHostState, col: i32, row: i32, text: &str) -> usize {
    let row = row.clamp(0, state.rows.max(1) - 1) as usize;
    let col = col.max(0) as usize;
    ensure_buffer_rows(state);

    let line = &mut state.buffer[row];
    let line_cols = line.chars().count();
    if line_cols < col {
        line.push_str(&" ".repeat(col - line_cols));
    }
    if line_cols > col {
        let offset = byte_offset(line, col);
        line.truncate(offset);
    }
    line.push_str(text);
    text.len()
}

pub fn all_functions() -> Vec<Box<dyn HostFunction>> {
    vec![
        Box::new(WriteOutput),
        Box::new(ReadInput),
        Box::new(GetBufferSize),
        Box::new(SetBufferSize),
        Box::new(GetCursorPosition),
        Box::new(SetCursorPosition),
        Box::new(ReadBuffer),
        Box::new(WriteBuffer),
        Box::new(ClearBuffer),
        Box::new(ScrollBuffer),
        Box::new(GetTabId),
        Box::new(GetGridId),
        Box::new(SendKeyEvent),
        Box::new(GetSelection),
        Box::new(SetSelection),
    ]
}

pub struct WriteOutput;

impl HostFunction for WriteOutput {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "write",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }

    fn description(&self) -> &'static str {
        "Escreve dados no terminal (stdout do processo filho)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let mut state = lock_state()?;
        state.output.extend_from_slice(params);
        let text = String::from_utf8_lossy(params);
        let col = state.cursor_col;
        let row = state.cursor_row;
        write_text_at(&mut state, col, row, &text);
        state.cursor_col = (state.cursor_col + text.len() as i32).min(state.cols.saturating_sub(1));
        Ok(i32_bytes(params.len() as i32))
    }
}

pub struct ReadInput;

impl HostFunction for ReadInput {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "read_input",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Lê input pendente do usuário (requer permissão)"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(lock_state()?.input_queue.pop_front().unwrap_or_default())
    }
}

pub struct GetBufferSize;

impl HostFunction for GetBufferSize {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "get_buffer_size",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna dimensões do buffer (cols, rows) via ponteiro"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let state = lock_state()?;
        Ok([state.cols.to_le_bytes(), state.rows.to_le_bytes()].concat())
    }
}

pub struct SetBufferSize;

impl HostFunction for SetBufferSize {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "set_buffer_size",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Define dimensões do buffer (cols, rows)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let cols = read_i32(params, 0)?;
        let rows = read_i32(params, 4)?;
        if cols <= 0 || rows <= 0 {
            return Err(HostFunctionError::InvalidParams);
        }

        let mut state = lock_state()?;
        state.cols = cols;
        state.rows = rows;
        state.cursor_col = state.cursor_col.min(cols - 1);
        state.cursor_row = state.cursor_row.min(rows - 1);
        ensure_buffer_rows(&mut state);
        Ok(i32_bytes(0))
    }
}

pub struct GetCursorPosition;

impl HostFunction for GetCursorPosition {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "get_cursor_position",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna posição do cursor (col, row) via ponteiro"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let state = lock_state()?;
        Ok([
            state.cursor_col.to_le_bytes(),
            state.cursor_row.to_le_bytes(),
        ]
        .concat())
    }
}

pub struct SetCursorPosition;

impl HostFunction for SetCursorPosition {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "set_cursor_position",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Define posição do cursor (col, row)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let col = read_i32(params, 0)?;
        let row = read_i32(params, 4)?;
        let mut state = lock_state()?;
        if col < 0 || row < 0 || col >= state.cols || row >= state.rows {
            return Err(HostFunctionError::InvalidParams);
        }
        state.cursor_col = col;
        state.cursor_row = row;
        Ok(i32_bytes(0))
    }
}

pub struct ReadBuffer;

impl HostFunction for ReadBuffer {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "read_buffer",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::I32,
                HostParamType::I32,
                HostParamType::I32,
                HostParamType::I32,
                HostParamType::Pointer,
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Lê região do buffer (start_col, start_row, end_col, end_row, out_ptr)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let start_col = read_i32(params, 0)?.max(0) as usize;
        let start_row = read_i32(params, 4)?.max(0) as usize;
        let end_col = read_i32(params, 8)?.max(0) as usize;
        let end_row = read_i32(params, 12)?.max(0) as usize;
        let state = lock_state()?;

        let mut output = String::new();
        for row in start_row..=end_row.min(state.buffer.len().saturating_sub(1)) {
            // Separa por linha mesmo quando a anterior é vazia
            if row > start_row {
                output.push('\n');
            }
            output.push_str(column_slice(&state.buffer[row], start_col, end_col));
        }

        Ok(output.into_bytes())
    }
}

pub struct WriteBuffer;

impl HostFunction for WriteBuffer {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "write_buffer",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::I32,
                HostParamType::I32,
                HostParamType::PointerWithLength,
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Escreve na posição do buffer (col, row, data)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 8 {
            return Err(HostFunctionError::InvalidParams);
        }
        let col = read_i32(params, 0)?;
        let row = read_i32(params, 4)?;
        let text = String::from_utf8(params[8..].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let mut state = lock_state()?;
        let written = write_text_at(&mut state, col, row, &text);
        Ok(i32_bytes(written as i32))
    }
}

pub struct ClearBuffer;

impl HostFunction for ClearBuffer {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "clear_buffer",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Limpa buffer (0=screen, 1=scrollback, 2=all)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let mode = if params.is_empty() {
            2
        } else {
            read_i32(params, 0)?
        };
        let mut state = lock_state()?;
        match mode {
            0 | 2 => {
                state.buffer = vec![String::new(); state.rows.max(1) as usize];
                state.cursor_col = 0;
                state.cursor_row = 0;
            }
            1 => {
                state.scroll_offset = 0;
            }
            _ => return Err(HostFunctionError::InvalidParams),
        }
        Ok(i32_bytes(0))
    }
}

pub struct ScrollBuffer;

impl HostFunction for ScrollBuffer {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "scroll_buffer",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Rola scrollback (linhas positivas=baixo, negativas=cima)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let lines = read_i32(params, 0)?;
        let mut state = lock_state()?;
        state.scroll_offset = state.scroll_offset.saturating_add(lines);
        Ok(i32_bytes(state.scroll_offset))
    }
}

pub struct GetTabId;

impl HostFunction for GetTabId {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "get_tab_id",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I64),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna ID da aba atual"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(i64_bytes(1))
    }
}

pub struct GetGridId;

impl HostFunction for GetGridId {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "get_grid_id",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I64),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna ID do grid atual"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(i64_bytes(1))
    }
}

pub struct SendKeyEvent;

impl HostFunction for SendKeyEvent {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "send_key_event",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32, HostParamType::I32, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Envia evento de tecla para o terminal (key_code, modifiers, action)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let key_code = read_i32(params, 0)?;
        let modifiers = read_i32(params, 4)?;
        let action = read_i32(params, 8)?;
        let mut state = lock_state()?;
        state.last_key_event = Some((key_code, modifiers, action));
        Ok(i32_bytes(0))
    }
}

pub struct GetSelection;

impl HostFunction for GetSelection {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "get_selection",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Obtém texto selecionado no terminal"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(lock_state()?.selection.as_bytes().to_vec())
    }
}

pub struct SetSelection;

impl HostFunction for SetSelection {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "set_selection",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::I32,
                HostParamType::I32,
                HostParamType::I32,
                HostParamType::I32,
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Define seleção no terminal (start_col, start_row, end_col, end_row)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let start_col = read_i32(params, 0)?.max(0) as usize;
        let start_row = read_i32(params, 4)?.max(0) as usize;
        let end_col = read_i32(params, 8)?.max(0) as usize;
        let end_row = read_i32(params, 12)?.max(0) as usize;
        let state = lock_state()?;

        let mut selection = String::new();
        for row in start_row..=end_row.min(state.buffer.len().saturating_sub(1)) {
            // Separa por linha mesmo quando a anterior é vazia
            if row > start_row {
                selection.push('\n');
            }
            selection.push_str(column_slice(&state.buffer[row], start_col, end_col));
        }
        drop(state);

        lock_state()?.selection = selection;
        Ok(i32_bytes(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O estado do terminal é global: testes que o alteram rodam em série.
    static STATE_LOCK: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn reset() {
        *lock_state().unwrap() = TerminalHostState::default();
    }

    #[test]
    fn test_all_functions_exist() {
        let functions = all_functions();
        assert!(
            functions.len() >= 15,
            "Deve haver pelo menos 15 funções de terminal"
        );
    }

    #[test]
    fn test_write_output_signature() {
        let func = WriteOutput;
        let sig = func.signature();
        assert_eq!(sig.params.len(), 1);
        assert!(matches!(sig.return_type, Some(HostReturnType::I32)));
    }

    #[test]
    fn test_dangerous_functions() {
        assert_eq!(WriteBuffer.permission(), HostFunctionPermission::Dangerous);
        assert_eq!(SendKeyEvent.permission(), HostFunctionPermission::Dangerous);
    }

    #[test]
    fn test_function_ids() {
        let func = GetCursorPosition;
        assert_eq!(func.id().category, HostFunctionCategory::Terminal);
        assert_eq!(func.id().name, "get_cursor_position");
    }

    #[test]
    fn test_buffer_size_and_cursor_execute() {
        let _guard = serial();
        reset();
        let mut resize = 120i32.to_le_bytes().to_vec();
        resize.extend_from_slice(&40i32.to_le_bytes());
        assert_eq!(
            i32::from_le_bytes(SetBufferSize.execute(&resize).unwrap().try_into().unwrap()),
            0
        );

        let size = GetBufferSize.execute(&[]).unwrap();
        assert_eq!(i32::from_le_bytes(size[0..4].try_into().unwrap()), 120);
        assert_eq!(i32::from_le_bytes(size[4..8].try_into().unwrap()), 40);

        let mut cursor = 5i32.to_le_bytes().to_vec();
        cursor.extend_from_slice(&3i32.to_le_bytes());
        SetCursorPosition.execute(&cursor).unwrap();

        let position = GetCursorPosition.execute(&[]).unwrap();
        assert_eq!(i32::from_le_bytes(position[0..4].try_into().unwrap()), 5);
        assert_eq!(i32::from_le_bytes(position[4..8].try_into().unwrap()), 3);
    }

    #[test]
    fn test_write_read_buffer_and_selection_execute() {
        let _guard = serial();
        reset();
        ClearBuffer.execute(&2i32.to_le_bytes()).unwrap();

        let mut write = 2i32.to_le_bytes().to_vec();
        write.extend_from_slice(&1i32.to_le_bytes());
        write.extend_from_slice(b"hello");
        assert_eq!(
            i32::from_le_bytes(WriteBuffer.execute(&write).unwrap().try_into().unwrap()),
            5
        );

        let mut read = 0i32.to_le_bytes().to_vec();
        read.extend_from_slice(&1i32.to_le_bytes());
        read.extend_from_slice(&10i32.to_le_bytes());
        read.extend_from_slice(&1i32.to_le_bytes());
        let output = ReadBuffer.execute(&read).unwrap();
        assert_eq!(String::from_utf8(output).unwrap().trim(), "hello");

        SetSelection.execute(&read).unwrap();
        let selection = GetSelection.execute(&[]).unwrap();
        assert_eq!(String::from_utf8(selection).unwrap().trim(), "hello");
    }

    #[test]
    fn test_write_output_scroll_and_key_event_execute() {
        let _guard = serial();
        reset();
        ClearBuffer.execute(&2i32.to_le_bytes()).unwrap();

        let written = WriteOutput.execute(b"abc").unwrap();
        assert_eq!(i32::from_le_bytes(written.try_into().unwrap()), 3);

        let offset = ScrollBuffer.execute(&3i32.to_le_bytes()).unwrap();
        assert_eq!(i32::from_le_bytes(offset.try_into().unwrap()), 3);

        let mut key = 13i32.to_le_bytes().to_vec();
        key.extend_from_slice(&1i32.to_le_bytes());
        key.extend_from_slice(&0i32.to_le_bytes());
        assert_eq!(
            i32::from_le_bytes(SendKeyEvent.execute(&key).unwrap().try_into().unwrap()),
            0
        );
    }

    fn params(values: &[i32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn i32_of(bytes: Vec<u8>) -> i32 {
        i32::from_le_bytes(bytes.try_into().unwrap())
    }

    fn read_region(start_col: i32, start_row: i32, end_col: i32, end_row: i32) -> String {
        String::from_utf8(
            ReadBuffer
                .execute(&params(&[start_col, start_row, end_col, end_row]))
                .unwrap(),
        )
        .unwrap()
    }

    fn write_at(col: i32, row: i32, text: &str) -> i32 {
        let mut p = params(&[col, row]);
        p.extend_from_slice(text.as_bytes());
        i32_of(WriteBuffer.execute(&p).unwrap())
    }

    /// Regressão: colunas eram tratadas como bytes e fatiar no meio de um
    /// caractere multibyte derrubava o host com pânico.
    #[test]
    fn test_utf8_columns_do_not_panic() {
        let _guard = serial();
        reset();
        write_at(0, 0, "olá mundo");
        assert_eq!(read_region(0, 0, 3, 0), "olá");
        assert_eq!(read_region(2, 0, 5, 0), "á m");

        SetSelection.execute(&params(&[1, 0, 3, 0])).unwrap();
        assert_eq!(GetSelection.execute(&[]).unwrap(), "lá".as_bytes());

        // Sobrescrever a partir de uma coluna depois de um caractere multibyte
        write_at(3, 0, "X");
        assert_eq!(read_region(0, 0, 10, 0), "oláX");
    }

    #[test]
    fn test_write_buffer_pads_truncates_and_clamps_row() {
        let _guard = serial();
        reset();
        assert_eq!(write_at(4, 0, "ab"), 2);
        assert_eq!(read_region(0, 0, 10, 0), "    ab");

        // Escrever antes do fim trunca o resto da linha
        write_at(1, 0, "Z");
        assert_eq!(read_region(0, 0, 10, 0), " Z");

        // Linha negativa vai para 0; linha além do fim vai para a última
        write_at(0, -5, "topo");
        assert_eq!(read_region(0, 0, 10, 0), "topo");
        write_at(0, 1000, "fim");
        assert_eq!(read_region(0, 23, 10, 23), "fim");

        // Várias linhas são unidas por \n; start >= end produz linha vazia
        write_at(0, 1, "segunda");
        assert_eq!(read_region(0, 0, 3, 1), "top\nseg");
        // Regressão: linhas vazias no começo sumiam (o \n dependia da saída
        // já ter conteúdo)
        assert_eq!(read_region(5, 0, 2, 1), "\n");
        ClearBuffer.execute(&[]).unwrap();
        write_at(0, 1, "abc");
        assert_eq!(read_region(0, 0, 3, 1), "\nabc");

        // Parâmetros inválidos
        assert_eq!(
            WriteBuffer.execute(&[0; 7]),
            Err(HostFunctionError::InvalidParams)
        );
        let mut bad = params(&[0, 0]);
        bad.push(0xff);
        assert_eq!(
            WriteBuffer.execute(&bad),
            Err(HostFunctionError::InvalidParams)
        );
        assert_eq!(
            ReadBuffer.execute(&[0; 12]),
            Err(HostFunctionError::InvalidParams)
        );
    }

    #[test]
    fn test_write_output_advances_cursor_and_records_output() {
        let _guard = serial();
        reset();
        SetCursorPosition.execute(&params(&[2, 1])).unwrap();
        assert_eq!(i32_of(WriteOutput.execute(b"oi").unwrap()), 2);
        assert_eq!(read_region(0, 1, 10, 1), "  oi");
        assert_eq!(GetCursorPosition.execute(&[]).unwrap(), params(&[4, 1]));
        assert_eq!(lock_state().unwrap().output, b"oi");

        // O cursor não passa da última coluna
        WriteOutput.execute(&[b'x'; 200]).unwrap();
        assert_eq!(GetCursorPosition.execute(&[]).unwrap(), params(&[79, 1]));
    }

    #[test]
    fn test_read_input_pops_queue() {
        let _guard = serial();
        reset();
        assert!(ReadInput.execute(&[]).unwrap().is_empty());
        {
            let mut state = lock_state().unwrap();
            state.input_queue.push_back(b"ls\n".to_vec());
            state.input_queue.push_back(b"pwd\n".to_vec());
        }
        assert_eq!(ReadInput.execute(&[]).unwrap(), b"ls\n");
        assert_eq!(ReadInput.execute(&[]).unwrap(), b"pwd\n");
        assert!(ReadInput.execute(&[]).unwrap().is_empty());
    }

    #[test]
    fn test_buffer_size_validation_and_cursor_clamping() {
        let _guard = serial();
        reset();
        SetCursorPosition.execute(&params(&[70, 20])).unwrap();
        SetBufferSize.execute(&params(&[10, 5])).unwrap();
        assert_eq!(GetCursorPosition.execute(&[]).unwrap(), params(&[9, 4]));
        assert_eq!(lock_state().unwrap().buffer.len(), 5);

        for bad in [[0, 5], [10, 0], [-1, 5]] {
            assert_eq!(
                SetBufferSize.execute(&params(&bad)),
                Err(HostFunctionError::InvalidParams)
            );
        }
        assert_eq!(
            SetBufferSize.execute(&[1, 2, 3]),
            Err(HostFunctionError::InvalidParams)
        );

        for bad in [[-1, 0], [0, -1], [10, 0], [0, 5]] {
            assert_eq!(
                SetCursorPosition.execute(&params(&bad)),
                Err(HostFunctionError::InvalidParams),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn test_clear_modes() {
        let _guard = serial();
        reset();
        write_at(0, 0, "abc");
        ScrollBuffer.execute(&params(&[5])).unwrap();

        // Modo 1 só zera o scroll
        ClearBuffer.execute(&params(&[1])).unwrap();
        assert_eq!(lock_state().unwrap().scroll_offset, 0);
        assert_eq!(read_region(0, 0, 10, 0), "abc");

        // Modo 0 limpa a tela e o cursor
        SetCursorPosition.execute(&params(&[3, 3])).unwrap();
        ClearBuffer.execute(&params(&[0])).unwrap();
        assert_eq!(read_region(0, 0, 10, 0), "");
        assert_eq!(GetCursorPosition.execute(&[]).unwrap(), params(&[0, 0]));

        // Sem parâmetro equivale a 2 (tudo)
        write_at(0, 0, "x");
        ClearBuffer.execute(&[]).unwrap();
        assert_eq!(read_region(0, 0, 10, 0), "");

        assert_eq!(
            ClearBuffer.execute(&params(&[9])),
            Err(HostFunctionError::InvalidParams)
        );
        assert_eq!(
            ClearBuffer.execute(&[1, 2]),
            Err(HostFunctionError::InvalidParams)
        );
    }

    #[test]
    fn test_scroll_saturates_and_ids() {
        let _guard = serial();
        reset();
        assert_eq!(i32_of(ScrollBuffer.execute(&params(&[-3])).unwrap()), -3);
        assert_eq!(
            i32_of(ScrollBuffer.execute(&params(&[i32::MIN])).unwrap()),
            i32::MIN
        );
        assert_eq!(
            ScrollBuffer.execute(&[]),
            Err(HostFunctionError::InvalidParams)
        );

        assert_eq!(GetTabId.execute(&[]).unwrap(), 1i64.to_le_bytes());
        assert_eq!(GetGridId.execute(&[]).unwrap(), 1i64.to_le_bytes());
    }

    #[test]
    fn test_send_key_event_records_last_event() {
        let _guard = serial();
        reset();
        SendKeyEvent.execute(&params(&[65, 2, 1])).unwrap();
        assert_eq!(lock_state().unwrap().last_key_event, Some((65, 2, 1)));
        assert_eq!(
            SendKeyEvent.execute(&params(&[65, 2])),
            Err(HostFunctionError::InvalidParams)
        );
    }

    #[test]
    fn test_column_helpers() {
        assert_eq!(byte_offset("aé", 0), 0);
        assert_eq!(byte_offset("aé", 1), 1);
        assert_eq!(byte_offset("aé", 2), 3);
        assert_eq!(byte_offset("aé", 9), 3);
        assert_eq!(column_slice("aéb", 1, 2), "é");
        assert_eq!(column_slice("aéb", 2, 1), "");
        assert_eq!(column_slice("aéb", 1, 99), "éb");
    }
}
