use crate::image::{ImageId, ImagePlacement};
use crate::terminal::history::History;
use crate::terminal::modes::TerminalModes;
use crate::terminal::parser::{Attributes, Color};
use crate::ui::search_bar::SearchMatch;

/// Representa uma célula no grid do terminal
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub char: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: Attributes,
    pub highlight: bool,
    /// Se a célula está selecionada
    pub selected: bool,
    /// Continuação de um caractere largo (CJK, emoji) da célula à esquerda.
    pub spacer: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self::empty()
    }
}

impl Cell {
    pub fn new(char: char, fg: Color, bg: Color, attrs: Attributes) -> Self {
        Self {
            char,
            fg,
            bg,
            attrs,
            highlight: false,
            selected: false,
            spacer: false,
        }
    }

    /// Cria uma célula vazia (espaço com cores default)
    pub fn empty() -> Self {
        Self {
            char: ' ',
            fg: Color::Default,
            bg: Color::Default,
            attrs: Attributes::default(),
            highlight: false,
            selected: false,
            spacer: false,
        }
    }
}

/// Tipo de seleção
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Seleção normal (caracteres)
    Normal,
    /// Seleção de linha (triple-click)
    Line,
    /// Seleção de palavra (double-click)
    Word,
}

/// Representa uma seleção de texto no terminal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Posição inicial (row, col)
    pub start: (usize, usize),
    /// Posição final (row, col)
    pub end: (usize, usize),
    /// Seleção ativa
    pub active: bool,
    /// Tipo de seleção
    pub kind: SelectionKind,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            start: (0, 0),
            end: (0, 0),
            active: false,
            kind: SelectionKind::Normal,
        }
    }
}

/// Imagem inline posta no grid, ancorada numa linha absoluta (acompanha a
/// rolagem e o histórico).
#[derive(Clone, Debug)]
pub struct CellImage {
    pub pixels: crate::terminal::images::Pixels,
    /// Identificador único (chave da textura no renderer).
    pub id: u64,
    /// Linha absoluta: linhas que já rolaram para o histórico + linha na tela.
    pub abs_row: u64,
    pub col: usize,
    pub cols: usize,
    pub rows: usize,
    /// Posta na tela alternativa (some ao sair dela).
    pub alt: bool,
}

/// Imagem visível, com a linha na tela (pode ser negativa: começou acima).
#[derive(Clone, Debug)]
pub struct VisibleImage {
    pub pixels: crate::terminal::images::Pixels,
    pub id: u64,
    pub row: i64,
    pub col: usize,
    pub cols: usize,
    pub rows: usize,
}

/// Quantas imagens o grid guarda (as mais antigas saem).
const MAX_IMAGES: usize = 64;

static NEXT_IMAGE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Aviso pedido pelo programa (OSC 9/99/777) ou campainha (BEL).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalNotification {
    pub title: String,
    pub body: String,
}

/// Cores que o programa pode consultar via OSC 10/11/12.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicColor {
    Foreground,
    Background,
    Cursor,
}

/// Grid de caracteres do terminal
#[derive(Debug, Clone)]
pub struct Grid {
    cols: usize,
    rows: usize,
    cursor_row: usize,
    cursor_col: usize,
    cells: Vec<Vec<Cell>>,
    /// Histórico compacto (texto + rodadas de estilo, #1).
    scrollback: History,
    scrollback_limit: usize,
    /// Linhas do histórico que aparecem na tela quando a aba está rolada,
    /// já abertas em células (vazio com `scroll_offset == 0`). Atualizado
    /// por [`Grid::refresh_view`] em toda mudança da rolagem ou do histórico.
    view: Vec<Vec<Cell>>,
    tab_stops: Vec<bool>,
    /// Modos de terminal ativos
    modes: TerminalModes,
    /// Buffer principal (salvo quando usa alternate)
    main_buffer_cells: Option<Vec<Vec<Cell>>>,
    main_buffer_scrollback: Option<History>,
    main_cursor: Option<(usize, usize)>,
    /// Scroll region top margin (0-indexed, inclusive)
    scroll_top: usize,
    /// Scroll region bottom margin (0-indexed, inclusive)
    scroll_bottom: usize,
    /// Scroll offset para navegação no scrollback (0 = no final)
    scroll_offset: usize,
    /// Seleção atual
    selection: Selection,
    /// Image placements for Kitty graphics protocol
    image_placements: Vec<ImagePlacement>,
    /// Saved image placements for alternate buffer
    main_image_placements: Option<Vec<ImagePlacement>>,
    /// Fundo usado ao apagar/rolar (BCE: o xterm-256color apaga com o fundo atual).
    erase_bg: Color,
    /// O último caractere foi escrito na última coluna: o próximo quebra a linha.
    wrap_pending: bool,
    /// Título pedido pelo programa (OSC 0/2).
    title: Option<String>,
    /// Diretório informado pelo shell (OSC 7).
    working_dir: Option<std::path::PathBuf>,
    /// Frente, fundo e cursor do tema, respondidos no OSC 10/11/12.
    dynamic_colors: [(u8, u8, u8); 3],
    /// Avisos ainda não lidos pelo app.
    notifications: Vec<TerminalNotification>,
    /// O programa tocou a campainha (BEL) desde a última leitura.
    bell: bool,
    /// Imagens inline (iTerm2/kitty)
    images: Vec<CellImage>,
    /// Linhas que já rolaram para fora da tela (âncora das imagens)
    scrolled_total: u64,
    /// Tamanho de uma célula em pixels (para medir imagens)
    cell_pixels: (f32, f32),
}

impl Grid {
    /// Cria um novo grid com as dimensões especificadas
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback_limit(cols, rows, 10000)
    }

    /// Cria um novo grid com limite de scrollback configurável
    pub fn with_scrollback_limit(cols: usize, rows: usize, scrollback_limit: usize) -> Self {
        let cells = vec![vec![Cell::default(); cols]; rows];
        let mut tab_stops = vec![false; cols];

        // Tab stops a cada 8 colunas
        for i in (0..cols).step_by(8) {
            tab_stops[i] = true;
        }

        Self {
            cols,
            rows,
            cursor_row: 0,
            cursor_col: 0,
            cells,
            scrollback: History::new(),
            scrollback_limit,
            view: Vec::new(),
            tab_stops,
            modes: TerminalModes::new(),
            main_buffer_cells: None,
            main_buffer_scrollback: None,
            main_cursor: None,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            scroll_offset: 0,
            selection: Selection::default(),
            image_placements: Vec::new(),
            main_image_placements: None,
            erase_bg: Color::Default,
            wrap_pending: false,
            title: None,
            working_dir: None,
            dynamic_colors: [(0xd4, 0xd4, 0xd4), (0x1e, 0x1e, 0x1e), (0xd4, 0xd4, 0xd4)],
            notifications: Vec::new(),
            bell: false,
            images: Vec::new(),
            scrolled_total: 0,
            cell_pixels: (9.0, 18.0),
        }
    }

    // === Imagens inline ===

    /// Tamanho de uma célula em pixels (o app informa ao medir a fonte).
    pub fn set_cell_pixel_size(&mut self, width: f32, height: f32) {
        if width > 0.0 && height > 0.0 {
            self.cell_pixels = (width, height);
        }
    }

    pub fn cell_pixel_size(&self) -> (f32, f32) {
        self.cell_pixels
    }

    /// Põe uma imagem no cursor ocupando `cols` x `rows` células e desce o
    /// cursor para a linha seguinte à imagem (como o iTerm2 e o kitty).
    pub fn place_image(
        &mut self,
        pixels: crate::terminal::images::Pixels,
        cols: usize,
        rows: usize,
    ) {
        self.wrap_pending = false;
        let cols = cols.clamp(1, self.cols.max(1));
        let rows = rows.max(1);
        // Reserva as linhas: rolar a tela agora mantém a âncora certa.
        let col = self.cursor_col.min(self.cols.saturating_sub(1));
        for _ in 0..rows.saturating_sub(1) {
            self.index();
        }
        // A imagem termina na linha do cursor; a primeira linha pode já ter
        // rolado para cima se a imagem for mais alta que a tela.
        let abs_row =
            (self.scrolled_total as i64 + self.cursor_row as i64 + 1 - rows as i64).max(0) as u64;
        if self.images.len() >= MAX_IMAGES {
            self.images.remove(0);
        }
        self.images.push(CellImage {
            pixels,
            id: NEXT_IMAGE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            abs_row,
            col,
            cols,
            rows,
            alt: self.modes.alternate_buffer,
        });
        self.newline();
    }

    /// Imagens que aparecem na tela agora (considerando o histórico rolado).
    pub fn visible_images(&self) -> Vec<VisibleImage> {
        let top = self.scrolled_total as i64 - self.scroll_offset as i64;
        let alt = self.modes.alternate_buffer;
        self.images
            .iter()
            .filter(|image| image.alt == alt)
            .filter_map(|image| {
                let row = image.abs_row as i64 - top;
                let visible = row + image.rows as i64 > 0 && row < self.rows as i64;
                visible.then(|| VisibleImage {
                    pixels: image.pixels.clone(),
                    id: image.id,
                    row,
                    col: image.col,
                    cols: image.cols,
                    rows: image.rows,
                })
            })
            .collect()
    }

    /// Apaga todas as imagens (kitty `a=d`).
    pub fn clear_images(&mut self) {
        self.images.clear();
    }

    /// Quantas imagens o grid guarda.
    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    /// Tira as imagens que estão na tela atual (apagar a tela).
    fn clear_visible_images(&mut self) {
        let top = self.scrolled_total;
        let bottom = top + self.rows as u64;
        let alt = self.modes.alternate_buffer;
        self.images.retain(|image| {
            image.alt != alt || image.abs_row + image.rows as u64 <= top || image.abs_row >= bottom
        });
    }

    /// Registra um aviso do programa (OSC 9/99/777).
    pub fn push_notification(&mut self, notification: TerminalNotification) {
        // Limite para um programa mal comportado não encher a memória.
        if self.notifications.len() < 64 {
            self.notifications.push(notification);
        }
    }

    /// Avisos pendentes (esvazia a fila).
    pub fn take_notifications(&mut self) -> Vec<TerminalNotification> {
        std::mem::take(&mut self.notifications)
    }

    /// Registra a campainha (BEL).
    pub fn ring_bell(&mut self) {
        self.bell = true;
    }

    /// A campainha tocou desde a última leitura? (zera o aviso)
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// Define as cores do tema respondidas no OSC 10/11/12.
    pub fn set_dynamic_colors(&mut self, fg: (u8, u8, u8), bg: (u8, u8, u8), cursor: (u8, u8, u8)) {
        self.dynamic_colors = [fg, bg, cursor];
    }

    /// Cor do tema pedida pelo programa.
    pub fn dynamic_color(&self, which: DynamicColor) -> (u8, u8, u8) {
        match which {
            DynamicColor::Foreground => self.dynamic_colors[0],
            DynamicColor::Background => self.dynamic_colors[1],
            DynamicColor::Cursor => self.dynamic_colors[2],
        }
    }

    /// Título definido pelo programa (OSC 0/2), se houver.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Define o título (vazio limpa).
    pub fn set_title(&mut self, title: &str) {
        self.title = (!title.is_empty()).then(|| title.to_string());
    }

    /// Diretório informado pelo shell via OSC 7.
    pub fn working_dir(&self) -> Option<&std::path::Path> {
        self.working_dir.as_deref()
    }

    /// Define o diretório informado pelo shell.
    pub fn set_working_dir(&mut self, dir: std::path::PathBuf) {
        self.working_dir = Some(dir);
    }

    /// Reset completo (RIS): limpa tela, histórico, modos e margens.
    pub fn full_reset(&mut self) {
        if self.modes.alternate_buffer {
            self.disable_alternate_buffer();
        }
        self.modes.reset();
        self.erase_bg = Color::Default;
        self.erase_all();
        self.scrollback.clear();
        self.scroll_offset = 0;
        self.view.clear();
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.wrap_pending = false;
        self.title = None;
        self.images.clear();
        for (i, stop) in self.tab_stops.iter_mut().enumerate() {
            *stop = i % 8 == 0;
        }
    }

    /// Define o fundo usado por apagar, inserir e rolar (BCE).
    pub fn set_erase_background(&mut self, bg: Color) {
        self.erase_bg = bg;
    }

    /// Célula em branco com o fundo de apagamento atual.
    fn blank(&self) -> Cell {
        Cell {
            bg: self.erase_bg,
            ..Cell::empty()
        }
    }

    fn blank_row(&self) -> Vec<Cell> {
        vec![self.blank(); self.cols]
    }

    /// Retorna referência aos modos
    pub fn modes(&self) -> &TerminalModes {
        &self.modes
    }

    /// Retorna referência mutável aos modos
    pub fn modes_mut(&mut self) -> &mut TerminalModes {
        &mut self.modes
    }

    /// Verifica se está usando alternate buffer
    pub fn using_alternate_buffer(&self) -> bool {
        self.modes.alternate_buffer
    }

    /// Habilita o alternate screen buffer
    pub fn enable_alternate_buffer(&mut self) {
        if self.modes.alternate_buffer {
            return; // Já está ativo
        }

        // Salva estado do buffer principal
        self.main_buffer_cells = Some(std::mem::take(&mut self.cells));
        self.main_buffer_scrollback = Some(std::mem::take(&mut self.scrollback));
        self.main_cursor = Some((self.cursor_row, self.cursor_col));
        self.main_image_placements = Some(std::mem::take(&mut self.image_placements));

        // Tela alternativa limpa. (Antes também criava um `AlternateBuffer`
        // que ninguém lia: outra tela inteira de células e uma reserva de 10
        // mil linhas a cada `vim`/`htop`, #1.)
        self.cells = vec![vec![Cell::default(); self.cols]; self.rows];
        self.scrollback.clear();
        self.image_placements.clear();
        self.cursor_row = 0;
        self.cursor_col = 0;

        self.modes.alternate_buffer = true;
        self.refresh_view();
    }

    /// Desabilita o alternate screen buffer (restaura principal)
    pub fn disable_alternate_buffer(&mut self) {
        if !self.modes.alternate_buffer {
            return; // Não está ativo
        }
        self.images.retain(|image| !image.alt);

        // Restaura buffer principal
        if let Some(cells) = self.main_buffer_cells.take() {
            self.cells = cells;
        }
        if let Some(scrollback) = self.main_buffer_scrollback.take() {
            self.scrollback = scrollback;
        }
        if let Some((row, col)) = self.main_cursor.take() {
            self.cursor_row = row;
            self.cursor_col = col;
        }
        if let Some(placements) = self.main_image_placements.take() {
            self.image_placements = placements;
        }

        self.modes.alternate_buffer = false;
        self.refresh_view();
    }

    /// Escreve um caractere na posição atual do cursor.
    ///
    /// Caracteres largos (CJK, emoji) ocupam duas colunas: a segunda vira uma
    /// célula de continuação (`spacer`). Marcas de combinação (largura 0)
    /// são descartadas.
    pub fn write(&mut self, c: char, fg: Color, bg: Color, attrs: Attributes) {
        let width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if width == 0 {
            return;
        }
        let width = if width >= 2 && self.cols >= 2 { 2 } else { 1 };
        let Some(col) = self.write_position(width) else {
            return;
        };

        self.clear_wide_partners(col, width);
        let row = &mut self.cells[self.cursor_row];
        if self.modes.insert_mode {
            // Modo de inserção: empurra o resto da linha para a direita.
            row[col..].rotate_right(width);
        }
        row[col] = Cell::new(c, fg, bg, attrs);
        if width == 2 {
            row[col + 1] = Cell {
                spacer: true,
                ..Cell::new(' ', fg, bg, attrs)
            };
        }
        let next = col + width;
        if next >= self.cols {
            // Última coluna: o cursor fica nela até o próximo caractere.
            self.cursor_col = self.cols - 1;
            self.wrap_pending = true;
        } else {
            self.cursor_col = next;
        }
    }

    /// Coluna onde o próximo caractere de `width` colunas vai, já aplicando a
    /// quebra pendente e quebrando antes de um largo que não cabe.
    fn write_position(&mut self, width: usize) -> Option<usize> {
        if self.wrap_pending {
            self.wrap_pending = false;
            if self.modes.auto_wrap {
                self.newline();
            }
        }
        if self.cursor_row >= self.rows || self.cols == 0 {
            return None;
        }
        let col = self.cursor_col.min(self.cols - 1);
        if width < 2 || col + 1 < self.cols {
            return Some(col);
        }
        if !self.modes.auto_wrap {
            return Some(self.cols - 2);
        }
        // Não cabe na última coluna: quebra antes.
        let blank = self.blank();
        self.cells[self.cursor_row][col] = blank;
        self.newline();
        Some(0)
    }

    /// Ao sobrescrever metade de um caractere largo, apaga a outra metade.
    fn clear_wide_partners(&mut self, col: usize, width: usize) {
        let blank = self.blank();
        let row = &mut self.cells[self.cursor_row];
        if row[col].spacer && col > 0 {
            row[col - 1] = blank;
        }
        let last = (col + width - 1).min(row.len() - 1);
        if last + 1 < row.len() && row[last + 1].spacer {
            row[last + 1] = blank;
        }
    }

    /// Escreve caractere com estilo padrão (cor default)
    pub fn write_char(&mut self, c: char) {
        self.write(c, Color::Default, Color::Default, Attributes::default());
    }

    /// Nova linha: volta à coluna 0 e desce (quebra automática, NEL).
    pub fn newline(&mut self) {
        self.cursor_col = 0;
        self.index();
    }

    /// Index (LF, IND): desce uma linha mantendo a coluna, rolando a região
    /// quando o cursor está na margem de baixo.
    pub fn index(&mut self) {
        self.wrap_pending = false;

        // Se estava visualizando scrollback, volta para o final
        if self.scroll_offset > 0 {
            self.scroll_to_bottom();
        }

        if self.cursor_row == self.scroll_bottom {
            self.scroll_up_one();
        } else if self.cursor_row + 1 < self.rows {
            self.cursor_row += 1;
        }
    }

    /// Carriage return (move para início da linha)
    pub fn carriage_return(&mut self) {
        self.wrap_pending = false;
        self.cursor_col = 0;
    }

    /// Backspace (move cursor para trás)
    pub fn backspace(&mut self) {
        self.wrap_pending = false;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    /// Tab (move para próxima tab stop)
    pub fn tab(&mut self) {
        // Respeita os tab stops (HTS/TBC alteram a lista); sem nenhum stop à
        // frente o cursor vai para a última coluna.
        self.wrap_pending = false;
        let last_col = self.cols.saturating_sub(1);
        let next = (self.cursor_col + 1..self.cols)
            .find(|&col| self.tab_stops.get(col).copied().unwrap_or(false))
            .unwrap_or(last_col);
        self.cursor_col = next.min(last_col);
    }

    /// Move o cursor para uma posição específica
    pub fn move_cursor_to(&mut self, row: usize, col: usize) {
        self.wrap_pending = false;
        let effective_row = if self.modes.origin_mode {
            // Origin mode: coordenadas são relativas à scroll region
            self.scroll_top.saturating_add(row)
        } else {
            row
        };

        // Limita à scroll region se origin mode estiver ativo
        let max_row = if self.modes.origin_mode {
            self.scroll_bottom
        } else {
            self.rows.saturating_sub(1)
        };

        self.cursor_row = effective_row.min(max_row);
        self.cursor_col = col.min(self.cols.saturating_sub(1));
    }

    /// Move o cursor para cima, parando na margem de cima se estiver na região.
    pub fn move_cursor_up(&mut self, n: usize) {
        self.wrap_pending = false;
        let limit = if self.cursor_row >= self.scroll_top {
            self.scroll_top
        } else {
            0
        };
        self.cursor_row = self.cursor_row.saturating_sub(n).max(limit);
    }

    /// Move o cursor para baixo, parando na margem de baixo se estiver na região.
    pub fn move_cursor_down(&mut self, n: usize) {
        self.wrap_pending = false;
        let limit = if self.cursor_row <= self.scroll_bottom {
            self.scroll_bottom
        } else {
            self.rows.saturating_sub(1)
        };
        self.cursor_row = (self.cursor_row + n).min(limit);
    }

    /// Move o cursor para frente (direita)
    pub fn move_cursor_forward(&mut self, n: usize) {
        self.wrap_pending = false;
        self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
    }

    /// Move o cursor para trás (esquerda)
    pub fn move_cursor_back(&mut self, n: usize) {
        self.wrap_pending = false;
        self.cursor_col = self.cursor_col.saturating_sub(n);
    }

    /// Volta `n` tab stops (CBT).
    pub fn back_tab(&mut self, n: usize) {
        self.wrap_pending = false;
        for _ in 0..n {
            self.cursor_col = (0..self.cursor_col)
                .rev()
                .find(|&col| self.tab_stops.get(col).copied().unwrap_or(false))
                .unwrap_or(0);
        }
    }

    /// Insere `n` brancos no cursor, empurrando o resto da linha (ICH).
    pub fn insert_chars(&mut self, n: usize) {
        self.wrap_pending = false;
        let blank = self.blank();
        let col = self.cursor_col.min(self.cols.saturating_sub(1));
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            let n = n.min(row.len() - col);
            row[col..].rotate_right(n);
            row[col..col + n].fill(blank);
        }
    }

    /// Apaga `n` caracteres no cursor, puxando o resto da linha (DCH).
    pub fn delete_chars(&mut self, n: usize) {
        self.wrap_pending = false;
        let blank = self.blank();
        let col = self.cursor_col.min(self.cols.saturating_sub(1));
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            let n = n.min(row.len() - col);
            row[col..].rotate_left(n);
            let len = row.len();
            row[len - n..].fill(blank);
        }
    }

    /// Troca `n` caracteres a partir do cursor por brancos (ECH).
    pub fn erase_chars(&mut self, n: usize) {
        self.wrap_pending = false;
        let blank = self.blank();
        let col = self.cursor_col.min(self.cols.saturating_sub(1));
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            let end = (col + n).min(row.len());
            row[col..end].fill(blank);
        }
    }

    /// Retorna a posição atual do cursor (row, col)
    pub fn cursor_position(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    /// Retorna as dimensões do grid (cols, rows)
    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// Retorna o número de colunas
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Retorna o número de linhas
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Retorna referência às células do grid
    pub fn get_cells(&self) -> &Vec<Vec<Cell>> {
        &self.cells
    }

    /// Retorna se o cursor deve ser mostrado
    pub fn show_cursor(&self) -> bool {
        self.modes.show_cursor
    }

    /// Rola a região uma linha para cima. Só a tela principal com a região
    /// começando no topo manda a linha para o histórico: rolagens dentro de
    /// uma região (nvim, less) ou na tela alternativa não poluem o scrollback.
    pub fn scroll_up_one(&mut self) {
        let mut line = self.cells.remove(self.scroll_top);
        if self.scroll_top == 0 && self.scroll_bottom + 1 == self.rows {
            // A tela inteira subiu: as imagens sobem junto.
            self.scrolled_total += 1;
        }
        if self.scroll_top == 0 && !self.modes.alternate_buffer && self.scrollback_limit > 0 {
            self.scrollback.push(&line, self.scrollback_limit);
            if self.scroll_offset > 0 {
                self.refresh_view();
            }
        }
        // A linha que saiu volta em branco embaixo (sem alocar outra).
        let blank = self.blank();
        line.clear();
        line.resize(self.cols, blank);
        self.cells.insert(self.scroll_bottom, line);
    }

    /// Scroll down uma linha (para sequences CSI T) - respeita scroll region
    pub fn scroll_down_one(&mut self) {
        let blank = self.blank_row();
        self.cells.insert(self.scroll_top, blank);
        self.cells.remove(self.scroll_bottom + 1);
    }

    /// Reverse Index (RI) - sobe uma linha na região de scroll
    pub fn reverse_index(&mut self) {
        self.wrap_pending = false;
        if self.cursor_row == self.scroll_top {
            // No topo da região: scroll down na região
            self.scroll_down_one();
        } else if self.cursor_row > 0 {
            // Acima do topo: move cursor para cima normalmente
            self.cursor_row -= 1;
        }
    }

    /// Retorna os limites da scroll region (top, bottom)
    pub fn scroll_region(&self) -> (usize, usize) {
        (self.scroll_top, self.scroll_bottom)
    }

    /// Define a scroll region (DECSTBM)
    /// top e bottom são 1-indexed (como no VT100), converte para 0-indexed
    /// Se top == 0, assume 1. Se bottom == 0 ou > rows, assume rows.
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        // Converte para 0-indexed
        let top = top.saturating_sub(1).min(self.rows.saturating_sub(1));
        let bottom = if bottom == 0 {
            self.rows.saturating_sub(1)
        } else {
            bottom.saturating_sub(1).min(self.rows.saturating_sub(1))
        };

        // Garante que top <= bottom e há espaço para scroll
        if top < bottom {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        } else {
            // Reset para full screen
            self.scroll_top = 0;
            self.scroll_bottom = self.rows.saturating_sub(1);
        }

        // Cursor vai para home position (1,1)
        self.cursor_row = 0;
        self.cursor_col = 0;

        tracing::trace!(
            "Scroll region set: top={}, bottom={}",
            self.scroll_top,
            self.scroll_bottom
        );
    }

    /// Reset scroll region para full screen
    pub fn reset_scroll_region(&mut self) {
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);

        // Cursor vai para home position
        self.cursor_row = 0;
        self.cursor_col = 0;

        tracing::trace!("Scroll region reset to full screen");
    }

    // === Scrollback Navigation ===

    /// Retorna o scroll offset atual
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Verifica se está visualizando scrollback
    pub fn is_in_scrollback(&self) -> bool {
        self.scroll_offset > 0
    }

    /// Scroll para cima (visualização de histórico)
    /// Retorna true se conseguiu scrollar
    pub fn scroll_up_lines(&mut self, lines: usize) -> bool {
        let max_scroll = self.scrollback.len();
        let new_offset = (self.scroll_offset + lines).min(max_scroll);

        if new_offset != self.scroll_offset {
            self.scroll_offset = new_offset;
            self.refresh_view();
            tracing::trace!("Scrolled up to offset {}", self.scroll_offset);
            true
        } else {
            false
        }
    }

    /// Scroll para baixo (voltar ao final)
    /// Retorna true se conseguiu scrollar
    pub fn scroll_down_lines(&mut self, lines: usize) -> bool {
        let new_offset = self.scroll_offset.saturating_sub(lines);

        if new_offset != self.scroll_offset {
            self.scroll_offset = new_offset;
            self.refresh_view();
            tracing::trace!("Scrolled down to offset {}", self.scroll_offset);
            true
        } else {
            false
        }
    }

    /// Scroll para o topo do scrollback
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = self.scrollback.len();
        self.refresh_view();
        tracing::trace!("Scrolled to top, offset {}", self.scroll_offset);
    }

    /// Scroll para o final (sai do scrollback)
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.view.clear();
        tracing::trace!("Scrolled to bottom");
    }

    /// Scroll para uma linha específica do scrollback
    /// line: índice da linha no scrollback (0 = mais antiga)
    pub fn scroll_to_line(&mut self, line: usize) {
        if line < self.scrollback.len() {
            self.scroll_offset = self.scrollback.len() - line;
            self.refresh_view();
            tracing::trace!("Scrolled to line {}, offset {}", line, self.scroll_offset);
        }
    }

    /// Retorna o tamanho do scrollback
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Linhas do histórico (a mais antiga primeiro), compactadas: abra com
    /// [`History::get`] ou [`History::iter`].
    pub fn scrollback_lines(&self) -> &History {
        &self.scrollback
    }

    /// Reabre as linhas do histórico que a rolagem atual mostra (mesma conta
    /// de [`Grid::visible_lines`]). Barato fora do histórico: só esvazia.
    fn refresh_view(&mut self) {
        if self.scroll_offset == 0 {
            self.view.clear();
            return;
        }
        let start = self.scrollback.len().saturating_sub(self.scroll_offset);
        let end = (start + self.rows).min(self.scrollback.len());
        let count = end.saturating_sub(start);
        self.view.truncate(count);
        self.view.resize_with(count, Vec::new);
        for (slot, index) in self.view.iter_mut().zip(start..end) {
            self.scrollback.decode_into(index, slot);
        }
    }

    /// Tela principal guardada enquanto a alternativa está ativa: linhas,
    /// histórico e cursor. `None` fora da tela alternativa.
    pub fn saved_main_screen(&self) -> Option<(&[Vec<Cell>], &History, (usize, usize))> {
        Some((
            self.main_buffer_cells.as_deref()?,
            self.main_buffer_scrollback.as_ref()?,
            self.main_cursor?,
        ))
    }

    /// Retorna o limite de scrollback
    pub fn scrollback_limit(&self) -> usize {
        self.scrollback_limit
    }

    /// Retorna as linhas visíveis considerando o scroll offset
    /// Usado pelo renderer para mostrar o conteúdo correto
    pub fn visible_lines(&self) -> Vec<&Vec<Cell>> {
        let mut lines = Vec::with_capacity(self.rows);

        // Linhas do scrollback visíveis (do histórico), já abertas.
        if self.scroll_offset > 0 {
            lines.extend(self.view.iter());
        }

        // Preenche o resto com linhas atuais do grid
        let remaining = self.rows.saturating_sub(lines.len());
        for i in 0..remaining {
            if i < self.cells.len() {
                lines.push(&self.cells[i]);
            }
        }

        lines
    }

    // === Search Highlighting ===

    /// Limpa todos os highlights de busca
    pub fn clear_highlights(&mut self) {
        for row in &mut self.cells {
            for cell in row {
                cell.highlight = false;
            }
        }
        self.scrollback.clear_highlights();
        self.refresh_view();
    }

    /// Aplica highlights em uma faixa de células
    pub fn set_highlight(&mut self, line: usize, start_col: usize, end_col: usize) {
        if line < self.scrollback.len() {
            self.scrollback.set_highlight(line, start_col, end_col);
            self.refresh_view();
        } else {
            let visible_line = line - self.scrollback.len();
            if visible_line < self.cells.len() {
                for col in start_col..end_col.min(self.cells[visible_line].len()) {
                    self.cells[visible_line][col].highlight = true;
                }
            }
        }
    }

    /// Aplica múltiplos highlights de uma vez
    pub fn apply_highlights(&mut self, matches: &[(usize, usize, usize)]) {
        self.clear_highlights();
        for &(line, start, end) in matches {
            self.set_highlight(line, start, end);
        }
    }

    /// Aplica highlights baseado em SearchMatch do search_bar
    pub fn apply_search_highlights(&mut self, matches: &[SearchMatch]) {
        self.clear_highlights();
        for m in matches {
            self.set_highlight(m.line, m.start_col, m.end_col);
        }
    }

    /// Retorna true se uma célula tem highlight
    pub fn is_highlighted(&self, line: usize, col: usize) -> bool {
        if line < self.scrollback.len() {
            return self.scrollback.is_highlighted(line, col);
        } else {
            let visible_line = line - self.scrollback.len();
            if visible_line < self.cells.len() && col < self.cells[visible_line].len() {
                return self.cells[visible_line][col].highlight;
            }
        }
        false
    }

    /// Insert Line (IL) - ESC [ n L
    /// Insere n linhas em branco na posição do cursor, empurrando para baixo
    /// Só funciona dentro da scroll region
    pub fn insert_lines(&mut self, n: usize) {
        if n == 0 || self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }
        self.wrap_pending = false;
        self.cursor_col = 0;

        let n = n.min(self.scroll_bottom.saturating_sub(self.cursor_row) + 1);

        for _ in 0..n {
            let blank = self.blank_row();
            self.cells.insert(self.cursor_row, blank);
            // Remove do bottom da região para manter tamanho
            if self.cells.len() > self.rows {
                self.cells.remove(self.scroll_bottom + 1);
            }
        }
    }

    /// Delete Line (DL) - ESC [ n M
    /// Deleta n linhas na posição do cursor, puxando de cima
    /// Só funciona dentro da scroll region
    pub fn delete_lines(&mut self, n: usize) {
        if n == 0 || self.cursor_row < self.scroll_top || self.cursor_row > self.scroll_bottom {
            return;
        }
        self.wrap_pending = false;
        self.cursor_col = 0;

        let n = n.min(self.scroll_bottom.saturating_sub(self.cursor_row) + 1);

        for _ in 0..n {
            if self.cursor_row < self.cells.len() {
                self.cells.remove(self.cursor_row);
                // Insere linha em branco no bottom da região
                let blank = self.blank_row();
                self.cells.insert(self.scroll_bottom, blank);
            }
        }
    }

    /// Redimensiona o grid
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        if new_cols == 0 || new_rows == 0 {
            return;
        }

        self.cells
            .resize_with(new_rows, || vec![Cell::default(); new_cols]);

        for row in &mut self.cells {
            row.resize(new_cols, Cell::default());
        }

        // Atualiza tab stops
        if new_cols != self.cols {
            let mut new_tab_stops = vec![false; new_cols];
            for i in (0..new_cols).step_by(8) {
                new_tab_stops[i] = true;
            }
            self.tab_stops = new_tab_stops;
        }

        self.cols = new_cols;
        self.rows = new_rows;
        self.wrap_pending = false;
        self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));

        // Reset scroll region on resize (DECCOLM behavior)
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);
        self.refresh_view();
    }

    // === Erase Functions ===

    /// Limpa toda a tela
    pub fn erase_all(&mut self) {
        self.clear_visible_images();
        let blank = self.blank();
        for row in &mut self.cells {
            row.fill(blank);
        }
    }

    /// Limpa desde o cursor até o final da tela
    pub fn erase_from_cursor_to_end(&mut self) {
        self.erase_line_from_cursor();
        let blank = self.blank();
        for row in self.cells.iter_mut().skip(self.cursor_row + 1) {
            row.fill(blank);
        }
    }

    /// Limpa desde o início da tela até o cursor
    pub fn erase_from_start_to_cursor(&mut self) {
        let blank = self.blank();
        for row in self.cells.iter_mut().take(self.cursor_row) {
            row.fill(blank);
        }
        self.erase_line_to_cursor();
    }

    /// Limpa o scrollback buffer
    pub fn erase_scrollback(&mut self) {
        let top = self.scrolled_total;
        self.images
            .retain(|image| image.alt || image.abs_row >= top);
        self.scrollback.clear();
        self.refresh_view();
    }

    /// Limpa a linha inteira onde o cursor está
    pub fn erase_line_all(&mut self) {
        let blank = self.blank();
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            row.fill(blank);
        }
    }

    /// Limpa da posição do cursor até o final da linha
    pub fn erase_line_from_cursor(&mut self) {
        self.wrap_pending = false;
        let blank = self.blank();
        let col = self.cursor_col;
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            if col < row.len() {
                row[col..].fill(blank);
            }
        }
    }

    /// Limpa do início da linha até a posição do cursor
    pub fn erase_line_to_cursor(&mut self) {
        let blank = self.blank();
        let col = self.cursor_col;
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            let end = (col + 1).min(row.len());
            row[..end].fill(blank);
        }
    }

    // === Tab Stop Functions ===

    /// Limpa o tab stop na coluna atual
    pub fn clear_tab_stop(&mut self) {
        if self.cursor_col < self.tab_stops.len() {
            self.tab_stops[self.cursor_col] = false;
        }
    }

    /// Limpa todos os tab stops
    pub fn clear_all_tab_stops(&mut self) {
        self.tab_stops.fill(false);
    }

    /// Verifica se uma célula está vazia (para testes)
    #[cfg(test)]
    pub fn is_cell_empty(&self, row: usize, col: usize) -> bool {
        if row >= self.rows || col >= self.cols {
            return true;
        }
        self.cells[row][col].char == '\0' || self.cells[row][col].char == ' '
    }

    /// Verifica se o grid foi limpo (para testes)
    #[cfg(test)]
    pub fn is_cleared(&self) -> bool {
        self.cells
            .iter()
            .all(|row| row.iter().all(|cell| cell.char == '\0' || cell.char == ' '))
    }

    // === Selection Methods ===

    /// Verifica se uma célula está selecionada
    pub fn is_cell_selected(&self, row: usize, col: usize) -> bool {
        if !self.selection.active {
            return false;
        }
        self.is_position_in_selection(row, col)
    }

    /// Verifica se uma posição está dentro da seleção
    fn is_position_in_selection(&self, row: usize, col: usize) -> bool {
        let (start_row, start_col) = self.selection.start;
        let (end_row, end_col) = self.selection.end;

        // Ordena para garantir que start <= end
        let (min_row, min_col, max_row, max_col) =
            if start_row < end_row || (start_row == end_row && start_col <= end_col) {
                (start_row, start_col, end_row, end_col)
            } else {
                (end_row, end_col, start_row, start_col)
            };

        // Verifica se a posição está dentro do range
        if row < min_row || row > max_row {
            return false;
        }

        if row == min_row && row == max_row {
            // Mesma linha
            col >= min_col && col <= max_col
        } else if row == min_row {
            // Linha inicial
            col >= min_col
        } else if row == max_row {
            // Linha final
            col <= max_col
        } else {
            // Linha do meio
            true
        }
    }

    /// Inicia seleção
    pub fn start_selection(&mut self, row: usize, col: usize, kind: SelectionKind) {
        let clamped_row = row.min(self.rows.saturating_sub(1));
        let clamped_col = col.min(self.cols.saturating_sub(1));

        self.selection = Selection {
            start: (clamped_row, clamped_col),
            end: (clamped_row, clamped_col),
            active: true,
            kind,
        };
        self.update_selection_cells();
    }

    /// Atualiza fim da seleção
    pub fn update_selection(&mut self, row: usize, col: usize) {
        if !self.selection.active {
            return;
        }

        let clamped_row = row.min(self.rows.saturating_sub(1));
        let clamped_col = col.min(self.cols.saturating_sub(1));

        self.selection.end = (clamped_row, clamped_col);

        // Ajusta seleção baseada no tipo
        match self.selection.kind {
            SelectionKind::Line => {
                // Seleciona linhas completas
                let start_row = self.selection.start.0.min(clamped_row);
                let end_row = self.selection.start.0.max(clamped_row);
                self.selection.start = (start_row, 0);
                self.selection.end = (end_row, self.cols.saturating_sub(1));
            }
            SelectionKind::Word => {
                // Expande para palavras completas (implementação básica)
                self.expand_selection_to_words();
            }
            SelectionKind::Normal => {}
        }

        self.update_selection_cells();
    }

    /// Expande seleção para palavras completas
    fn expand_selection_to_words(&mut self) {
        // Encontra início da palavra no start
        let (start_row, start_col) = self.selection.start;
        let new_start_col = self.find_word_start(start_row, start_col);

        // Encontra fim da palavra no end
        let (end_row, end_col) = self.selection.end;
        let new_end_col = self.find_word_end(end_row, end_col);

        self.selection.start = (start_row, new_start_col);
        self.selection.end = (end_row, new_end_col);
    }

    /// Encontra início de uma palavra
    fn find_word_start(&self, row: usize, col: usize) -> usize {
        if row >= self.rows {
            return col;
        }

        let line = &self.cells[row];
        let mut pos = col.min(line.len().saturating_sub(1));

        // Anda para trás até encontrar separador ou início
        while pos > 0 && !self.is_word_separator(line[pos].char) {
            pos -= 1;
        }

        // Se parou em um separador, avança
        if self.is_word_separator(line[pos].char) && pos < col {
            pos += 1;
        }

        pos
    }

    /// Encontra fim de uma palavra
    fn find_word_end(&self, row: usize, col: usize) -> usize {
        if row >= self.rows {
            return col;
        }

        let line = &self.cells[row];
        let mut pos = col;

        // Anda para frente até encontrar separador ou fim
        while pos < line.len().saturating_sub(1) && !self.is_word_separator(line[pos].char) {
            pos += 1;
        }

        // Se parou em um separador, volta um
        if self.is_word_separator(line[pos].char) && pos > col {
            pos = pos.saturating_sub(1);
        }

        pos
    }

    /// Verifica se um caractere é separador de palavra
    fn is_word_separator(&self, c: char) -> bool {
        c.is_whitespace()
            || [
                ',', '.', ';', ':', '!', '?', '(', ')', '[', ']', '{', '}', '<', '>', '"', '\'',
                '`', '|', '&',
            ]
            .contains(&c)
    }

    /// Atualiza células marcadas como selecionadas
    fn update_selection_cells(&mut self) {
        // Limpa seleção anterior
        for row in &mut self.cells {
            for cell in row {
                cell.selected = false;
            }
        }

        if !self.selection.active {
            return;
        }

        // Marca novas células selecionadas
        for row in 0..self.rows {
            for col in 0..self.cols {
                if self.is_position_in_selection(row, col) {
                    self.cells[row][col].selected = true;
                }
            }
        }
    }

    /// Finaliza seleção
    pub fn end_selection(&mut self) {
        // Mantém a seleção ativa mas marca como finalizada
        // A seleção permanece visível até o usuário clicar em outro lugar
    }

    /// Cancela seleção
    pub fn clear_selection(&mut self) {
        self.selection.active = false;
        self.update_selection_cells();
    }

    /// Retorna texto selecionado
    pub fn get_selected_text(&self) -> String {
        if !self.selection.active {
            return String::new();
        }

        let (start_row, start_col) = self.selection.start;
        let (end_row, end_col) = self.selection.end;

        // Ordena para garantir que start <= end
        let (min_row, min_col, max_row, max_col) =
            if start_row < end_row || (start_row == end_row && start_col <= end_col) {
                (start_row, start_col, end_row, end_col)
            } else {
                (end_row, end_col, start_row, start_col)
            };

        let mut result = String::new();

        for row in min_row..=max_row {
            if row >= self.rows {
                break;
            }

            let start = if row == min_row { min_col } else { 0 };
            let end = if row == max_row {
                max_col
            } else {
                self.cols.saturating_sub(1)
            };

            for col in start..=end {
                if col < self.cols {
                    let cell = &self.cells[row][col];
                    if cell.char != '\0' && !cell.spacer {
                        result.push(cell.char);
                    }
                }
            }

            // Adiciona newline se não for a última linha
            if row < max_row {
                result.push('\n');
            }
        }

        result
    }

    /// Retorna referência à seleção atual
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Verifica se há seleção ativa
    pub fn has_selection(&self) -> bool {
        self.selection.active
    }

    /// Seleciona uma palavra na posição (double-click)
    pub fn select_word_at(&mut self, row: usize, col: usize) {
        if row >= self.rows || col >= self.cols {
            return;
        }

        let word_start = self.find_word_start(row, col);
        let word_end = self.find_word_end(row, col);

        self.selection = Selection {
            start: (row, word_start),
            end: (row, word_end),
            active: true,
            kind: SelectionKind::Word,
        };
        self.update_selection_cells();
    }

    /// Seleciona uma linha na posição (triple-click)
    pub fn select_line_at(&mut self, row: usize) {
        if row >= self.rows {
            return;
        }

        self.selection = Selection {
            start: (row, 0),
            end: (row, self.cols.saturating_sub(1)),
            active: true,
            kind: SelectionKind::Line,
        };
        self.update_selection_cells();
    }

    /// Seleciona todo o texto
    pub fn select_all(&mut self) {
        self.selection = Selection {
            start: (0, 0),
            end: (self.rows.saturating_sub(1), self.cols.saturating_sub(1)),
            active: true,
            kind: SelectionKind::Normal,
        };
        self.update_selection_cells();
    }

    // === Image Placement Methods ===

    /// Adiciona um placement de imagem
    pub fn add_image_placement(&mut self, placement: ImagePlacement) {
        tracing::debug!(
            "Added image placement: id={:?}, row={}, col={}",
            placement.image_id,
            placement.row,
            placement.col
        );
        self.image_placements.push(placement);
    }

    /// Remove um placement de imagem por ID
    pub fn remove_image_placement(&mut self, image_id: ImageId) {
        let initial_len = self.image_placements.len();
        self.image_placements.retain(|p| p.image_id != image_id);

        if self.image_placements.len() < initial_len {
            tracing::debug!(
                "Removed {} placements for image {:?}",
                initial_len - self.image_placements.len(),
                image_id
            );
        }
    }

    /// Limpa todos os placements de imagem
    pub fn clear_image_placements(&mut self) {
        self.image_placements.clear();
        tracing::debug!("Cleared all image placements");
    }

    /// Retorna referência aos placements de imagem
    pub fn image_placements(&self) -> &[ImagePlacement] {
        &self.image_placements
    }

    /// Retorna referência mutável aos placements de imagem
    pub fn image_placements_mut(&mut self) -> &mut Vec<ImagePlacement> {
        &mut self.image_placements
    }

    /// Retorna placements em uma posição específica
    pub fn get_placements_at(&self, row: usize, col: usize) -> Vec<&ImagePlacement> {
        self.image_placements
            .iter()
            .filter(|p| {
                row >= p.row
                    && row < p.row + p.height_cells
                    && col >= p.col
                    && col < p.col + p.width_cells
            })
            .collect()
    }

    /// Atualiza z-index de um placement
    pub fn update_placement_z_index(&mut self, image_id: ImageId, z_index: i32) {
        for placement in &mut self.image_placements {
            if placement.image_id == image_id {
                placement.z_index = z_index;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_creation() {
        let grid = Grid::new(80, 24);
        assert_eq!(grid.size(), (80, 24));
    }

    #[test]
    fn test_write_and_read() {
        let mut grid = Grid::new(80, 24);
        grid.write_char('A');
        grid.write_char('B');
        grid.write_char('C');

        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'A');
        assert_eq!(cells[0][1].char, 'B');
        assert_eq!(cells[0][2].char, 'C');
    }

    #[test]
    fn test_cursor_movement() {
        let mut grid = Grid::new(80, 24);

        grid.move_cursor_to(10, 20);
        assert_eq!(grid.cursor_position(), (10, 20));

        grid.move_cursor_up(5);
        assert_eq!(grid.cursor_position(), (5, 20));

        grid.move_cursor_down(3);
        assert_eq!(grid.cursor_position(), (8, 20));

        grid.move_cursor_forward(10);
        assert_eq!(grid.cursor_position(), (8, 30));

        grid.move_cursor_back(5);
        assert_eq!(grid.cursor_position(), (8, 25));
    }

    #[test]
    fn test_cursor_bounds() {
        let mut grid = Grid::new(80, 24);

        // Não deve passar dos limites
        grid.move_cursor_to(100, 100);
        assert_eq!(grid.cursor_position(), (23, 79));

        // Não deve ir abaixo de 0
        grid.move_cursor_up(1000);
        assert_eq!(grid.cursor_position().0, 0);

        grid.move_cursor_back(1000);
        assert_eq!(grid.cursor_position().1, 0);
    }

    #[test]
    fn test_erase_all() {
        let mut grid = Grid::new(80, 24);
        grid.write_char('X');
        grid.write_char('Y');
        grid.write_char('Z');

        grid.erase_all();

        assert!(grid.is_cleared());
    }

    #[test]
    fn test_erase_from_cursor() {
        let mut grid = Grid::new(80, 24);

        // Escreve em várias posições
        grid.write_char('A');
        for _ in 0..79 {
            grid.write_char('B');
        } // Preenche primeira linha
        grid.newline();
        grid.write_char('C');

        // Move para (0, 10) e apaga até o final
        grid.move_cursor_to(0, 10);
        grid.erase_from_cursor_to_end();

        // Verifica que 'A' e alguns 'B' iniciais ainda existem
        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'A');
        assert_eq!(cells[0][9].char, 'B');
        // Posição 10 em diante deve estar limpa
        assert!(grid.is_cell_empty(0, 10));
        assert!(grid.is_cell_empty(0, 79));
    }

    #[test]
    fn test_erase_line() {
        let mut grid = Grid::new(80, 24);

        // Preenche uma linha
        grid.move_cursor_to(5, 0);
        for _ in 0..80 {
            grid.write_char('X');
        }

        // Move para coluna 40 e apaga até o final
        grid.move_cursor_to(5, 40);
        grid.erase_line_from_cursor();

        // Primeiras colunas devem ter 'X'
        let cells = grid.get_cells();
        assert_eq!(cells[5][0].char, 'X');
        assert_eq!(cells[5][39].char, 'X');

        // A partir da coluna 40 deve estar limpo
        assert!(grid.is_cell_empty(5, 40));
        assert!(grid.is_cell_empty(5, 79));
    }

    #[test]
    fn test_resize() {
        let mut grid = Grid::new(80, 24);

        grid.write_char('A');
        grid.move_cursor_to(23, 79);
        grid.write_char('B');

        grid.resize(40, 12);

        assert_eq!(grid.size(), (40, 12));

        // Cursor deve estar dentro dos novos limites
        let (row, col) = grid.cursor_position();
        assert!(row < 12);
        assert!(col < 40);
    }

    #[test]
    fn test_scroll() {
        let mut grid = Grid::new(10, 5);

        // Preenche o grid
        for row in 0..5 {
            grid.move_cursor_to(row, 0);
            for _ in 0..10 {
                grid.write_char('X');
            }
        }

        // Scroll up
        grid.scroll_up_one();

        // Primeira linha deve estar limpa agora (veio do scroll)
        let cells = grid.get_cells();
        assert!(cells[4][0].char == '\0' || cells[4][0].char == ' ');
    }

    #[test]
    fn test_alternate_buffer() {
        let mut grid = Grid::new(80, 24);

        // Escreve no buffer principal
        grid.write_char('A');
        grid.write_char('B');
        grid.write_char('C');

        // Habilita alternate buffer
        grid.enable_alternate_buffer();

        assert!(grid.using_alternate_buffer());

        // Alternate deve estar limpo
        assert!(grid.is_cell_empty(0, 0));

        // Escreve no alternate
        grid.write_char('X');
        assert_eq!(grid.get_cells()[0][0].char, 'X');

        // Desabilita alternate
        grid.disable_alternate_buffer();

        assert!(!grid.using_alternate_buffer());

        // Deve restaurar ABC
        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'A');
        assert_eq!(cells[0][1].char, 'B');
        assert_eq!(cells[0][2].char, 'C');
    }

    #[test]
    fn test_cursor_visibility() {
        let mut grid = Grid::new(80, 24);

        // Padrão é visível
        assert!(grid.show_cursor());

        // Esconde
        grid.modes_mut().show_cursor = false;
        assert!(!grid.show_cursor());

        // Mostra
        grid.modes_mut().show_cursor = true;
        assert!(grid.show_cursor());
    }

    #[test]
    fn test_insert_mode() {
        let mut grid = Grid::new(10, 5);

        grid.write_char('A');
        grid.write_char('B');
        grid.write_char('C');

        // Volta para posição 1 e habilita insert mode
        grid.move_cursor_to(0, 1);
        grid.modes_mut().insert_mode = true;

        // Escreve 'X' - deve shiftar B e C
        grid.write_char('X');

        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'A');
        assert_eq!(cells[0][1].char, 'X');
        assert_eq!(cells[0][2].char, 'B');
        assert_eq!(cells[0][3].char, 'C');
    }

    #[test]
    fn test_scrollback_navigation() {
        let mut grid = Grid::new(10, 5);

        // Preenche linhas e força scroll up para criar scrollback
        for i in 0..10 {
            // Move para última linha e força scroll
            grid.move_cursor_to(grid.rows - 1, 0);
            // Escreve identificador na linha
            for _ in 0..5 {
                grid.write_char(char::from_u32('0' as u32 + (i as u32 % 10)).unwrap());
            }
            // Força scroll up (move linha para scrollback)
            grid.scroll_up_one();
        }

        // Deve ter scrollback
        assert!(!grid.scrollback.is_empty());

        // Scroll up
        let scrolled = grid.scroll_up_lines(3);
        assert!(scrolled);
        assert_eq!(grid.scroll_offset(), 3);
        assert!(grid.is_in_scrollback());

        // Scroll down
        let scrolled = grid.scroll_down_lines(1);
        assert!(scrolled);
        assert_eq!(grid.scroll_offset(), 2);

        // Scroll to top
        grid.scroll_to_top();
        assert_eq!(grid.scroll_offset(), grid.scrollback.len());

        // Scroll to bottom
        grid.scroll_to_bottom();
        assert_eq!(grid.scroll_offset(), 0);
        assert!(!grid.is_in_scrollback());
    }

    #[test]
    fn test_scrollback_auto_reset_on_new_output() {
        let mut grid = Grid::new(10, 5);

        // Gera scrollback preenchendo a tela e fazendo scroll
        for _ in 0..10 {
            grid.move_cursor_to(grid.rows - 1, 0);
            grid.write_char('X');
            grid.scroll_up_one();
        }

        // Entra em scrollback
        grid.scroll_up_lines(5);
        assert!(grid.is_in_scrollback());

        // Nova saída deve resetar scroll
        grid.newline();
        assert!(!grid.is_in_scrollback());
    }

    #[test]
    fn test_visible_lines_with_scrollback() {
        let mut grid = Grid::new(10, 5);

        // Preenche scrollback
        for i in 0..100 {
            grid.move_cursor_to(0, 0);
            grid.write_char(char::from_u32('A' as u32 + (i % 26)).unwrap());
            grid.newline();
        }

        // Sem scroll offset, visible_lines = cells
        grid.scroll_to_bottom();
        let visible = grid.visible_lines();
        assert_eq!(visible.len(), 5);

        // Com scroll offset, visible_lines inclui scrollback
        grid.scroll_up_lines(3);
        let visible = grid.visible_lines();
        assert_eq!(visible.len(), 5);
    }

    // === Search Highlight Tests ===

    #[test]
    fn test_cell_highlight_default() {
        let cell = Cell::default();
        assert!(!cell.highlight);
    }

    #[test]
    fn test_clear_highlights() {
        let mut grid = Grid::new(10, 5);

        grid.move_cursor_to(0, 0);
        grid.write_char('A');
        grid.write_char('B');
        grid.write_char('C');

        grid.set_highlight(0, 0, 3);
        assert!(grid.is_highlighted(0, 0));
        assert!(grid.is_highlighted(0, 1));
        assert!(grid.is_highlighted(0, 2));

        grid.clear_highlights();
        assert!(!grid.is_highlighted(0, 0));
        assert!(!grid.is_highlighted(0, 1));
        assert!(!grid.is_highlighted(0, 2));
    }

    #[test]
    fn test_set_highlight_visible_lines() {
        let mut grid = Grid::new(10, 5);

        grid.move_cursor_to(0, 0);
        for c in "hello".chars() {
            grid.write_char(c);
        }

        grid.set_highlight(0, 0, 5);

        assert!(grid.is_highlighted(0, 0));
        assert!(grid.is_highlighted(0, 2));
        assert!(grid.is_highlighted(0, 4));
        assert!(!grid.is_highlighted(0, 5));
    }

    #[test]
    fn test_apply_highlights_multiple() {
        let mut grid = Grid::new(10, 5);

        grid.move_cursor_to(0, 0);
        for _ in 0..5 {
            grid.write_char('X');
        }
        grid.newline();
        for _ in 0..5 {
            grid.write_char('Y');
        }
        grid.newline();
        for _ in 0..5 {
            grid.write_char('Z');
        }

        let matches = vec![(0, 0, 3), (1, 1, 4), (2, 2, 5)];
        grid.apply_highlights(&matches);

        assert!(grid.is_highlighted(0, 0));
        assert!(grid.is_highlighted(0, 2));
        assert!(grid.is_highlighted(1, 1));
        assert!(grid.is_highlighted(1, 3));
        assert!(grid.is_highlighted(2, 2));
        assert!(grid.is_highlighted(2, 4));

        assert!(!grid.is_highlighted(0, 3));
        assert!(!grid.is_highlighted(1, 0));
    }

    #[test]
    fn test_apply_search_highlights() {
        use crate::ui::search_bar::SearchMatch;

        let mut grid = Grid::new(20, 5);

        grid.move_cursor_to(0, 0);
        for c in "hello world".chars() {
            grid.write_char(c);
        }

        let matches = vec![
            SearchMatch::new(0, 0, 5, "hello".to_string()),
            SearchMatch::new(0, 6, 11, "world".to_string()),
        ];

        grid.apply_search_highlights(&matches);

        assert!(grid.is_highlighted(0, 0));
        assert!(grid.is_highlighted(0, 4));
        assert!(grid.is_highlighted(0, 6));
        assert!(grid.is_highlighted(0, 10));
        assert!(!grid.is_highlighted(0, 5));
    }

    #[test]
    fn test_highlight_out_of_bounds() {
        let mut grid = Grid::new(10, 5);

        grid.move_cursor_to(0, 0);
        grid.write_char('A');

        grid.set_highlight(0, 0, 100);

        assert!(grid.is_highlighted(0, 0));
    }

    #[test]
    fn test_highlight_scrollback_line() {
        let mut grid = Grid::new(10, 5);

        for i in 0..10 {
            grid.move_cursor_to(0, 0);
            for c in format!("line{}", i).chars() {
                grid.write_char(c);
            }
            grid.newline();
        }

        grid.set_highlight(0, 0, 4);

        assert!(grid.is_highlighted(0, 0));
        assert!(grid.is_highlighted(0, 3));
    }

    // === Selection Tests ===

    #[test]
    fn test_selection_basic() {
        let mut grid = Grid::new(80, 24);
        grid.start_selection(0, 0, SelectionKind::Normal);
        grid.update_selection(0, 5);
        grid.end_selection();

        assert!(grid.is_cell_selected(0, 2));
        assert!(!grid.is_cell_selected(1, 0));
    }

    #[test]
    fn test_selection_text() {
        let mut grid = Grid::new(80, 24);

        // Escreve "Hello"
        grid.move_cursor_to(0, 0);
        for c in "Hello".chars() {
            grid.write_char(c);
        }

        grid.start_selection(0, 0, SelectionKind::Normal);
        grid.update_selection(0, 4);

        assert_eq!(grid.get_selected_text(), "Hello");
    }

    #[test]
    fn test_selection_multiline() {
        let mut grid = Grid::new(80, 24);

        grid.move_cursor_to(0, 0);
        for c in "Line1".chars() {
            grid.write_char(c);
        }
        grid.newline();
        for c in "Line2".chars() {
            grid.write_char(c);
        }

        grid.start_selection(0, 0, SelectionKind::Normal);
        grid.update_selection(1, 4);

        assert!(grid.has_selection());
        assert!(grid.is_cell_selected(0, 0));
        assert!(grid.is_cell_selected(1, 2));
    }

    #[test]
    fn test_clear_selection() {
        let mut grid = Grid::new(80, 24);

        grid.start_selection(0, 0, SelectionKind::Normal);
        grid.update_selection(0, 5);
        assert!(grid.has_selection());

        grid.clear_selection();
        assert!(!grid.has_selection());
        assert!(!grid.is_cell_selected(0, 2));
    }

    #[test]
    fn test_select_word_at() {
        let mut grid = Grid::new(80, 24);

        grid.move_cursor_to(0, 0);
        for c in "Hello World".chars() {
            grid.write_char(c);
        }

        // Double-click no meio de "Hello"
        grid.select_word_at(0, 2);

        assert!(grid.has_selection());
        assert_eq!(grid.get_selected_text(), "Hello");
    }

    #[test]
    fn test_select_line_at() {
        let mut grid = Grid::new(80, 24);

        grid.move_cursor_to(5, 0);
        for c in "Test line".chars() {
            grid.write_char(c);
        }

        // Triple-click na linha 5
        grid.select_line_at(5);

        assert!(grid.has_selection());
        assert_eq!(grid.selection().kind, SelectionKind::Line);
    }

    #[test]
    fn test_select_all() {
        let mut grid = Grid::new(10, 5);

        grid.move_cursor_to(0, 0);
        for c in "ABCDE".chars() {
            grid.write_char(c);
        }

        grid.select_all();

        assert!(grid.has_selection());
        assert!(grid.is_cell_selected(0, 0));
        assert!(grid.is_cell_selected(4, 9));
    }

    #[test]
    fn test_cell_selected_default() {
        let cell = Cell::default();
        assert!(!cell.selected);
    }

    // === Image Placement Tests ===

    #[test]
    fn test_add_image_placement() {
        let mut grid = Grid::new(80, 24);

        let placement = ImagePlacement {
            image_id: crate::image::ImageId(1),
            row: 5,
            col: 10,
            width_cells: 20,
            height_cells: 10,
            z_index: 0,
        };

        grid.add_image_placement(placement);

        assert_eq!(grid.image_placements().len(), 1);
        assert_eq!(grid.image_placements()[0].image_id.0, 1);
        assert_eq!(grid.image_placements()[0].row, 5);
    }

    #[test]
    fn test_remove_image_placement() {
        let mut grid = Grid::new(80, 24);

        // Add placements
        for i in 1..=3u32 {
            let placement = ImagePlacement {
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

        // Remove middle one
        grid.remove_image_placement(crate::image::ImageId(2));

        assert_eq!(grid.image_placements().len(), 2);
        assert!(grid.image_placements().iter().all(|p| p.image_id.0 != 2));
    }

    #[test]
    fn test_clear_image_placements() {
        let mut grid = Grid::new(80, 24);

        for i in 1..=5u32 {
            let placement = ImagePlacement {
                image_id: crate::image::ImageId(i),
                row: i as usize,
                col: 0,
                width_cells: 10,
                height_cells: 5,
                z_index: 0,
            };
            grid.add_image_placement(placement);
        }

        assert_eq!(grid.image_placements().len(), 5);

        grid.clear_image_placements();

        assert_eq!(grid.image_placements().len(), 0);
    }

    #[test]
    fn test_get_placements_at_position() {
        let mut grid = Grid::new(80, 24);

        // Add placement at row 5, col 10, size 20x10
        let placement = ImagePlacement {
            image_id: crate::image::ImageId(1),
            row: 5,
            col: 10,
            width_cells: 20,
            height_cells: 10,
            z_index: 0,
        };
        grid.add_image_placement(placement);

        // Inside placement
        let found = grid.get_placements_at(5, 10);
        assert_eq!(found.len(), 1);

        let found = grid.get_placements_at(10, 20);
        assert_eq!(found.len(), 1);

        let found = grid.get_placements_at(14, 29);
        assert_eq!(found.len(), 1);

        // Outside placement
        let found = grid.get_placements_at(4, 10);
        assert_eq!(found.len(), 0);

        let found = grid.get_placements_at(15, 10);
        assert_eq!(found.len(), 0);

        let found = grid.get_placements_at(5, 9);
        assert_eq!(found.len(), 0);

        let found = grid.get_placements_at(5, 30);
        assert_eq!(found.len(), 0);
    }

    #[test]
    fn test_update_placement_z_index() {
        let mut grid = Grid::new(80, 24);

        let placement = ImagePlacement {
            image_id: crate::image::ImageId(1),
            row: 0,
            col: 0,
            width_cells: 10,
            height_cells: 5,
            z_index: 0,
        };
        grid.add_image_placement(placement);

        assert_eq!(grid.image_placements()[0].z_index, 0);

        grid.update_placement_z_index(crate::image::ImageId(1), 5);

        assert_eq!(grid.image_placements()[0].z_index, 5);
    }

    #[test]
    fn test_image_placements_with_alternate_buffer() {
        let mut grid = Grid::new(80, 24);

        // Add placement in main buffer
        let placement = ImagePlacement {
            image_id: crate::image::ImageId(1),
            row: 0,
            col: 0,
            width_cells: 10,
            height_cells: 5,
            z_index: 0,
        };
        grid.add_image_placement(placement);

        assert_eq!(grid.image_placements().len(), 1);

        // Switch to alternate buffer
        grid.enable_alternate_buffer();

        // Placements should be cleared in alternate buffer
        assert_eq!(grid.image_placements().len(), 0);

        // Add placement in alternate buffer
        let alt_placement = ImagePlacement {
            image_id: crate::image::ImageId(2),
            row: 5,
            col: 5,
            width_cells: 10,
            height_cells: 5,
            z_index: 0,
        };
        grid.add_image_placement(alt_placement);

        assert_eq!(grid.image_placements().len(), 1);

        // Switch back to main buffer
        grid.disable_alternate_buffer();

        // Main buffer placements should be restored
        assert_eq!(grid.image_placements().len(), 1);
        assert_eq!(grid.image_placements()[0].image_id.0, 1);
    }

    // === Histórico compacto (#1) ===

    fn row_text(row: &[Cell]) -> String {
        row.iter()
            .filter(|c| !c.spacer)
            .map(|c| c.char)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn grid_with(cols: usize, rows: usize, input: &str) -> Grid {
        let mut grid = Grid::new(cols, rows);
        crate::terminal::parser::Parser::new(&mut grid).process(input.as_bytes());
        grid
    }

    /// Rolar o histórico mostra as linhas que saíram, com cores 24-bit,
    /// atributos e caracteres largos iguais aos da tela.
    #[test]
    fn test_scrolled_view_shows_compacted_history_lines() {
        let mut input = String::new();
        for i in 0..30 {
            input.push_str(&format!(
                "\x1b[1;38;2;{i};100;200mlinha {i}\x1b[0m \x1b[48;5;{i}m日本🚀\x1b[0m\r\n"
            ));
        }
        let mut grid = grid_with(20, 5, &input);
        assert_eq!(grid.scrollback_len(), 26);
        grid.scroll_to_top();
        let visible = grid.visible_lines();
        assert_eq!(visible.len(), 5);
        assert_eq!(row_text(visible[0]), "linha 0 日本🚀");
        assert_eq!(row_text(visible[4]), "linha 4 日本🚀");
        let first = visible[0];
        assert_eq!(first[0].fg, Color::Rgb(0, 100, 200));
        assert!(first[0].attrs.bold);
        assert_eq!(first[8].char, '日');
        assert!(first[9].spacer);
        assert_eq!(first[8].bg, Color::Indexed(0));
        assert_eq!(first.len(), 20);

        // Um pouco para baixo: a janela anda junto.
        grid.scroll_down_lines(2);
        assert_eq!(row_text(grid.visible_lines()[0]), "linha 2 日本🚀");
        // Perto do fim, o resto vem da tela.
        grid.scroll_to_bottom();
        grid.scroll_up_lines(1);
        let visible = grid.visible_lines();
        assert_eq!(row_text(visible[0]), "linha 25 日本🚀");
        assert_eq!(row_text(visible[1]), "linha 26 日本🚀");
    }

    /// O histórico guardado é o mesmo que a tela tinha, célula por célula.
    #[test]
    fn test_history_lines_match_cells_that_scrolled_off() {
        let mut grid = Grid::new(12, 3);
        let mut expected = Vec::new();
        for i in 0..8u8 {
            grid.move_cursor_to(0, 0);
            grid.write(
                'a',
                Color::Rgb(i, 2, 3),
                Color::Ansi(i % 8),
                Attributes::default(),
            );
            grid.write('字', Color::Default, Color::Default, Attributes::default());
            grid.cells[0][5].highlight = true;
            expected.push(grid.get_cells()[0].clone());
            grid.scroll_up_one();
        }
        let history: Vec<Vec<Cell>> = grid.scrollback_lines().iter().collect();
        assert_eq!(history.len(), 8);
        for (got, want) in history.iter().zip(&expected) {
            assert_eq!(got.len(), want.len());
            for (a, b) in got.iter().zip(want) {
                assert_eq!(
                    (
                        a.char,
                        a.fg,
                        a.bg,
                        a.attrs,
                        a.highlight,
                        a.selected,
                        a.spacer
                    ),
                    (
                        b.char,
                        b.fg,
                        b.bg,
                        b.attrs,
                        b.highlight,
                        b.selected,
                        b.spacer
                    )
                );
            }
        }
    }

    /// A linha nova que entra embaixo é branca com o fundo de apagamento,
    /// mesmo reaproveitando a que subiu.
    #[test]
    fn test_recycled_bottom_row_is_blank_with_erase_background() {
        let mut grid = grid_with(6, 2, "\x1b[41mxxxxxx\r\nyyyyyy\x1b[44m\r\n");
        let bottom = &grid.get_cells()[1];
        assert_eq!(bottom.len(), 6);
        assert!(bottom
            .iter()
            .all(|c| c.char == ' ' && c.bg == Color::Ansi(4)));
        grid.scroll_to_top();
        assert_eq!(row_text(grid.visible_lines()[0]), "xxxxxx");
    }

    /// Destaque da busca numa linha do histórico aparece na tela rolada e
    /// some ao limpar.
    #[test]
    fn test_search_highlight_in_history_shows_when_scrolled() {
        let mut input = String::new();
        for i in 0..10 {
            input.push_str(&format!("line{i}\r\n"));
        }
        let mut grid = grid_with(10, 5, &input);
        grid.scroll_to_top();
        grid.set_highlight(1, 0, 4);
        assert!(grid.visible_lines()[1][..4].iter().all(|c| c.highlight));
        assert!(!grid.visible_lines()[1][4].highlight);
        grid.clear_highlights();
        assert!(grid.visible_lines()[1].iter().all(|c| !c.highlight));
        assert!(!grid.is_highlighted(1, 0));
    }

    /// Saída nova com a aba rolada (sem quebra de linha, ex. `CSI S`) mantém
    /// a mesma distância do fim, como antes.
    #[test]
    fn test_scrolled_view_follows_history_growth() {
        let mut input = String::new();
        for i in 0..10 {
            input.push_str(&format!("l{i}\r\n"));
        }
        let mut grid = grid_with(6, 3, &input);
        grid.scroll_up_lines(2);
        let before = row_text(grid.visible_lines()[0]);
        grid.scroll_up_one();
        let after = row_text(grid.visible_lines()[0]);
        assert_ne!(before, after);
        assert_eq!(grid.scroll_offset(), 2);
    }

    /// Tela alternativa guarda e devolve o histórico da principal; mudar o
    /// tamanho com a aba rolada refaz a janela do histórico.
    #[test]
    fn test_history_survives_alternate_screen_and_resize() {
        let mut input = String::new();
        for i in 0..20 {
            input.push_str(&format!("l{i}\r\n"));
        }
        let mut grid = grid_with(8, 4, &input);
        let len = grid.scrollback_len();
        grid.enable_alternate_buffer();
        assert_eq!(grid.scrollback_len(), 0);
        let (_, saved, _) = grid.saved_main_screen().unwrap();
        assert_eq!(saved.len(), len);
        grid.disable_alternate_buffer();
        assert_eq!(grid.scrollback_len(), len);

        grid.scroll_to_top();
        grid.resize(8, 6);
        let visible = grid.visible_lines();
        assert_eq!(visible.len(), 6);
        assert_eq!(row_text(visible[0]), "l0");
        assert_eq!(row_text(visible[5]), "l5");
        grid.erase_scrollback();
        assert_eq!(grid.scrollback_len(), 0);
        assert!(grid.visible_lines().len() <= 6);
    }

    /// O alvo da #1: aba de 160×40 com 10 mil linhas no histórico cabe em
    /// poucos MB (antes 38,4 MB só de células).
    #[test]
    fn test_ten_thousand_history_lines_use_little_memory() {
        let mut grid = Grid::new(160, 40);
        let mut parser = crate::terminal::parser::Parser::new(&mut grid);
        for i in 0..10_040 {
            parser.process(
                format!(
                    "linha {i:06} de historico com texto para ocupar boa parte da largura da tab\r\n"
                )
                .as_bytes(),
            );
        }
        drop(parser);
        assert_eq!(grid.scrollback_len(), 10_000);
        let bytes = grid.scrollback_lines().memory_bytes();
        assert!(bytes < 2 * 1024 * 1024, "{bytes} bytes");
        assert!(std::mem::size_of::<Cell>() <= 24);
    }
}
