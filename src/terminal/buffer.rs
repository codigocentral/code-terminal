use crate::config::Config;
use crate::terminal::grid::Grid;
use crate::terminal::images::{
    decode_kitty, kitty_reply, parse_kitty, ApcFilter, KittyAssembler, KittyCommand, Segment,
};
use crate::terminal::parser::{Parser, ParserState};
use crate::terminal::pty::Pty;
use anyhow::Result;
use tracing::warn;

/// `#rrggbb` para RGB; cor inválida vira `None`.
fn rgb_from_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some((channel(0)?, channel(2)?, channel(4)?))
}

/// Grid com o histórico e as cores do tema configurados.
fn themed_grid(cols: u16, rows: u16, config: &Config) -> Grid {
    let mut grid = Grid::with_scrollback_limit(
        cols as usize,
        rows as usize,
        config.terminal.scrollback_limit,
    );
    apply_theme_colors(&mut grid, &config.colors);
    grid
}

/// Informa ao grid as cores do tema (respondidas no OSC 10/11/12).
pub fn apply_theme_colors(grid: &mut Grid, colors: &crate::config::ColorScheme) {
    let fg = rgb_from_hex(&colors.foreground).unwrap_or((0xd4, 0xd4, 0xd4));
    let bg = rgb_from_hex(&colors.background).unwrap_or((0x1e, 0x1e, 0x1e));
    grid.set_dynamic_colors(fg, bg, fg);
}

/// Buffer do terminal que integra PTY, Parser VT e Grid
#[derive(Debug)]
pub struct TerminalBuffer {
    pty: Pty,
    grid: Grid,
    /// Estado do parser VT entre leituras do PTY
    parser_state: ParserState,
    /// Bytes recentes do PTY, quando o inspetor de protocolo está ligado
    inspector: Option<std::collections::VecDeque<u8>>,
    /// Imagens do kitty: APC separado antes do parser, pedaços e imagens transmitidas
    kitty: KittyState,
    /// Responder consultas (DSR, DA...) ao processo. O multiplexer desliga
    /// enquanto há cliente anexado: o terminal do cliente já responde.
    mute_replies: bool,
}

/// Estado do protocolo gráfico do kitty por painel.
#[derive(Debug, Default)]
struct KittyState {
    filter: ApcFilter,
    assembler: KittyAssembler,
    /// Imagens transmitidas (`a=t`) aguardando `a=p`, por id.
    stored: std::collections::HashMap<u32, crate::terminal::images::Pixels>,
}

/// Quantos bytes recentes o inspetor guarda por painel.
const INSPECTOR_BYTES: usize = 16 * 1024;

/// Mostra bytes do terminal de forma legível: ESC vira `\e`, controles viram
/// `^X`, quebras de linha continuam quebras.
pub fn escape_for_inspector(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\x1b' => out.push_str("\\e"),
            '\n' => out.push_str("\\n\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x07' => out.push_str("^G"),
            c if (c as u32) < 0x20 => out.push_str(&format!("^{}", (c as u8 + b'@') as char)),
            '\x7f' => out.push_str("^?"),
            c => out.push(c),
        }
    }
    out
}

impl TerminalBuffer {
    /// Cria um novo buffer de terminal com as dimensões especificadas
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        let pty = Pty::new(cols, rows)?;
        let grid = Grid::new(cols as usize, rows as usize);
        Ok(Self {
            pty,
            grid,
            parser_state: ParserState::default(),
            inspector: None,
            kitty: KittyState::default(),
            mute_replies: false,
        })
    }

    /// Cria um novo buffer de terminal com configuração
    pub fn with_config(cols: u16, rows: u16, config: &Config) -> Result<Self> {
        Self::with_config_in(cols, rows, config, None)
    }

    /// Como [`TerminalBuffer::with_config`], com o shell começando em `cwd`.
    pub fn with_config_in(
        cols: u16,
        rows: u16,
        config: &Config,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        let pty = Pty::new_in(cols, rows, cwd)?;
        let grid = themed_grid(cols, rows, config);
        Ok(Self {
            pty,
            grid,
            parser_state: ParserState::default(),
            inspector: None,
            kitty: KittyState::default(),
            mute_replies: false,
        })
    }

    /// Buffer de um painel: comando (ou shell), diretório e ambiente extra
    /// (ex.: `CODE_TERMINAL_PANE_ID`, para o CLI saber em que painel está).
    pub fn for_pane(
        cols: u16,
        rows: u16,
        config: &Config,
        command: Option<(&str, &[String])>,
        cwd: Option<&std::path::Path>,
        env: &[(String, String)],
    ) -> Result<Self> {
        let pty = Pty::spawn_with(cols, rows, command, cwd, env)?;
        let grid = themed_grid(cols, rows, config);
        Ok(Self {
            pty,
            grid,
            parser_state: ParserState::default(),
            inspector: None,
            kitty: KittyState::default(),
            mute_replies: false,
        })
    }

    /// Buffer de um PTY já aberto (tab persistente anexada ao daemon).
    pub fn with_pty(pty: Pty, cols: u16, rows: u16, config: &Config) -> Self {
        Self {
            pty,
            grid: themed_grid(cols, rows, config),
            parser_state: ParserState::default(),
            inspector: None,
            kitty: KittyState::default(),
            mute_replies: false,
        }
    }

    /// Cria um novo buffer executando um comando específico no PTY.
    pub fn with_command(
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
        config: &Config,
    ) -> Result<Self> {
        let pty = Pty::with_command(cols, rows, program, args)?;
        let grid = themed_grid(cols, rows, config);
        Ok(Self {
            pty,
            grid,
            parser_state: ParserState::default(),
            inspector: None,
            kitty: KittyState::default(),
            mute_replies: false,
        })
    }

    /// Processa o output do PTY através do parser VT
    pub fn process_output(&mut self) {
        self.drain_output();
    }

    /// Como [`TerminalBuffer::process_output`], devolvendo os bytes crus lidos
    /// (o multiplexer os repassa aos clientes anexados).
    pub fn drain_output(&mut self) -> Vec<u8> {
        let mut raw = Vec::new();
        while let Some(chunk) = self.pty.try_read_chunk() {
            let data = chunk.data;
            if chunk.replay {
                // Histórico de uma tab persistente reanexada: desenha, mas não
                // responde às consultas antigas nem repete avisos e campainha.
                let muted = std::mem::replace(&mut self.mute_replies, true);
                self.feed(&data);
                self.mute_replies = muted;
                let _ = self.grid.take_notifications();
                let _ = self.grid.take_bell();
                continue;
            }
            raw.extend_from_slice(&data);
            if let Some(log) = self.inspector.as_mut() {
                log.extend(&data);
                let excess = log.len().saturating_sub(INSPECTOR_BYTES);
                log.drain(..excess);
            }
            self.feed(&data);
        }
        raw
    }

    /// Passa a saída pelo parser (as imagens do kitty saem antes do vte, que
    /// descarta APC, na ordem).
    fn feed(&mut self, data: &[u8]) {
        for segment in self.kitty.filter.feed(data) {
            match segment {
                Segment::Text(text) => self.parse(&text),
                Segment::Apc(apc) => self.handle_apc(&apc),
            }
        }
    }

    /// Texto exibido no painel sem passar pelo processo (plugins). As
    /// respostas do parser (ex.: DSR) não vão ao shell: quem escreveu não
    /// foi o programa do painel.
    pub fn inject_output(&mut self, data: &[u8]) {
        let state = std::mem::take(&mut self.parser_state);
        let mut parser = Parser::resume(&mut self.grid, state);
        parser.process(data);
        let _ = parser.take_response();
        self.parser_state = parser.into_state();
    }

    /// Passa bytes pelo parser VT e responde o que ele pedir.
    fn parse(&mut self, data: &[u8]) {
        // Retoma o estado do chunk anterior: sequências e cores podem
        // atravessar a fronteira entre leituras do PTY.
        let state = std::mem::take(&mut self.parser_state);
        let mut parser = Parser::resume(&mut self.grid, state);
        parser.process(data);
        let response = parser.take_response();
        self.parser_state = parser.into_state();
        self.reply(&response);
    }

    /// Liga/desliga as respostas do parser ao processo (ver `mute_replies`).
    pub fn set_mute_replies(&mut self, mute: bool) {
        self.mute_replies = mute;
    }

    fn reply(&mut self, response: &[u8]) {
        if !response.is_empty() && !self.mute_replies {
            if let Err(error) = self.pty.write(response) {
                warn!(error = %error, "Falha ao responder sequência VT ao PTY");
            }
        }
    }

    /// Comando gráfico do kitty (`ESC _ G … ESC \`).
    fn handle_apc(&mut self, apc: &[u8]) {
        let Some(cmd) = parse_kitty(apc) else {
            return;
        };
        let Some(cmd) = self.kitty.assembler.push(cmd) else {
            return;
        };
        let result = self.run_kitty(&cmd);
        if let Some(reply) = kitty_reply(&cmd, result) {
            self.reply(&reply);
        }
    }

    fn run_kitty(&mut self, cmd: &KittyCommand) -> std::result::Result<(), &'static str> {
        match cmd.action {
            'q' => decode_kitty(cmd)
                .map(|_| ())
                .ok_or("EINVAL:unsupported image"),
            't' | 'T' => {
                let pixels = std::sync::Arc::new(decode_kitty(cmd).ok_or("EINVAL:bad image data")?);
                if cmd.action == 'T' {
                    self.place_kitty(&pixels, cmd);
                }
                if cmd.id != 0 {
                    if self.kitty.stored.len() >= 32 {
                        self.kitty.stored.clear();
                    }
                    self.kitty.stored.insert(cmd.id, pixels);
                }
                Ok(())
            }
            'p' => {
                let pixels = self
                    .kitty
                    .stored
                    .get(&cmd.id)
                    .cloned()
                    .ok_or("ENOENT:no such image")?;
                self.place_kitty(&pixels, cmd);
                Ok(())
            }
            'd' => {
                self.grid.clear_images();
                Ok(())
            }
            _ => Err("EINVAL:unsupported action"),
        }
    }

    fn place_kitty(&mut self, pixels: &crate::terminal::images::Pixels, cmd: &KittyCommand) {
        use crate::terminal::images::{cells_for, SizeSpec};
        let spec = |n: u32| {
            if n == 0 {
                SizeSpec::Auto
            } else {
                SizeSpec::Cells(n)
            }
        };
        let (cols, rows) = cells_for(
            pixels.dimensions(),
            spec(cmd.cols),
            spec(cmd.rows),
            false,
            self.grid.cell_pixel_size(),
            self.grid.size(),
        );
        self.grid.place_image(pixels.clone(), cols, rows);
    }

    /// Escreve dados no PTY (input do usuário)
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.pty.write(data)
    }

    /// Escreve uma string no PTY
    pub fn write_str(&mut self, s: &str) -> Result<()> {
        self.pty.write_str(s)
    }

    /// Redimensiona o terminal
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.pty.resize(cols, rows)?;
        self.grid.resize(cols as usize, rows as usize);
        Ok(())
    }

    /// Liga/desliga a gravação de bytes para o inspetor de protocolo.
    pub fn set_inspector(&mut self, enabled: bool) {
        self.inspector =
            enabled.then(|| std::collections::VecDeque::with_capacity(INSPECTOR_BYTES));
    }

    /// O inspetor está gravando este painel?
    pub fn inspector_enabled(&self) -> bool {
        self.inspector.is_some()
    }

    /// Últimas `lines` linhas do fluxo bruto, já legíveis.
    pub fn inspector_lines(&self, lines: usize) -> Vec<String> {
        let Some(log) = &self.inspector else {
            return Vec::new();
        };
        let bytes: Vec<u8> = log.iter().copied().collect();
        let text = escape_for_inspector(&bytes);
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..]
            .iter()
            .map(|line| line.to_string())
            .collect()
    }

    /// Retorna referência ao grid
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// Retorna referência mutável ao grid
    pub fn grid_mut(&mut self) -> &mut Grid {
        &mut self.grid
    }

    /// Retorna referência ao PTY
    pub fn pty(&self) -> &Pty {
        &self.pty
    }

    /// PTY para mudar de dono (tornar a tab persistente).
    pub fn pty_mut(&mut self) -> &mut Pty {
        &mut self.pty
    }

    /// Retorna o conteúdo como texto
    pub fn get_content_text(&self) -> String {
        self.grid.get_selected_text()
    }

    /// Retorna o tamanho do conteúdo (cols, rows)
    pub fn content_size(&self) -> (u16, u16) {
        let (cols, rows) = self.grid.size();
        (cols as u16, rows as u16)
    }

    /// Scroll up
    pub fn scroll_up(&mut self, lines: usize) {
        self.grid.scroll_up_lines(lines);
    }

    /// Scroll down
    pub fn scroll_down(&mut self, lines: usize) {
        self.grid.scroll_down_lines(lines);
    }

    /// Scroll to top
    pub fn scroll_to_top(&mut self) {
        self.grid.scroll_to_top();
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        self.grid.scroll_to_bottom();
    }

    /// Retorna a posição do scroll
    pub fn scroll_position(&self) -> usize {
        self.grid.scroll_offset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_for_inspector() {
        assert_eq!(
            escape_for_inspector(b"\x1b[31mred\x1b[0m\r\n\x07\x01"),
            "\\e[31mred\\e[0m\\r\\n\n^G^A"
        );
    }

    /// O inspetor guarda o que o programa mandou, só quando ligado.
    #[cfg(unix)]
    #[test]
    fn test_inspector_records_raw_output() {
        let config = Config::default();
        let mut buffer = TerminalBuffer::with_command(
            40,
            5,
            "/bin/sh",
            &[
                "-c".into(),
                "sleep 0.3; printf '\\033[1mhi\\033[0m'; sleep 5".into(),
            ],
            &config,
        )
        .unwrap();
        assert!(buffer.inspector_lines(5).is_empty());
        buffer.set_inspector(true);
        assert!(buffer.inspector_enabled());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut lines = Vec::new();
        while std::time::Instant::now() < deadline {
            buffer.process_output();
            lines = buffer.inspector_lines(5);
            if lines.iter().any(|l| l.contains("\\e[1mhi\\e[0m")) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            lines.iter().any(|l| l.contains("\\e[1mhi\\e[0m")),
            "{lines:?}"
        );
        buffer.set_inspector(false);
        assert!(buffer.inspector_lines(5).is_empty());
    }

    /// `kitty +kitten icat` / `timg -pk`: a imagem chega por APC (que o vte
    /// descartava) e ocupa células; a consulta inicial recebe OK.
    #[cfg(unix)]
    #[test]
    fn test_kitty_graphics_through_the_pty() {
        use base64::Engine;
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(18, 36))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        let (a, b) = data.split_at(data.len() / 2);
        let script = format!(
            "sleep 0.2; printf 'x\\033_Ga=T,f=100,i=5,m=1;{a}\\033\\\\'; printf '\\033_Gm=0;{b}\\033\\\\y'; sleep 5"
        );
        let mut buffer = TerminalBuffer::with_command(
            20,
            6,
            "/bin/sh",
            &["-c".into(), script],
            &Config::default(),
        )
        .unwrap();
        buffer.grid_mut().set_cell_pixel_size(9.0, 18.0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while buffer.grid().image_count() == 0 && std::time::Instant::now() < deadline {
            buffer.process_output();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let images = buffer.grid().visible_images();
        assert_eq!(images.len(), 1);
        assert_eq!((images[0].cols, images[0].rows), (2, 2));
        // O texto antes e depois do APC continua no grid, na ordem.
        let text: String = buffer.grid().get_cells()[0]
            .iter()
            .map(|c| c.char)
            .collect();
        assert!(text.starts_with('x'), "{text:?}");
    }

    #[test]
    fn test_rgb_from_hex() {
        assert_eq!(rgb_from_hex("#1e1e1e"), Some((0x1e, 0x1e, 0x1e)));
        assert_eq!(rgb_from_hex("fafafa"), Some((0xfa, 0xfa, 0xfa)));
        assert_eq!(rgb_from_hex("#12"), None);
        assert_eq!(rgb_from_hex("#zzzzzz"), None);
    }

    #[test]
    fn test_buffer_creation() {
        let buffer = TerminalBuffer::new(80, 24);
        assert!(buffer.is_ok());
    }

    #[test]
    fn test_buffer_resize() {
        let mut buffer = TerminalBuffer::new(80, 24).unwrap();
        assert!(buffer.resize(100, 30).is_ok());

        let (cols, rows) = buffer.grid().size();
        assert_eq!(cols, 100);
        assert_eq!(rows, 30);
    }

    #[test]
    fn test_buffer_with_config() {
        let mut config = Config::default();
        config.terminal.scrollback_limit = 5000;

        let buffer = TerminalBuffer::with_config(80, 24, &config);
        assert!(buffer.is_ok());

        let buffer = buffer.unwrap();
        assert_eq!(buffer.grid().scrollback_limit(), 5000);
    }
}
