use crate::image::{GraphicsCommand, ImageManager, ImagePlacement};
use crate::terminal::grid::{DynamicColor, Grid, TerminalNotification};
use crate::terminal::modes::CursorShapeMode;
use vte::{Params, Perform};

/// Parser de escape sequences VT100/XTerm usando crate vte
pub struct Parser<'a> {
    grid: &'a mut Grid,
    /// Posição salva do cursor para DECSC/DECRC
    saved_cursor: Option<(usize, usize)>,
    /// Cor foreground atual (SGR)
    current_fg: Color,
    /// Cor background atual (SGR)
    current_bg: Color,
    /// Atributos de estilo atual (SGR)
    current_attrs: Attributes,
    /// Image manager for Kitty graphics protocol
    image_manager: Option<&'a mut ImageManager>,
    /// Buffer for APC/APC sequences (graphics protocol)
    apc_buffer: Vec<u8>,
    /// Flag indicating we're in a graphics sequence
    in_graphics_sequence: bool,
    /// Respostas VT geradas pelo terminal para envio ao PTY.
    pending_response: Vec<u8>,
    /// Máquina de estados do VT (mantida entre chamadas de `process`)
    vte: vte::Parser,
    /// Conjuntos de caracteres, último caractere impresso e SGR salvo.
    text: TextState,
}

/// Conjunto de caracteres designado para G0/G1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Charset {
    #[default]
    Ascii,
    /// DEC Special Graphics: `q` vira `─`, `x` vira `│`... (bordas de tmux, mc, dialog)
    DecSpecialGraphics,
}

impl Charset {
    fn map(self, c: char) -> char {
        if self == Self::Ascii {
            return c;
        }
        match c {
            '`' => '◆',
            'a' => '▒',
            'b' => '␉',
            'c' => '␌',
            'd' => '␍',
            'e' => '␊',
            'f' => '°',
            'g' => '±',
            'h' => '␤',
            'i' => '␋',
            'j' => '┘',
            'k' => '┐',
            'l' => '┌',
            'm' => '└',
            'n' => '┼',
            'o' => '⎺',
            'p' => '⎻',
            'q' => '─',
            'r' => '⎼',
            's' => '⎽',
            't' => '├',
            'u' => '┤',
            'v' => '┴',
            'w' => '┬',
            'x' => '│',
            'y' => '≤',
            'z' => '≥',
            '{' => 'π',
            '|' => '≠',
            '}' => '£',
            '~' => '·',
            other => other,
        }
    }
}

/// Estado de texto que sobrevive entre chunks: charsets, REP e DECSC.
#[derive(Clone, Debug, Default)]
struct TextState {
    charsets: [Charset; 2],
    /// 0 = G0 (SI), 1 = G1 (SO)
    active_charset: usize,
    last_printed: Option<char>,
    saved_style: Option<(Color, Color, Attributes, [Charset; 2], usize)>,
}

/// Estado do parser que precisa sobreviver entre chunks de saída do PTY.
///
/// O PTY entrega a saída em pedaços arbitrários: uma sequência de escape
/// pode chegar partida entre duas leituras, e as cores/atributos definidos
/// por um SGR valem para o texto dos chunks seguintes. Quem cria um
/// `Parser` por chunk deve guardar este estado com [`Parser::into_state`] e
/// retomá-lo com [`Parser::resume`].
#[derive(Default)]
pub struct ParserState {
    vte: vte::Parser,
    saved_cursor: Option<(usize, usize)>,
    current_fg: Color,
    current_bg: Color,
    current_attrs: Attributes,
    apc_buffer: Vec<u8>,
    in_graphics_sequence: bool,
    text: TextState,
}

impl std::fmt::Debug for ParserState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParserState")
            .field("saved_cursor", &self.saved_cursor)
            .field("current_fg", &self.current_fg)
            .field("current_bg", &self.current_bg)
            .field("current_attrs", &self.current_attrs)
            .field("in_graphics_sequence", &self.in_graphics_sequence)
            .finish_non_exhaustive()
    }
}

/// Representação de cor suportando ANSI, 256 cores e True Color
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Color {
    /// Cores ANSI (0-7 normal, 8-15 bright)
    Ansi(u8),
    /// 256 cores (16-255)
    Indexed(u8),
    /// True Color RGB
    Rgb(u8, u8, u8),
    /// Cor default
    #[default]
    Default,
}

/// Atributos de estilo de texto
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attributes {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub reverse: bool,
    pub hidden: bool,
    pub strikethrough: bool,
    pub dim: bool,
}

impl<'a> Parser<'a> {
    /// Cria um novo parser associado a um Grid
    pub fn new(grid: &'a mut Grid) -> Self {
        Self {
            grid,
            saved_cursor: None,
            current_fg: Color::Default,
            current_bg: Color::Default,
            current_attrs: Attributes::default(),
            image_manager: None,
            apc_buffer: Vec::new(),
            in_graphics_sequence: false,
            pending_response: Vec::new(),
            vte: vte::Parser::new(),
            text: TextState::default(),
        }
    }

    /// Cria um novo parser com suporte a imagens
    pub fn with_images(grid: &'a mut Grid, image_manager: &'a mut ImageManager) -> Self {
        Self {
            grid,
            saved_cursor: None,
            current_fg: Color::Default,
            current_bg: Color::Default,
            current_attrs: Attributes::default(),
            image_manager: Some(image_manager),
            apc_buffer: Vec::new(),
            in_graphics_sequence: false,
            pending_response: Vec::new(),
            vte: vte::Parser::new(),
            text: TextState::default(),
        }
    }

    /// Retoma um parser com o estado salvo de um chunk anterior
    pub fn resume(grid: &'a mut Grid, state: ParserState) -> Self {
        Self {
            grid,
            saved_cursor: state.saved_cursor,
            current_fg: state.current_fg,
            current_bg: state.current_bg,
            current_attrs: state.current_attrs,
            image_manager: None,
            apc_buffer: state.apc_buffer,
            in_graphics_sequence: state.in_graphics_sequence,
            pending_response: Vec::new(),
            vte: state.vte,
            text: state.text,
        }
    }

    /// Devolve o estado a ser retomado no próximo chunk
    pub fn into_state(self) -> ParserState {
        ParserState {
            vte: self.vte,
            saved_cursor: self.saved_cursor,
            current_fg: self.current_fg,
            current_bg: self.current_bg,
            current_attrs: self.current_attrs,
            apc_buffer: self.apc_buffer,
            in_graphics_sequence: self.in_graphics_sequence,
            text: self.text,
        }
    }

    /// Processa um buffer de bytes
    ///
    /// A máquina de estados do VT é mantida entre chamadas: uma sequência
    /// partida entre dois `process` é montada corretamente.
    pub fn process(&mut self, data: &[u8]) {
        let mut statemachine = std::mem::take(&mut self.vte);
        statemachine.advance(self, data);
        self.vte = statemachine;
    }

    /// Retorna e limpa respostas VT geradas durante o processamento.
    pub fn take_response(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending_response)
    }

    /// Retorna a cor foreground atual
    pub fn current_fg(&self) -> Color {
        self.current_fg
    }

    /// Retorna a cor background atual
    pub fn current_bg(&self) -> Color {
        self.current_bg
    }

    /// Retorna os atributos atuais
    pub fn current_attrs(&self) -> Attributes {
        self.current_attrs
    }

    /// Processa comando gráfico do Kitty protocol
    fn process_graphics_command(&mut self, command: GraphicsCommand) {
        if let Some(ref mut manager) = self.image_manager {
            match &command {
                GraphicsCommand::Display {
                    image_id,
                    row,
                    col,
                    width,
                    height,
                } => {
                    let placement = ImagePlacement {
                        image_id: *image_id,
                        row: *row,
                        col: *col,
                        width_cells: *width,
                        height_cells: *height,
                        z_index: 0,
                    };
                    self.grid.add_image_placement(placement);
                }
                GraphicsCommand::Delete { image_id } => {
                    self.grid.remove_image_placement(*image_id);
                }
                GraphicsCommand::Clear => {
                    self.grid.clear_image_placements();
                }
                _ => {}
            }

            if let Err(e) = manager.process_command(command) {
                tracing::warn!("Graphics command error: {}", e);
            }
        }
    }
}

/// Converte `file://host/caminho%20com%20espaço` (OSC 7) em caminho local.
fn parse_file_uri(uri: &[u8]) -> Option<std::path::PathBuf> {
    let uri = std::str::from_utf8(uri).ok()?;
    let rest = uri.strip_prefix("file://")?;
    let path = &rest[rest.find('/')?..];
    let mut bytes = Vec::with_capacity(path.len());
    let mut iter = path.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let hex = [iter.next()?, iter.next()?];
            let hex = std::str::from_utf8(&hex).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
        } else {
            bytes.push(b);
        }
    }
    Some(std::path::PathBuf::from(String::from_utf8(bytes).ok()?))
}

/// Cor estendida depois de 38/48: `5;n` (256 cores) ou `2;r;g;b`.
/// Devolve a cor e quantos parâmetros foram consumidos.
fn extended_color(rest: &[usize]) -> Option<(Color, usize)> {
    match rest {
        [5, n, ..] => Some((Color::Indexed(*n as u8), 2)),
        [2, r, g, b, ..] => Some((Color::Rgb(*r as u8, *g as u8, *b as u8), 4)),
        _ => None,
    }
}

/// Aviso de um OSC 9, 777 ou 99. OSC 9;4 (barra de progresso do ConEmu) e
/// mensagens vazias são ignorados.
fn parse_notification(params: &[&[u8]]) -> Option<TerminalNotification> {
    let text = |i: usize| -> String {
        params
            .get(i)
            .map(|p| String::from_utf8_lossy(p).trim().to_string())
            .unwrap_or_default()
    };
    let (title, body) = match params.first().copied()? {
        b"9" => {
            if params.get(1).is_some_and(|p| p == b"4") {
                return None;
            }
            // O texto pode conter ';': junta o resto.
            let body = params[1..]
                .iter()
                .map(|p| String::from_utf8_lossy(p))
                .collect::<Vec<_>>()
                .join(";");
            (String::new(), body.trim().to_string())
        }
        b"777" if params.get(1).is_some_and(|p| p == b"notify") => (text(2), text(3)),
        b"99" => {
            // Metadados `chave=valor:...`; p=body indica corpo, o padrão é título.
            let meta = text(1);
            let is_body = meta.split(':').any(|kv| kv == "p=body");
            let payload = text(2);
            if is_body {
                (String::new(), payload)
            } else {
                (payload, String::new())
            }
        }
        _ => return None,
    };
    if title.is_empty() && body.is_empty() {
        return None;
    }
    Some(TerminalNotification { title, body })
}

/// `CSI s` chega do vte como um único parâmetro 0.
fn no_params(params: &[usize]) -> bool {
    params.iter().all(|&p| p == 0) && params.len() <= 1
}

/// Parâmetro de contagem (CUU, CUD, SU, IL...): ausente ou 0 vale 1.
///
/// O vte entrega `ESC [ A` com um parâmetro 0, então `unwrap_or(1)` sozinho
/// movia o cursor 0 posições para a seta "pra cima" mais comum.
fn count_param(params: &[usize]) -> usize {
    params.first().copied().filter(|&n| n > 0).unwrap_or(1)
}

impl Perform for Parser<'_> {
    /// Imprime um caractere na posição atual do cursor
    fn print(&mut self, c: char) {
        let c = self.text.charsets[self.text.active_charset].map(c);
        self.text.last_printed = Some(c);
        self.grid
            .write(c, self.current_fg, self.current_bg, self.current_attrs);
    }

    /// Executa um comando C0/C1 control
    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => self.grid.ring_bell(), // BEL - Bell
            0x08 => self.grid.backspace(), // BS - Backspace
            0x09 => self.grid.tab(),       // HT - Horizontal Tab
            // LF/VT/FF descem mantendo a coluna (o PTY já troca \n por \r\n);
            // com LNM ligado também voltam à coluna 0.
            0x0A..=0x0C => {
                if self.grid.modes().linefeed_newline {
                    self.grid.newline();
                } else {
                    self.grid.index();
                }
            }
            0x0D => self.grid.carriage_return(), // CR - Carriage Return
            0x0E => self.text.active_charset = 1, // SO - usa G1
            0x0F => self.text.active_charset = 0, // SI - usa G0
            0x84 => self.grid.index(),           // IND - Index
            0x85 => {
                // NEL - Next Line
                self.grid.carriage_return();
                self.grid.newline();
            }
            _ => {
                tracing::trace!("Unhandled control byte: 0x{:02X}", byte);
            }
        }
    }

    /// Hook para sequences DCS (Device Control String)
    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        // Check for Kitty Graphics Protocol: ESC _ G (APC with 'G')
        // intermediates == [0x5f] means '_', action == 'G' means graphics
        if intermediates == b"_" && action == 'G' {
            self.in_graphics_sequence = true;
            self.apc_buffer.clear();
            tracing::trace!("Kitty graphics sequence started");
            return;
        }

        tracing::trace!(
            "DCS hook: params={:?}, intermediates={:?}, ignore={}, action={}",
            params,
            intermediates,
            ignore,
            action
        );
    }

    /// Put data para DCS/APC
    fn put(&mut self, byte: u8) {
        if self.in_graphics_sequence {
            self.apc_buffer.push(byte);
        } else {
            tracing::trace!("DCS put: 0x{:02X}", byte);
        }
    }

    /// Unhook DCS/APC
    fn unhook(&mut self) {
        if self.in_graphics_sequence {
            self.in_graphics_sequence = false;

            // Process the graphics sequence
            if let Some(ref mut manager) = self.image_manager {
                match manager.protocol_handler.parse(&self.apc_buffer) {
                    Ok(Some(command)) => {
                        tracing::debug!("Graphics command parsed: {:?}", command);
                        self.process_graphics_command(command);
                    }
                    Ok(None) => {
                        // Stream mode: waiting for more data
                        tracing::trace!("Graphics command waiting for more data");
                    }
                    Err(e) => {
                        tracing::warn!("Graphics protocol parse error: {}", e);
                    }
                }
            }

            self.apc_buffer.clear();
            tracing::trace!("Kitty graphics sequence ended");
        } else {
            tracing::trace!("DCS unhook");
        }
    }

    /// OSC (Operating System Command) dispatch
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        tracing::trace!(
            "OSC: params={:?}, bell_terminated={}",
            params,
            bell_terminated
        );

        if params.is_empty() {
            return;
        }

        let osc_type = params[0];
        match osc_type {
            b"0" | b"2" => {
                // Título da janela/tab
                let title = params[1..]
                    .iter()
                    .map(|part| String::from_utf8_lossy(part))
                    .collect::<Vec<_>>()
                    .join(";");
                self.grid.set_title(&title);
            }
            b"7" => {
                // Diretório atual do shell: file://host/caminho
                if let Some(dir) = params.get(1).and_then(|uri| parse_file_uri(uri)) {
                    self.grid.set_working_dir(dir);
                }
            }
            b"8" => {
                // Hyperlink (OSC 8)
                tracing::trace!("Hyperlink OSC 8");
            }
            // Imagem inline do iTerm2: 1337;File=...;inline=1:BASE64
            b"1337" => {
                let payload = params[1..].join(&b';');
                self.place_iterm2_image(&payload);
            }
            // Avisos para o usuário: OSC 9 (iTerm2), OSC 777 (urxvt/foot) e
            // OSC 99 (kitty). Claude Code e outros agentes usam um destes.
            b"9" | b"777" | b"99" => {
                if let Some(notification) = parse_notification(params) {
                    self.grid.push_notification(notification);
                }
            }
            b"10" | b"11" | b"12" if params.get(1) == Some(&&b"?"[..]) => {
                // Consulta de cor (nvim, bat e delta usam para saber se o fundo
                // é claro ou escuro). Respondemos com a paleta configurada.
                let which = match osc_type {
                    b"10" => DynamicColor::Foreground,
                    b"11" => DynamicColor::Background,
                    _ => DynamicColor::Cursor,
                };
                let rgb = self.grid.dynamic_color(which);
                let terminator = if bell_terminated { "\x07" } else { "\x1b\\" };
                let reply = format!(
                    "\x1b]{};rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}{}",
                    String::from_utf8_lossy(osc_type),
                    rgb.0,
                    rgb.0,
                    rgb.1,
                    rgb.1,
                    rgb.2,
                    rgb.2,
                    terminator
                );
                self.pending_response.extend_from_slice(reply.as_bytes());
            }
            _ => {
                tracing::trace!("Unhandled OSC: {:?}", osc_type);
            }
        }
    }

    /// CSI (Control Sequence Introducer) dispatch
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }

        // Converte params para slice de usize para facilitar
        let params: Vec<usize> = params.iter().map(|p| p[0] as usize).collect();

        match intermediates {
            [] => self.csi_plain(&params, action),
            [b'?'] => self.csi_private(&params, action),
            [b'>'] => self.csi_secondary(&params, action),
            [b' '] if action == 'q' => {
                // DECSCUSR - forma do cursor (nvim: barra no modo de inserção)
                let param = params.first().copied().unwrap_or(0);
                self.grid.modes_mut().cursor_shape = CursorShapeMode::from_param(param);
            }
            [b'!'] if action == 'p' => self.soft_reset(), // DECSTR
            _ => {
                tracing::trace!(
                    "Unhandled CSI: action={}, params={:?}, intermediates={:?}",
                    action,
                    params,
                    intermediates
                );
            }
        }
    }

    /// ESC (Escape) dispatch
    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore {
            return;
        }

        match (intermediates, byte) {
            // Save/Restore Cursor
            (&[], b'7') => self.handle_decsc(), // DECSC - Save Cursor
            (&[], b'8') => self.handle_decrc(), // DECRC - Restore Cursor

            // Indexing
            (&[], b'D') => self.grid.index(),         // IND - Index
            (&[], b'E') => self.grid.newline(),       // NEL - Next Line
            (&[], b'M') => self.grid.reverse_index(), // RI - Reverse Index

            (&[], b'c') => self.full_reset(), // RIS - Reset to Initial State
            (&[], b'=') => self.grid.modes_mut().application_keypad = true, // DECKPAM
            (&[], b'>') => self.grid.modes_mut().application_keypad = false, // DECKPNM

            // Designação de charset: ESC ( 0 = desenho de linhas em G0, ESC ( B = ASCII
            (&[b'('], b'0') => self.text.charsets[0] = Charset::DecSpecialGraphics,
            (&[b'('], _) => self.text.charsets[0] = Charset::Ascii,
            (&[b')'], b'0') => self.text.charsets[1] = Charset::DecSpecialGraphics,
            (&[b')'], _) => self.text.charsets[1] = Charset::Ascii,

            // Nota: Set/Reset Mode são tratados em csi_dispatch, não aqui
            _ => {
                tracing::trace!(
                    "Unhandled ESC: byte=0x{:02X}, intermediates={:?}",
                    byte,
                    intermediates
                );
            }
        }
    }
}

// Implementação dos handlers CSI
impl Parser<'_> {
    /// CSI sem intermediários.
    fn csi_plain(&mut self, params: &[usize], action: char) {
        let n = count_param(params);
        match action {
            // Cursor Positioning
            'H' | 'f' => self.handle_cup(params), // CUP - Cursor Position
            'A' => self.handle_cuu(params),       // CUU - Cursor Up
            'B' | 'e' => self.handle_cud(params), // CUD / VPR
            'C' | 'a' => self.handle_cuf(params), // CUF / HPR
            'D' => self.handle_cub(params),       // CUB - Cursor Back
            'E' => self.handle_cnl(params),       // CNL - Cursor Next Line
            'F' => self.handle_cpl(params),       // CPL - Cursor Previous Line
            'G' | '`' => self.handle_cha(params), // CHA / HPA
            'd' => self.handle_vpa(params),       // VPA - Vertical Position Absolute
            'I' => (0..n).for_each(|_| self.grid.tab()), // CHT - avança tabs
            'Z' => self.grid.back_tab(n),         // CBT - volta tabs

            // Erase / edição de caracteres
            'J' => self.handle_ed(params),    // ED - Erase in Display
            'K' => self.handle_el(params),    // EL - Erase in Line
            '@' => self.grid.insert_chars(n), // ICH
            'P' => self.grid.delete_chars(n), // DCH (readline apaga no meio da linha assim)
            'X' => self.grid.erase_chars(n),  // ECH
            'b' => self.repeat_last(n),       // REP

            // SGR - Select Graphic Rendition
            'm' => self.handle_sgr(params),

            // Scrolling
            'S' => self.handle_su(params), // SU - Scroll Up
            'T' => self.handle_sd(params), // SD - Scroll Down

            // Scroll region
            'r' => self.handle_decstbm(params), // DECSTBM - Set Top/Bottom Margins

            // Insert/Delete lines
            'L' => self.handle_il(params), // IL - Insert Line
            'M' => self.handle_dl(params), // DL - Delete Line

            // Tab stops
            'g' => self.handle_tbc(params), // TBC - Tab Clear

            // Device Status / atributos
            'n' => self.handle_dsr(params), // DSR - Device Status Report
            'c' if params.first().copied().unwrap_or(0) == 0 => {
                // DA1: VT220 com cores ANSI. vim e fish esperam esta resposta.
                self.pending_response.extend_from_slice(b"\x1b[?62;22c");
            }
            't' => self.handle_window_report(params),

            // Salvar/restaurar cursor (SCOSC/SCORC)
            's' if no_params(params) => self.handle_decsc(),
            'u' if no_params(params) => self.handle_decrc(),

            // Modes (sem intermediates)
            'h' => self.handle_set_mode(params, &[]), // SM - Set Mode
            'l' => self.handle_reset_mode(params, &[]), // RM - Reset Mode

            _ => {
                tracing::trace!("Unhandled CSI: action={}, params={:?}", action, params);
            }
        }
    }

    /// CSI com `?` (modos privados do DEC).
    fn csi_private(&mut self, params: &[usize], action: char) {
        match action {
            'h' => self.handle_set_mode(params, b"?"),
            'l' => self.handle_reset_mode(params, b"?"),
            // Apagamento seletivo: tratamos como o normal.
            'J' => self.handle_ed(params),
            'K' => self.handle_el(params),
            'n' if params.first() == Some(&6) => {
                // DECXCPR - posição do cursor
                let (row, col) = self.grid.cursor_position();
                self.pending_response
                    .extend_from_slice(format!("\x1b[?{};{}R", row + 1, col + 1).as_bytes());
            }
            _ => {
                // Ex.: CSI ? u (consulta do protocolo de teclado do kitty): sem
                // resposta, o programa usa o modo tradicional.
                tracing::trace!(
                    "Unhandled private CSI: action={}, params={:?}",
                    action,
                    params
                );
            }
        }
    }

    /// CSI com `>` (consultas e opções do xterm).
    fn csi_secondary(&mut self, params: &[usize], action: char) {
        match action {
            'c' => {
                // DA2: tipo 0, versão 10, sem ROM.
                self.pending_response.extend_from_slice(b"\x1b[>0;10;1c");
            }
            'q' => {
                // XTVERSION
                let reply = format!("\x1bP>|code-terminal({})\x1b\\", env!("CARGO_PKG_VERSION"));
                self.pending_response.extend_from_slice(reply.as_bytes());
            }
            _ => {
                // Ex.: CSI > 4 ; 2 m (modifyOtherKeys do vim) não é SGR.
                tracing::trace!(
                    "Unhandled secondary CSI: action={}, params={:?}",
                    action,
                    params
                );
            }
        }
    }

    /// Põe uma imagem do iTerm2 (`OSC 1337 File=…:BASE64`) no cursor.
    fn place_iterm2_image(&mut self, payload: &[u8]) {
        use crate::terminal::images::{cells_for, parse_iterm2};
        let Some(request) = parse_iterm2(payload) else {
            tracing::debug!("Ignored OSC 1337 (not an inline image)");
            return;
        };
        let (cols, rows) = cells_for(
            request.pixels.dimensions(),
            request.width,
            request.height,
            request.preserve_aspect,
            self.grid.cell_pixel_size(),
            self.grid.size(),
        );
        self.grid.place_image(request.pixels, cols, rows);
    }

    /// REP - repete o último caractere impresso.
    fn repeat_last(&mut self, n: usize) {
        if let Some(c) = self.text.last_printed {
            for _ in 0..n.min(u16::MAX as usize) {
                self.grid
                    .write(c, self.current_fg, self.current_bg, self.current_attrs);
            }
        }
    }

    /// XTWINOPS: só os relatórios de tamanho (18 = linhas e colunas).
    fn handle_window_report(&mut self, params: &[usize]) {
        if params.first() == Some(&18) {
            let (cols, rows) = self.grid.size();
            self.pending_response
                .extend_from_slice(format!("\x1b[8;{};{}t", rows, cols).as_bytes());
        }
    }

    /// DECSTR - reset leve: modos e atributos, sem apagar a tela.
    fn soft_reset(&mut self) {
        self.current_fg = Color::Default;
        self.current_bg = Color::Default;
        self.current_attrs = Attributes::default();
        self.grid.set_erase_background(Color::Default);
        self.text = TextState::default();
        let modes = self.grid.modes_mut();
        modes.show_cursor = true;
        modes.insert_mode = false;
        modes.origin_mode = false;
        modes.auto_wrap = true;
        modes.cursor_keys = crate::terminal::modes::CursorKeysMode::Normal;
        modes.application_keypad = false;
        modes.cursor_shape = None;
        self.grid.reset_scroll_region();
    }

    /// RIS - reset completo.
    fn full_reset(&mut self) {
        self.current_fg = Color::Default;
        self.current_bg = Color::Default;
        self.current_attrs = Attributes::default();
        self.saved_cursor = None;
        self.text = TextState::default();
        self.grid.full_reset();
    }

    /// CUP - Cursor Position [row;col]
    fn handle_cup(&mut self, params: &[usize]) {
        let row = params.first().copied().unwrap_or(1).saturating_sub(1);
        let col = params.get(1).copied().unwrap_or(1).saturating_sub(1);
        self.grid.move_cursor_to(row, col);
    }

    /// CUU - Cursor Up
    fn handle_cuu(&mut self, params: &[usize]) {
        let n = count_param(params);
        self.grid.move_cursor_up(n);
    }

    /// CUD - Cursor Down
    fn handle_cud(&mut self, params: &[usize]) {
        let n = count_param(params);
        self.grid.move_cursor_down(n);
    }

    /// CUF - Cursor Forward
    fn handle_cuf(&mut self, params: &[usize]) {
        let n = count_param(params);
        self.grid.move_cursor_forward(n);
    }

    /// CUB - Cursor Back
    fn handle_cub(&mut self, params: &[usize]) {
        let n = count_param(params);
        self.grid.move_cursor_back(n);
    }

    /// CNL - Cursor Next Line
    fn handle_cnl(&mut self, params: &[usize]) {
        let n = count_param(params);
        for _ in 0..n {
            self.grid.carriage_return();
            self.grid.newline();
        }
    }

    /// CPL - Cursor Previous Line
    fn handle_cpl(&mut self, params: &[usize]) {
        let n = count_param(params);
        self.grid.move_cursor_up(n);
        self.grid.carriage_return();
    }

    /// CHA - Cursor Horizontal Absolute
    fn handle_cha(&mut self, params: &[usize]) {
        let col = params.first().copied().unwrap_or(1).saturating_sub(1);
        // cursor_position() devolve (row, col)
        let (row, _) = self.grid.cursor_position();
        self.grid.move_cursor_to(row, col);
    }

    /// VPA - Vertical Position Absolute
    fn handle_vpa(&mut self, params: &[usize]) {
        let row = params.first().copied().unwrap_or(1).saturating_sub(1);
        let (_, col) = self.grid.cursor_position();
        self.grid.move_cursor_to(row, col);
    }

    /// ED - Erase in Display
    fn handle_ed(&mut self, params: &[usize]) {
        let mode = params.first().copied().unwrap_or(0);
        match mode {
            0 => self.grid.erase_from_cursor_to_end(),
            1 => self.grid.erase_from_start_to_cursor(),
            2 => self.grid.erase_all(),
            3 => self.grid.erase_scrollback(),
            _ => {}
        }
    }

    /// EL - Erase in Line
    fn handle_el(&mut self, params: &[usize]) {
        let mode = params.first().copied().unwrap_or(0);
        match mode {
            0 => self.grid.erase_line_from_cursor(),
            1 => self.grid.erase_line_to_cursor(),
            2 => self.grid.erase_line_all(),
            _ => {}
        }
    }

    /// SGR - Select Graphic Rendition
    fn handle_sgr(&mut self, params: &[usize]) {
        self.apply_sgr(params);
        // BCE: apagar/rolar usa o fundo atual.
        self.grid.set_erase_background(self.current_bg);
    }

    fn apply_sgr(&mut self, params: &[usize]) {
        if params.is_empty() {
            self.reset_sgr();
            return;
        }

        let mut i = 0;
        while i < params.len() {
            match params[i] {
                // Cor estendida: 38/48 ; 5 ; n  ou  38/48 ; 2 ; r ; g ; b
                code @ (38 | 48) => {
                    if let Some((color, used)) = extended_color(&params[i + 1..]) {
                        if code == 38 {
                            self.current_fg = color;
                        } else {
                            self.current_bg = color;
                        }
                        i += used;
                    }
                }
                code => self.apply_sgr_code(code),
            }
            i += 1;
        }
    }

    fn reset_sgr(&mut self) {
        self.current_fg = Color::Default;
        self.current_bg = Color::Default;
        self.current_attrs = Attributes::default();
    }

    /// Um código SGR simples (atributo ou cor da paleta).
    fn apply_sgr_code(&mut self, code: usize) {
        let attrs = &mut self.current_attrs;
        match code {
            0 => self.reset_sgr(),
            1 => attrs.bold = true,
            2 => attrs.dim = true,
            3 => attrs.italic = true,
            4 => attrs.underline = true,
            5 => attrs.blink = true,
            7 => attrs.reverse = true,
            8 => attrs.hidden = true,
            9 => attrs.strikethrough = true,
            22 => {
                attrs.bold = false;
                attrs.dim = false;
            }
            23 => attrs.italic = false,
            24 => attrs.underline = false,
            25 => attrs.blink = false,
            27 => attrs.reverse = false,
            28 => attrs.hidden = false,
            29 => attrs.strikethrough = false,
            30..=37 => self.current_fg = Color::Ansi((code - 30) as u8),
            39 => self.current_fg = Color::Default,
            40..=47 => self.current_bg = Color::Ansi((code - 40) as u8),
            49 => self.current_bg = Color::Default,
            90..=97 => self.current_fg = Color::Ansi((code - 90 + 8) as u8),
            100..=107 => self.current_bg = Color::Ansi((code - 100 + 8) as u8),
            _ => tracing::trace!("Unhandled SGR: {}", code),
        }
    }

    /// SU - Scroll Up
    fn handle_su(&mut self, params: &[usize]) {
        let n = count_param(params);
        for _ in 0..n {
            self.grid.scroll_up_one();
        }
    }

    /// SD - Scroll Down
    fn handle_sd(&mut self, params: &[usize]) {
        let n = count_param(params);
        for _ in 0..n {
            self.grid.scroll_down_one();
        }
    }

    /// TBC - Tab Clear
    fn handle_tbc(&mut self, params: &[usize]) {
        let mode = params.first().copied().unwrap_or(0);
        match mode {
            0 => self.grid.clear_tab_stop(),
            3 => self.grid.clear_all_tab_stops(),
            _ => {}
        }
    }

    /// DSR - Device Status Report
    fn handle_dsr(&mut self, params: &[usize]) {
        let mode = params.first().copied().unwrap_or(0);
        match mode {
            5 => {
                // Device status report - respond with OK
                tracing::trace!("DSR 5 - Device OK");
                self.pending_response.extend_from_slice(b"\x1b[0n");
            }
            6 => {
                // Cursor position report
                let (row, col) = self.grid.cursor_position();
                tracing::trace!("DSR 6 - Cursor at row={}, col={}", row, col);
                self.pending_response
                    .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            }
            _ => {}
        }
    }

    /// DECSC - Save Cursor
    fn handle_decsc(&mut self) {
        self.saved_cursor = Some(self.grid.cursor_position());
        self.text.saved_style = Some((
            self.current_fg,
            self.current_bg,
            self.current_attrs,
            self.text.charsets,
            self.text.active_charset,
        ));
    }

    /// DECRC - Restore Cursor
    fn handle_decrc(&mut self) {
        if let Some((row, col)) = self.saved_cursor {
            self.grid.move_cursor_to(row, col);
        }
        if let Some((fg, bg, attrs, charsets, active)) = self.text.saved_style {
            self.current_fg = fg;
            self.current_bg = bg;
            self.current_attrs = attrs;
            self.text.charsets = charsets;
            self.text.active_charset = active;
            self.grid.set_erase_background(bg);
        }
    }

    /// SM - Set Mode (ESC [ ? n h)
    fn handle_set_mode(&mut self, params: &[usize], intermediates: &[u8]) {
        // Private mode sequences use '?' intermediate
        let is_private = intermediates == b"?";

        if is_private {
            for &mode in params {
                match mode {
                    1 => {
                        // DECCKM - Cursor Keys Mode (application)
                        self.grid.modes_mut().cursor_keys =
                            crate::terminal::modes::CursorKeysMode::Application;
                        tracing::trace!("DECCKM set - Application mode");
                    }
                    3 => {
                        // DECCOLM - 132 Column Mode
                        self.grid.modes_mut().columns =
                            crate::terminal::modes::ColumnMode::Columns132;
                        let (_, rows) = self.grid.size();
                        self.grid.resize(132, rows);
                        self.grid.erase_all();
                        self.grid.move_cursor_to(0, 0);
                        tracing::trace!("DECCOLM set - 132 columns");
                    }
                    6 => {
                        // DECOM - Origin Mode
                        self.grid.modes_mut().origin_mode = true;
                        tracing::trace!("DECOM set - Origin mode");
                    }
                    7 => {
                        // DECAWM - Auto Wrap Mode
                        self.grid.modes_mut().auto_wrap = true;
                        tracing::trace!("DECAWM set - Auto wrap enabled");
                    }
                    12 => {
                        // Start Blinking Cursor
                        tracing::trace!("Start blinking cursor");
                    }
                    25 => {
                        // DECTCEM - Show Cursor
                        self.grid.modes_mut().show_cursor = true;
                        tracing::trace!("DECTCEM set - Cursor visible");
                    }
                    47 => {
                        // 47 - Use Alternate Screen Buffer
                        self.grid.enable_alternate_buffer();
                        tracing::trace!("Alternate buffer enabled (47)");
                    }
                    1000 => {
                        // Mouse tracking - X11
                        self.grid.modes_mut().mouse_mode = crate::terminal::modes::MouseMode::VT200;
                        tracing::trace!("Mouse mode set - X11");
                    }
                    1002 => {
                        // Mouse tracking - button event
                        self.grid.modes_mut().mouse_mode =
                            crate::terminal::modes::MouseMode::ButtonEvent;
                        tracing::trace!("Mouse mode set - Button event");
                    }
                    1006 => self.grid.modes_mut().sgr_mouse = true,
                    1004 => self.grid.modes_mut().focus_events = true,
                    9 => {
                        self.grid.modes_mut().mouse_mode = crate::terminal::modes::MouseMode::X10;
                    }
                    1003 => {
                        // Mouse tracking - qualquer movimento
                        self.grid.modes_mut().mouse_mode =
                            crate::terminal::modes::MouseMode::AnyEvent;
                    }
                    1047 => {
                        // 1047 - Use Alternate Screen Buffer
                        self.grid.enable_alternate_buffer();
                        tracing::trace!("Alternate buffer enabled (1047)");
                    }
                    1048 => {
                        // 1048 - Save Cursor
                        self.handle_decsc();
                        tracing::trace!("Cursor saved (1048)");
                    }
                    1049 => {
                        // 1049 - Save cursor and use Alternate Screen Buffer
                        self.handle_decsc();
                        self.grid.enable_alternate_buffer();
                        self.grid.erase_all();
                        tracing::trace!("Alternate buffer enabled with clear (1049)");
                    }
                    2004 => {
                        // Bracketed paste mode
                        self.grid.modes_mut().bracketed_paste = true;
                        tracing::trace!("Bracketed paste enabled");
                    }
                    _ => {
                        tracing::trace!("Unhandled set mode: {}", mode);
                    }
                }
            }
        } else {
            // Standard modes (sem ?)
            for &mode in params {
                match mode {
                    4 => {
                        // IRM - Insert Mode
                        self.grid.modes_mut().insert_mode = true;
                        tracing::trace!("Insert mode enabled");
                    }
                    20 => {
                        // LNM - Line Feed/New Line Mode
                        self.grid.modes_mut().linefeed_newline = true;
                    }
                    _ => {
                        tracing::trace!("Unhandled standard set mode: {}", mode);
                    }
                }
            }
        }
    }

    /// RM - Reset Mode (ESC [ ? n l)
    fn handle_reset_mode(&mut self, params: &[usize], intermediates: &[u8]) {
        // Private mode sequences use '?' intermediate
        let is_private = intermediates == b"?";

        if is_private {
            for &mode in params {
                match mode {
                    1 => {
                        // DECCKM - Cursor Keys Mode (normal)
                        self.grid.modes_mut().cursor_keys =
                            crate::terminal::modes::CursorKeysMode::Normal;
                        tracing::trace!("DECCKM reset - Normal mode");
                    }
                    3 => {
                        // DECCOLM - 80 Column Mode
                        self.grid.modes_mut().columns =
                            crate::terminal::modes::ColumnMode::Columns80;
                        let (_, rows) = self.grid.size();
                        self.grid.resize(80, rows);
                        self.grid.erase_all(); // DECCOLM also clears screen
                        self.grid.move_cursor_to(0, 0);
                        tracing::trace!("DECCOLM reset - 80 columns");
                    }
                    6 => {
                        // DECOM - Origin Mode
                        self.grid.modes_mut().origin_mode = false;
                        tracing::trace!("DECOM reset - Origin mode");
                    }
                    7 => {
                        // DECAWM - Auto Wrap Mode
                        self.grid.modes_mut().auto_wrap = false;
                        tracing::trace!("DECAWM reset - Auto wrap disabled");
                    }
                    12 => {
                        // Stop Blinking Cursor
                        tracing::trace!("Stop blinking cursor");
                    }
                    25 => {
                        // DECTCEM - Hide Cursor
                        self.grid.modes_mut().show_cursor = false;
                        tracing::trace!("DECTCEM reset - Cursor hidden");
                    }
                    47 => {
                        // 47 - Use Normal Screen Buffer
                        self.grid.disable_alternate_buffer();
                        tracing::trace!("Alternate buffer disabled (47)");
                    }
                    9 | 1000 | 1002 | 1003 => {
                        // Disable mouse tracking
                        self.grid.modes_mut().mouse_mode = crate::terminal::modes::MouseMode::None;
                        tracing::trace!("Mouse mode disabled");
                    }
                    1006 => self.grid.modes_mut().sgr_mouse = false,
                    1004 => self.grid.modes_mut().focus_events = false,
                    1047 => {
                        // 1047 - Use Normal Screen Buffer
                        self.grid.disable_alternate_buffer();
                        tracing::trace!("Alternate buffer disabled (1047)");
                    }
                    1048 => {
                        // 1048 - Restore Cursor
                        self.handle_decrc();
                        tracing::trace!("Cursor restored (1048)");
                    }
                    1049 => {
                        // 1049 - Use Normal Screen Buffer and restore cursor
                        self.grid.disable_alternate_buffer();
                        self.handle_decrc();
                        tracing::trace!("Alternate buffer disabled with restore (1049)");
                    }
                    2004 => {
                        // Bracketed paste mode
                        self.grid.modes_mut().bracketed_paste = false;
                        tracing::trace!("Bracketed paste disabled");
                    }
                    _ => {
                        tracing::trace!("Unhandled reset mode: {}", mode);
                    }
                }
            }
        } else {
            // Standard modes (sem ?)
            for &mode in params {
                match mode {
                    4 => {
                        // IRM - Replace Mode
                        self.grid.modes_mut().insert_mode = false;
                        tracing::trace!("Insert mode disabled");
                    }
                    20 => {
                        // LNM - Line Feed Mode
                        self.grid.modes_mut().linefeed_newline = false;
                    }
                    _ => {
                        tracing::trace!("Unhandled standard reset mode: {}", mode);
                    }
                }
            }
        }
    }

    /// DECSTBM - Set Top and Bottom Margins (ESC [ top ; bottom r)
    fn handle_decstbm(&mut self, params: &[usize]) {
        let top = params.first().copied().unwrap_or(0);
        let bottom = params.get(1).copied().unwrap_or(0);

        if top == 0 && bottom == 0 {
            // Reset scroll region
            self.grid.reset_scroll_region();
        } else {
            self.grid.set_scroll_region(top, bottom);
        }
    }

    /// IL - Insert Line (ESC [ n L)
    fn handle_il(&mut self, params: &[usize]) {
        let n = count_param(params);
        self.grid.insert_lines(n);
    }

    /// DL - Delete Line (ESC [ n M)
    fn handle_dl(&mut self, params: &[usize]) {
        let n = count_param(params);
        self.grid.delete_lines(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_creation() {
        let mut grid = Grid::new(80, 24);
        let _parser = Parser::new(&mut grid);
    }

    #[test]
    fn test_simple_text() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"Hello");
        }

        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'H');
        assert_eq!(cells[0][1].char, 'e');
        assert_eq!(cells[0][2].char, 'l');
        assert_eq!(cells[0][3].char, 'l');
        assert_eq!(cells[0][4].char, 'o');
    }

    #[test]
    fn test_cup_sequence() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"\x1b[10;20H"); // Move to row 10, col 20
        }

        let (row, col) = grid.cursor_position();
        assert_eq!(row, 9); // 0-indexed
        assert_eq!(col, 19); // 0-indexed
    }

    #[test]
    fn test_cpl_sequence_moves_previous_line_without_scroll() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"\x1b[10;20H");
            parser.process(b"\x1b[3F");
        }

        assert_eq!(grid.cursor_position(), (6, 0));
    }

    #[test]
    fn test_dsr_device_status_response() {
        let mut grid = Grid::new(80, 24);
        let mut parser = Parser::new(&mut grid);

        parser.process(b"\x1b[5n");

        assert_eq!(parser.take_response(), b"\x1b[0n");
    }

    #[test]
    fn test_dsr_cursor_position_response() {
        let mut grid = Grid::new(80, 24);
        let mut parser = Parser::new(&mut grid);

        parser.process(b"\x1b[10;20H");
        parser.process(b"\x1b[6n");

        assert_eq!(parser.take_response(), b"\x1b[10;20R");
    }

    #[test]
    fn test_deccolm_set_resizes_to_132_and_clears() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"X");
            parser.process(b"\x1b[10;20H");
            parser.process(b"\x1b[?3h");
        }

        assert_eq!(grid.size(), (132, 24));
        assert_eq!(grid.cursor_position(), (0, 0));
        assert_eq!(grid.get_cells()[0][0].char, ' ');
        assert!(matches!(
            grid.modes().columns,
            crate::terminal::modes::ColumnMode::Columns132
        ));
    }

    #[test]
    fn test_deccolm_reset_resizes_to_80_and_clears() {
        let mut grid = Grid::new(132, 24);
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"X");
            parser.process(b"\x1b[10;20H");
            parser.process(b"\x1b[?3l");
        }

        assert_eq!(grid.size(), (80, 24));
        assert_eq!(grid.cursor_position(), (0, 0));
        assert_eq!(grid.get_cells()[0][0].char, ' ');
        assert!(matches!(
            grid.modes().columns,
            crate::terminal::modes::ColumnMode::Columns80
        ));
    }

    #[test]
    fn test_sgr_reset() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"\x1b[31m"); // Red foreground
            assert_eq!(parser.current_fg(), Color::Ansi(1));

            parser.process(b"\x1b[0m"); // Reset
            assert_eq!(parser.current_fg(), Color::Default);
        }
    }

    #[test]
    fn test_sgr_colors() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);

            parser.process(b"\x1b[32m"); // Green
            assert_eq!(parser.current_fg(), Color::Ansi(2));

            parser.process(b"\x1b[44m"); // Blue background
            assert_eq!(parser.current_bg(), Color::Ansi(4));

            parser.process(b"\x1b[38;5;200m"); // 256 color foreground
            assert_eq!(parser.current_fg(), Color::Indexed(200));

            parser.process(b"\x1b[38;2;255;128;64m"); // True color
            assert_eq!(parser.current_fg(), Color::Rgb(255, 128, 64));
        }
    }

    #[test]
    fn test_cursor_save_restore() {
        let mut grid = Grid::new(80, 24);

        // Test save/restore via Grid's direct methods (tested in grid tests)
        // Here we just verify parser doesn't panic on these sequences
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"\x1b[5;10H");
            parser.process(b"\x1b7"); // Save
            parser.process(b"\x1b[1;1H");
            parser.process(b"\x1b8"); // Restore
        }
        // Success if we get here without panic
    }

    #[test]
    fn test_alternate_buffer() {
        let mut grid = Grid::new(80, 24);

        // Write to main buffer
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"Main content");
        }

        // Enable alternate buffer
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"\x1b[?1049h");
            parser.process(b"Alternate");
        }
        assert!(grid.using_alternate_buffer());

        // Disable alternate buffer
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"\x1b[?1049l");
        }
        assert!(!grid.using_alternate_buffer());

        // Verify main content is restored
        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'M');
    }

    #[test]
    fn test_scroll_region() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);

            // Set scroll region: lines 1-23 (0-indexed: 0-22)
            parser.process(b"\x1b[1;23r");
            let (top, bottom) = grid.scroll_region();
            assert_eq!(top, 0);
            assert_eq!(bottom, 22);

            // Cursor should be at home position after DECSTBM
            assert_eq!(grid.cursor_position(), (0, 0));
        }
    }

    #[test]
    fn test_reverse_index() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);

            // Set scroll region and move to top
            parser.process(b"\x1b[1;24r");
            parser.process(b"\x1b[H");

            // Reverse index at top should scroll down
            parser.process(b"\x1bM");

            // Should still be at top line after RI
            let (row, _) = grid.cursor_position();
            assert_eq!(row, 0);
        }
    }

    #[test]
    fn test_show_hide_cursor() {
        let mut grid = Grid::new(80, 24);

        // Default: cursor visible
        assert!(grid.show_cursor());

        // Hide cursor
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"\x1b[?25l");
            assert!(!grid.show_cursor());
        }

        // Show cursor
        {
            let mut parser = Parser::new(&mut grid);
            parser.process(b"\x1b[?25h");
            assert!(grid.show_cursor());
        }
    }

    #[test]
    fn test_vim_like_sequence() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);

            // Simulate vim startup sequence
            parser.process(b"\x1b[?1049h"); // Alternate buffer
            parser.process(b"\x1b[2J"); // Clear screen
            parser.process(b"\x1b[?25l"); // Hide cursor
            parser.process(b"\x1b[1;24r"); // Set scroll region
            parser.process(b"\x1b[H"); // Home position

            assert!(grid.using_alternate_buffer());
            assert!(!grid.show_cursor());
            assert!(grid.is_cleared());
        }
    }

    #[test]
    fn test_htop_like_sequence() {
        let mut grid = Grid::new(80, 24);
        {
            let mut parser = Parser::new(&mut grid);

            // Simulate htop startup
            parser.process(b"\x1b[?1049h");
            parser.process(b"\x1b[2J");
            parser.process(b"\x1b[1;32m"); // Green
            parser.process(b"PID");
            parser.process(b"\x1b[0m");

            assert!(grid.using_alternate_buffer());

            let cells = grid.get_cells();
            assert_eq!(cells[0][0].char, 'P');
        }
    }

    // === Kitty Graphics Protocol Tests ===

    #[test]
    fn test_parser_with_images() {
        let mut grid = Grid::new(80, 24);
        let mut image_manager = crate::image::ImageManager::new();

        {
            let mut parser = Parser::with_images(&mut grid, &mut image_manager);
            parser.process(b"Hello");
        }

        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'H');
    }

    #[test]
    fn test_kitty_transmit_sequence() {
        use base64::{engine::general_purpose::STANDARD, Engine};

        let mut grid = Grid::new(80, 24);
        let mut image_manager = crate::image::ImageManager::new();

        // Create a simple base64 encoded payload
        let base64_data = STANDARD.encode(b"test image data");
        let sequence = format!("\x1b_Gf=24,s=100,v=100,a=t;{}\x1b\\", base64_data);

        {
            let mut parser = Parser::with_images(&mut grid, &mut image_manager);
            parser.process(sequence.as_bytes());
        }

        // Parser processes the sequence without crashing
        // Full image decoding tested in image module tests
    }

    #[test]
    fn test_kitty_display_sequence() {
        let mut grid = Grid::new(80, 24);
        let mut image_manager = crate::image::ImageManager::new();

        // First transmit an image (simplified)
        // Then display it
        let sequence = b"\x1b_Ga=d,i=1,r=5,c=10,w=20,h=10\x1b\\";

        {
            let mut parser = Parser::with_images(&mut grid, &mut image_manager);
            parser.process(sequence);
        }

        // Check that placement was added
        let _placements = grid.image_placements();
        // Note: This test would need a valid image in cache to work fully
        // For now, we just verify the parser doesn't crash
    }

    #[test]
    fn test_kitty_delete_sequence() {
        let mut grid = Grid::new(80, 24);

        // Add a placement manually
        let placement = crate::image::ImagePlacement {
            image_id: crate::image::ImageId(1),
            row: 0,
            col: 0,
            width_cells: 10,
            height_cells: 5,
            z_index: 0,
        };
        grid.add_image_placement(placement);

        assert_eq!(grid.image_placements().len(), 1);

        // Delete the image via grid method (parser delegates to this)
        grid.remove_image_placement(crate::image::ImageId(1));

        assert_eq!(grid.image_placements().len(), 0);
    }

    #[test]
    fn test_kitty_clear_sequence() {
        let mut grid = Grid::new(80, 24);

        // Add some placements
        for i in 1..=3u32 {
            let placement = crate::image::ImagePlacement {
                image_id: crate::image::ImageId(i),
                row: 0,
                col: ((i - 1) * 10) as usize,
                width_cells: 10,
                height_cells: 5,
                z_index: 0,
            };
            grid.add_image_placement(placement);
        }

        assert_eq!(grid.image_placements().len(), 3);

        // Clear all images via grid method (parser delegates to this)
        grid.clear_image_placements();

        assert_eq!(grid.image_placements().len(), 0);
    }

    #[test]
    fn test_kitty_mixed_with_text() {
        use base64::{engine::general_purpose::STANDARD, Engine};

        let mut grid = Grid::new(80, 24);
        let mut image_manager = crate::image::ImageManager::new();

        let base64_data = STANDARD.encode(b"test");
        let sequence = format!("Before\x1b_Gf=24,s=10,v=10;{}\x1b\\After", base64_data);

        {
            let mut parser = Parser::with_images(&mut grid, &mut image_manager);
            parser.process(sequence.as_bytes());
        }

        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'B');
        assert_eq!(cells[0][1].char, 'e');
        assert_eq!(cells[0][2].char, 'f');
        assert_eq!(cells[0][3].char, 'o');
        assert_eq!(cells[0][4].char, 'r');
        assert_eq!(cells[0][5].char, 'e');
        // "After" should be after the image sequence
    }

    fn feed(grid: &mut Grid, data: &[u8]) {
        Parser::new(grid).process(data);
    }

    fn row_text(grid: &Grid, row: usize) -> String {
        grid.get_cells()[row]
            .iter()
            .map(|c| c.char)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// Regressão: CHA usava a coluna atual como linha de destino.
    /// Regressão: `ESC [ A` sem parâmetro (o que a seta envia) chega do vte
    /// como 0 e não movia o cursor.
    #[test]
    fn test_missing_or_zero_count_means_one() {
        let mut grid = Grid::new(20, 10);
        feed(&mut grid, b"\x1b[5;5H\x1b[A");
        assert_eq!(grid.cursor_position(), (3, 4));
        feed(&mut grid, b"\x1b[0B\x1b[C\x1b[0D\x1b[D");
        assert_eq!(grid.cursor_position(), (4, 3));
        feed(&mut grid, b"\x1b[F");
        assert_eq!(grid.cursor_position(), (3, 0));
    }

    #[test]
    fn test_cha_keeps_current_row() {
        let mut grid = Grid::new(80, 24);
        feed(&mut grid, b"\x1b[5;10H\x1b[3G");
        assert_eq!(grid.cursor_position(), (4, 2));
        feed(&mut grid, b"\x1b[G");
        assert_eq!(grid.cursor_position(), (4, 0));
    }

    /// Regressão: cada chunk recriava o parser, perdendo sequências partidas
    /// e as cores definidas no chunk anterior.
    #[test]
    fn test_state_survives_between_chunks() {
        let mut grid = Grid::new(20, 5);
        let mut state = ParserState::default();
        for chunk in [&b"\x1b[3"[..], b"1mR", b"\x1b[1", b";4H", b"X"] {
            let mut parser = Parser::resume(&mut grid, state);
            parser.process(chunk);
            state = parser.into_state();
        }
        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'R');
        assert_eq!(cells[0][0].fg, Color::Ansi(1));
        // "1;4H" chegou partido e mesmo assim posicionou o cursor
        assert_eq!(cells[0][3].char, 'X');
        assert_eq!(cells[0][3].fg, Color::Ansi(1), "cor do chunk anterior");
        assert_eq!(row_text(&grid, 0), "R  X");
        assert!(format!("{:?}", state).contains("Ansi(1)"));

        // O mesmo Parser também mantém a sequência entre chamadas de process
        let mut grid = Grid::new(10, 2);
        let mut parser = Parser::new(&mut grid);
        parser.process(b"\x1b[");
        parser.process(b"2CZ");
        drop(parser);
        assert_eq!(grid.get_cells()[0][2].char, 'Z');
    }

    #[test]
    fn test_c0_controls() {
        let mut grid = Grid::new(20, 5);
        feed(&mut grid, b"ab\x08c\x07");
        assert_eq!(row_text(&grid, 0), "ac");
        feed(&mut grid, b"\r\x09t");
        assert_eq!(grid.cursor_position(), (0, 9));
        feed(&mut grid, b"\x0b\x0c");
        assert_eq!(grid.cursor_position().0, 2);
        feed(&mut grid, b"\rq");
        assert_eq!(row_text(&grid, 2), "q");
    }

    #[test]
    fn test_cursor_movement_sequences() {
        let mut grid = Grid::new(80, 24);
        feed(&mut grid, b"\x1b[10;10H");
        feed(&mut grid, b"\x1b[2A");
        assert_eq!(grid.cursor_position(), (7, 9));
        feed(&mut grid, b"\x1b[3B");
        assert_eq!(grid.cursor_position(), (10, 9));
        feed(&mut grid, b"\x1b[4C");
        assert_eq!(grid.cursor_position(), (10, 13));
        feed(&mut grid, b"\x1b[D");
        assert_eq!(grid.cursor_position(), (10, 12));
        feed(&mut grid, b"\x1b[2E");
        assert_eq!(grid.cursor_position(), (12, 0));
        feed(&mut grid, b"\x1b[5d");
        assert_eq!(grid.cursor_position(), (4, 0));
        feed(&mut grid, b"\x1b[3;7f");
        assert_eq!(grid.cursor_position(), (2, 6));
    }

    #[test]
    fn test_erase_sequences() {
        let mut grid = Grid::new(10, 3);
        feed(&mut grid, b"0123456789\x1b[1;5H\x1b[K");
        assert_eq!(row_text(&grid, 0), "0123");
        feed(&mut grid, b"\x1b[1;3H\x1b[1K");
        assert_eq!(row_text(&grid, 0), "   3");
        feed(&mut grid, b"\x1b[2K");
        assert_eq!(row_text(&grid, 0), "");
        feed(&mut grid, b"\x1b[9K");

        feed(&mut grid, b"\x1b[2;1Habc\x1b[3;1Hdef\x1b[2;2H\x1b[0J");
        assert_eq!(row_text(&grid, 1), "a");
        assert_eq!(row_text(&grid, 2), "");
        feed(&mut grid, b"\x1b[2;1Hxyz\x1b[2;2H\x1b[1J");
        assert_eq!(row_text(&grid, 1), "  z");
        feed(&mut grid, b"\x1b[3J\x1b[7J");
        assert_eq!(grid.scrollback_len(), 0);
    }

    #[test]
    fn test_sgr_attributes_and_colors() {
        let mut grid = Grid::new(40, 2);
        let mut parser = Parser::new(&mut grid);
        parser.process(b"\x1b[2;3;4;5;7;8;9m");
        let attrs = parser.current_attrs();
        assert!(attrs.dim && attrs.italic && attrs.underline && attrs.blink);
        assert!(attrs.reverse && attrs.hidden && attrs.strikethrough);
        parser.process(b"\x1b[1;22;23;24;25;27;28;29m");
        let attrs = parser.current_attrs();
        assert!(!attrs.bold && !attrs.italic && !attrs.underline && !attrs.blink);
        assert!(!attrs.reverse && !attrs.hidden && !attrs.strikethrough);

        parser.process(b"\x1b[48;5;200m");
        assert_eq!(parser.current_bg(), Color::Indexed(200));
        parser.process(b"\x1b[48;2;1;2;3m");
        assert_eq!(parser.current_bg(), Color::Rgb(1, 2, 3));
        parser.process(b"\x1b[49;92;103m");
        assert_eq!(parser.current_fg(), Color::Ansi(10));
        assert_eq!(parser.current_bg(), Color::Ansi(11));
        parser.process(b"\x1b[39m");
        assert_eq!(parser.current_fg(), Color::Default);
        // Desconhecido é ignorado; reset vazio limpa tudo
        parser.process(b"\x1b[31;66m\x1b[m");
        assert_eq!(parser.current_fg(), Color::Default);
        assert_eq!(parser.current_bg(), Color::Default);
    }

    #[test]
    fn test_scroll_insert_delete_lines() {
        let mut grid = Grid::new(5, 3);
        feed(&mut grid, b"a\r\nb\r\nc");
        feed(&mut grid, b"\x1b[S");
        assert_eq!(row_text(&grid, 0), "b");
        feed(&mut grid, b"\x1b[T");
        assert_eq!(row_text(&grid, 1), "b");
        assert_eq!(row_text(&grid, 0), "");

        let mut grid = Grid::new(5, 3);
        feed(&mut grid, b"a\r\nb\r\nc\x1b[1;1H\x1b[L");
        assert_eq!(row_text(&grid, 0), "");
        assert_eq!(row_text(&grid, 1), "a");
        feed(&mut grid, b"\x1b[2M");
        assert_eq!(row_text(&grid, 0), "b");
    }

    #[test]
    fn test_scroll_region_reset_and_tab_clear() {
        let mut grid = Grid::new(20, 10);
        feed(&mut grid, b"\x1b[2;5r");
        feed(&mut grid, b"\x1b[r");
        // Sem região, o cursor vai para a origem
        assert_eq!(grid.cursor_position(), (0, 0));

        feed(&mut grid, b"\x1b[1;9H\x1b[g\x1b[1;1H\t");
        assert_eq!(
            grid.cursor_position(),
            (0, 16),
            "tab stop da coluna 8 removido"
        );
        feed(&mut grid, b"\x1b[3g\x1b[1;1H\t");
        assert_eq!(
            grid.cursor_position(),
            (0, 19),
            "sem tab stops vai para o fim"
        );
        feed(&mut grid, b"\x1b[5g");
    }

    #[test]
    fn test_private_modes_set_and_reset() {
        use crate::terminal::modes::{CursorKeysMode, MouseMode};

        let mut grid = Grid::new(20, 5);
        feed(&mut grid, b"\x1b[?1;6;7;12;1000;2004h");
        assert_eq!(grid.modes().cursor_keys, CursorKeysMode::Application);
        assert!(grid.modes().origin_mode);
        assert!(grid.modes().auto_wrap);
        assert!(grid.modes().bracketed_paste);
        assert_eq!(grid.modes().mouse_mode, MouseMode::VT200);
        feed(&mut grid, b"\x1b[?1002;1006;9999h");
        assert_eq!(grid.modes().mouse_mode, MouseMode::ButtonEvent);

        feed(&mut grid, b"\x1b[?1;6;7;12;1000;1006;2004;9999l");
        assert_eq!(grid.modes().cursor_keys, CursorKeysMode::Normal);
        assert!(!grid.modes().origin_mode);
        assert!(!grid.modes().auto_wrap);
        assert!(!grid.modes().bracketed_paste);
        assert_eq!(grid.modes().mouse_mode, MouseMode::None);
    }

    #[test]
    fn test_alternate_buffer_modes() {
        for (enable, disable) in [
            (&b"\x1b[?47h"[..], &b"\x1b[?47l"[..]),
            (b"\x1b[?1047h", b"\x1b[?1047l"),
        ] {
            let mut grid = Grid::new(10, 3);
            feed(&mut grid, b"main");
            feed(&mut grid, enable);
            assert!(grid.using_alternate_buffer());
            feed(&mut grid, disable);
            assert!(!grid.using_alternate_buffer());
            assert_eq!(row_text(&grid, 0), "main");
        }
    }

    #[test]
    fn test_mode_1048_saves_and_restores_cursor() {
        let mut grid = Grid::new(20, 5);
        let mut parser = Parser::new(&mut grid);
        parser.process(b"\x1b[3;4H\x1b[?1048h\x1b[1;1H\x1b[?1048l");
        drop(parser);
        assert_eq!(grid.cursor_position(), (2, 3));
    }

    #[test]
    fn test_standard_modes() {
        let mut grid = Grid::new(20, 5);
        feed(&mut grid, b"\x1b[4;20;99h");
        assert!(grid.modes().insert_mode);
        feed(&mut grid, b"\x1b[4;20;99l");
        assert!(!grid.modes().insert_mode);
    }

    #[test]
    fn test_esc_index_and_next_line() {
        let mut grid = Grid::new(10, 5);
        feed(&mut grid, b"ab\x1bD");
        // grid.newline() também volta à coluna 0
        assert_eq!(grid.cursor_position().0, 1);
        feed(&mut grid, b"\x1bE");
        assert_eq!(grid.cursor_position(), (2, 0));
    }

    #[test]
    fn test_osc_and_dcs_are_ignored_without_side_effects() {
        let mut grid = Grid::new(20, 2);
        feed(
            &mut grid,
            b"\x1b]0;titulo\x07\x1b]2\x07\x1b]8;;http://x\x1b\\\x1b]52;c;YQ==\x07\x1b]\x07ok",
        );
        assert_eq!(row_text(&grid, 0), "ok");
        // DCS genérico (não é Kitty graphics)
        feed(&mut grid, b"\x1bPq#0;1\x1b\\!");
        assert_eq!(row_text(&grid, 0), "ok!");
    }

    // === Conformidade VT (o que bash, nvim, htop, tmux e less usam) ===

    fn respond(grid: &mut Grid, data: &[u8]) -> Vec<u8> {
        let mut parser = Parser::new(grid);
        parser.process(data);
        parser.take_response()
    }

    /// Regressão: readline apaga no meio da linha com DCH e insere com ICH.
    #[test]
    fn test_ich_dch_ech() {
        let mut grid = Grid::new(10, 2);
        feed(&mut grid, b"abcdef\x1b[3G\x1b[2P");
        assert_eq!(row_text(&grid, 0), "abef");
        feed(&mut grid, b"\x1b[2@");
        assert_eq!(row_text(&grid, 0), "ab  ef");
        feed(&mut grid, b"XY\x1b[1G\x1b[2X");
        assert_eq!(row_text(&grid, 0), "  XYef");
        assert_eq!(grid.cursor_position(), (0, 0));
    }

    /// Regressão: apagar com fundo colorido deixava a linha sem cor (nvim).
    #[test]
    fn test_erase_uses_current_background() {
        let mut grid = Grid::new(6, 3);
        feed(&mut grid, b"\x1b[44m\x1b[2J\x1b[Hx\x1b[K");
        let row = &grid.get_cells()[0];
        assert_eq!(row[0].char, 'x');
        assert!(row.iter().all(|c| c.bg == Color::Ansi(4)));
        assert!(grid.get_cells()[2].iter().all(|c| c.bg == Color::Ansi(4)));
        feed(&mut grid, b"\x1b[0m\x1b[2;1H\x1b[K");
        assert!(grid.get_cells()[1].iter().all(|c| c.bg == Color::Default));
    }

    /// LF desce mantendo a coluna; só CR volta ao início.
    #[test]
    fn test_linefeed_keeps_column() {
        let mut grid = Grid::new(10, 3);
        feed(&mut grid, b"ab\ncd");
        assert_eq!(row_text(&grid, 1), "  cd");
        feed(&mut grid, b"\x1b[20h\nef");
        assert_eq!(row_text(&grid, 2), "ef");
    }

    /// Escrever na última coluna não quebra a linha até o próximo caractere.
    #[test]
    fn test_pending_wrap_at_last_column() {
        let mut grid = Grid::new(4, 3);
        feed(&mut grid, b"abcd");
        assert_eq!(grid.cursor_position(), (0, 3));
        feed(&mut grid, b"\r\n");
        assert_eq!(grid.cursor_position(), (1, 0));
        assert_eq!(row_text(&grid, 1), "");
        feed(&mut grid, b"wxyzQ");
        assert_eq!(row_text(&grid, 1), "wxyz");
        assert_eq!(row_text(&grid, 2), "Q");
        // Sem auto wrap, o último caractere sobrescreve a última coluna.
        let mut grid = Grid::new(4, 2);
        feed(&mut grid, b"\x1b[?7labcdef");
        assert_eq!(row_text(&grid, 0), "abcf");
    }

    /// Rolagem dentro de uma região (nvim, less) não vai para o histórico.
    #[test]
    fn test_region_scroll_does_not_pollute_scrollback() {
        let mut grid = Grid::new(10, 4);
        feed(&mut grid, b"\x1b[2;3r\x1b[3;1Hx\n\n\n");
        assert_eq!(grid.scrollback_len(), 0);
        let mut grid = Grid::new(10, 2);
        feed(&mut grid, b"\x1b[?1049h1\r\n2\r\n3\r\n");
        feed(&mut grid, b"\x1b[?1049l");
        assert_eq!(grid.scrollback_len(), 0);
        feed(&mut grid, b"1\r\n2\r\n3");
        assert_eq!(grid.scrollback_len(), 1);
    }

    #[test]
    fn test_cursor_up_down_stop_at_margins() {
        let mut grid = Grid::new(10, 10);
        feed(&mut grid, b"\x1b[3;6r\x1b[5;1H\x1b[9A");
        assert_eq!(grid.cursor_position().0, 2);
        feed(&mut grid, b"\x1b[9B");
        assert_eq!(grid.cursor_position().0, 5);
    }

    /// Bordas de tmux/mc/dialog usam o charset DEC de desenho de linhas.
    #[test]
    fn test_dec_special_graphics() {
        let mut grid = Grid::new(10, 2);
        feed(&mut grid, b"\x1b(0lqqk\x1b(Bq");
        assert_eq!(row_text(&grid, 0), "┌──┐q");
        feed(&mut grid, b"\r\n\x1b)0\x0ex\x0fx");
        assert_eq!(row_text(&grid, 1), "│x");
    }

    #[test]
    fn test_rep_cht_cbt_hpa_vpr() {
        let mut grid = Grid::new(30, 4);
        feed(&mut grid, b"=\x1b[4b");
        assert_eq!(row_text(&grid, 0), "=====");
        feed(&mut grid, b"\x1b[1G\x1b[2I");
        assert_eq!(grid.cursor_position(), (0, 16));
        feed(&mut grid, b"\x1b[Z");
        assert_eq!(grid.cursor_position(), (0, 8));
        feed(&mut grid, b"\x1b[3`\x1b[2e");
        assert_eq!(grid.cursor_position(), (2, 2));
    }

    /// Regressão: CSI > 4 ; 2 m (modifyOtherKeys do vim) era lido como SGR.
    #[test]
    fn test_private_csi_is_not_sgr() {
        let mut grid = Grid::new(10, 2);
        feed(&mut grid, b"\x1b[>4;2mA\x1b[?1;5mB");
        let row = &grid.get_cells()[0];
        assert!(!row[0].attrs.underline && !row[0].attrs.bold);
        assert!(!row[1].attrs.bold);
    }

    #[test]
    fn test_device_attributes_and_reports() {
        let mut grid = Grid::new(80, 24);
        assert_eq!(respond(&mut grid, b"\x1b[c"), b"\x1b[?62;22c");
        assert_eq!(respond(&mut grid, b"\x1b[>c"), b"\x1b[>0;10;1c");
        assert_eq!(respond(&mut grid, b"\x1b[3;5H\x1b[?6n"), b"\x1b[?3;5R");
        assert_eq!(respond(&mut grid, b"\x1b[18t"), b"\x1b[8;24;80t");
        assert!(respond(&mut grid, b"\x1b[>q").starts_with(b"\x1bP>|code-terminal("));
    }

    /// nvim/bat/delta perguntam a cor de fundo para escolher tema claro/escuro.
    #[test]
    fn test_osc_color_queries() {
        let mut grid = Grid::new(10, 2);
        grid.set_dynamic_colors((0xd4, 0xd4, 0xd4), (0x1e, 0x1e, 0x1e), (1, 2, 3));
        assert_eq!(
            respond(&mut grid, b"\x1b]11;?\x07"),
            b"\x1b]11;rgb:1e1e/1e1e/1e1e\x07"
        );
        assert_eq!(
            respond(&mut grid, b"\x1b]10;?\x1b\\"),
            b"\x1b]10;rgb:d4d4/d4d4/d4d4\x1b\\"
        );
        assert_eq!(
            respond(&mut grid, b"\x1b]12;?\x07"),
            b"\x1b]12;rgb:0101/0202/0303\x07"
        );
    }

    #[test]
    fn test_osc_title_and_cwd() {
        let mut grid = Grid::new(10, 2);
        feed(&mut grid, b"\x1b]2;diogo@pc1: ~/x;y\x07");
        assert_eq!(grid.title(), Some("diogo@pc1: ~/x;y"));
        feed(
            &mut grid,
            b"\x1b]7;file://example.invalid/home/example/meu%20projeto\x1b\\",
        );
        assert_eq!(
            grid.working_dir(),
            Some(std::path::Path::new("/home/example/meu projeto"))
        );
        feed(&mut grid, b"\x1b]0;\x07");
        assert_eq!(grid.title(), None);
    }

    #[test]
    fn test_cursor_shape_and_resets() {
        use crate::terminal::modes::CursorShapeMode;
        let mut grid = Grid::new(10, 3);
        feed(&mut grid, b"\x1b[6 q");
        assert_eq!(grid.modes().cursor_shape, Some(CursorShapeMode::Bar));
        feed(&mut grid, b"\x1b[4 q");
        assert_eq!(grid.modes().cursor_shape, Some(CursorShapeMode::Underline));
        feed(&mut grid, b"\x1b[0 q");
        assert_eq!(grid.modes().cursor_shape, None);

        feed(&mut grid, b"\x1b[?25l\x1b[!p");
        assert!(grid.modes().show_cursor);

        feed(&mut grid, b"\x1b[41mhello\x1bc");
        assert_eq!(row_text(&grid, 0), "");
        assert_eq!(grid.cursor_position(), (0, 0));
        feed(&mut grid, b"x");
        assert_eq!(grid.get_cells()[0][0].bg, Color::Default);
    }

    /// DECSC/DECRC salvam também cores e atributos.
    #[test]
    fn test_save_restore_cursor_keeps_style() {
        let mut grid = Grid::new(10, 2);
        feed(&mut grid, b"\x1b[31m\x1b7\x1b[0m\x1b[2;5H\x1b8x");
        assert_eq!(grid.get_cells()[0][0].fg, Color::Ansi(1));
        feed(&mut grid, b"\x1b[32m\x1b[s\x1b[0m\x1b[2;2H\x1b[uy");
        assert_eq!(grid.get_cells()[0][1].fg, Color::Ansi(2));
    }

    #[test]
    fn test_mouse_and_focus_modes() {
        use crate::terminal::modes::MouseMode;
        let mut grid = Grid::new(10, 2);
        feed(&mut grid, b"\x1b[?1003h\x1b[?1006h\x1b[?1004h");
        assert_eq!(grid.modes().mouse_mode, MouseMode::AnyEvent);
        assert!(grid.modes().sgr_mouse && grid.modes().focus_events);
        feed(&mut grid, b"\x1b[?1003l\x1b[?1006l\x1b[?1004l\x1b[?9h");
        assert_eq!(grid.modes().mouse_mode, MouseMode::X10);
        assert!(!grid.modes().sgr_mouse && !grid.modes().focus_events);
    }

    /// CJK/emoji ocupam duas colunas; o cursor avança 2.
    #[test]
    fn test_wide_chars_take_two_columns() {
        let mut grid = Grid::new(6, 2);
        feed(&mut grid, "a日b".as_bytes());
        let row = &grid.get_cells()[0];
        assert_eq!(row[1].char, '日');
        assert!(row[2].spacer);
        assert_eq!(row[3].char, 'b');
        assert_eq!(grid.cursor_position(), (0, 4));
        // Não cabe na última coluna: quebra a linha.
        feed(&mut grid, "c🚀".as_bytes());
        assert_eq!(grid.get_cells()[1][0].char, '🚀');
        assert_eq!(grid.cursor_position(), (1, 2));
        // Sobrescrever a metade de um largo apaga a outra metade.
        feed(&mut grid, b"\x1b[1;3Hx");
        let row = &grid.get_cells()[0];
        assert_eq!(row[1].char, ' ');
        assert_eq!(row[2].char, 'x');
        assert!(!row[2].spacer);
        // Marca de combinação é descartada.
        let mut grid = Grid::new(6, 1);
        feed(&mut grid, "e\u{301}x".as_bytes());
        assert_eq!(row_text(&grid, 0), "ex");
    }

    #[test]
    fn test_extended_color_and_sgr_22_clears_dim() {
        assert_eq!(extended_color(&[5, 200]), Some((Color::Indexed(200), 2)));
        assert_eq!(
            extended_color(&[2, 1, 2, 3, 9]),
            Some((Color::Rgb(1, 2, 3), 4))
        );
        assert_eq!(extended_color(&[2, 1]), None);
        let mut grid = Grid::new(10, 1);
        feed(&mut grid, b"\x1b[1;2;22mx\x1b[38;5;9;48;2;1;2;3my");
        let row = &grid.get_cells()[0];
        assert!(!row[0].attrs.bold && !row[0].attrs.dim);
        assert_eq!(row[1].fg, Color::Indexed(9));
        assert_eq!(row[1].bg, Color::Rgb(1, 2, 3));
    }

    /// Claude Code e outros agentes avisam por OSC 9, 777, 99 ou BEL.
    #[test]
    fn test_notifications_and_bell() {
        let mut grid = Grid::new(10, 2);
        feed(&mut grid, b"\x1b]9;Claude needs your input\x07");
        feed(&mut grid, b"\x1b]777;notify;Build;done; 0 errors\x1b\\");
        feed(&mut grid, b"\x1b]99;i=1:d=0;Tests passed\x07");
        feed(&mut grid, b"\x1b]99;i=1:p=body;all 42\x07");
        feed(&mut grid, b"\x1b]9;4;1;50\x07\x1b]9;\x07");
        let got = grid.take_notifications();
        assert_eq!(
            got,
            vec![
                TerminalNotification {
                    title: String::new(),
                    body: "Claude needs your input".into()
                },
                TerminalNotification {
                    title: "Build".into(),
                    body: "done".into()
                },
                TerminalNotification {
                    title: "Tests passed".into(),
                    body: String::new()
                },
                TerminalNotification {
                    title: String::new(),
                    body: "all 42".into()
                },
            ]
        );
        assert!(grid.take_notifications().is_empty());
        assert!(!grid.take_bell());
        feed(&mut grid, b"\x07");
        assert!(grid.take_bell());
        assert!(!grid.take_bell());
    }

    /// `imgcat`: a imagem ocupa células a partir do cursor, que desce para a
    /// linha seguinte; ela rola junto com o texto.
    #[test]
    fn test_iterm2_image_is_placed_and_scrolls() {
        use base64::Engine;
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(20, 40))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        let mut grid = Grid::new(20, 6);
        grid.set_cell_pixel_size(10.0, 20.0);
        feed(&mut grid, b"top\r\n");
        feed(
            &mut grid,
            format!("\x1b]1337;File=inline=1:{data}\x07").as_bytes(),
        );
        let images = grid.visible_images();
        assert_eq!(images.len(), 1);
        assert_eq!(
            (images[0].row, images[0].col, images[0].cols, images[0].rows),
            (1, 0, 2, 2)
        );
        assert_eq!(grid.cursor_position(), (3, 0));
        // Rola a tela: a imagem sobe junto.
        feed(&mut grid, b"a\r\nb\r\nc\r\nd\r\n");
        assert_eq!(grid.visible_images()[0].row, -1); // metade de cima já rolou
        feed(&mut grid, b"e\r\nf\r\ng\r\n");
        assert!(grid.visible_images().is_empty());
        // Limpar a tela tira as imagens visíveis.
        feed(
            &mut grid,
            format!("\x1b]1337;File=inline=1:{data}\x07\x1b[2J").as_bytes(),
        );
        assert!(grid.visible_images().is_empty());
    }

    #[test]
    fn test_parse_file_uri() {
        assert_eq!(
            parse_file_uri(b"file:///tmp/a%2Fb"),
            Some(std::path::PathBuf::from("/tmp/a/b"))
        );
        assert_eq!(parse_file_uri(b"http://x/y"), None);
        assert_eq!(parse_file_uri(b"file://host"), None);
    }
}
