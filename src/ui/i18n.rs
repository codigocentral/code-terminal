//! Textos da interface em inglês e português (#28).
//!
//! Cada texto é uma variante de [`Text`], declarada uma vez com as duas
//! traduções lado a lado ([`texts!`]): faltar um idioma é erro de
//! compilação. As tabelas são `match` compilados no binário (memória só
//! leitura, compartilhada); nada é carregado de arquivo. [`tr`] devolve a
//! frase no idioma em uso, que troca na hora com [`set_language`].
//!
//! A CLI usa o mesmo idioma (ajuda e erros, #32). Logs continuam em inglês.

/// Idioma da interface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    PortugueseBr,
}

impl Language {
    /// Todos os idiomas, na ordem do seletor.
    pub const ALL: &'static [Language] = &[Language::English, Language::PortugueseBr];

    /// Nome do idioma escrito nele mesmo.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::PortugueseBr => "Portugu\u{ea}s (Brasil)",
        }
    }

    /// Código usado no `config.toml`.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::PortugueseBr => "pt-BR",
        }
    }

    /// Lê um código ("pt-BR", "pt_BR.UTF-8", "en", "en_US"...).
    pub fn from_code(code: &str) -> Option<Language> {
        let code = code.trim().to_ascii_lowercase();
        if code.starts_with("pt") {
            Some(Language::PortugueseBr)
        } else if code.starts_with("en") {
            Some(Language::English)
        } else {
            None
        }
    }

    fn index(self) -> u8 {
        match self {
            Language::English => 0,
            Language::PortugueseBr => 1,
        }
    }

    fn from_index(index: u8) -> Self {
        Language::ALL
            .get(index as usize)
            .copied()
            .unwrap_or_default()
    }
}

/// Idioma do sistema: `LC_ALL`, `LC_MESSAGES`, `LANG` e, sem elas (app
/// aberto pelo Finder ou no Windows), o idioma da conta (macOS:
/// `AppleLanguages`; Windows: `GetUserDefaultUILanguage`). Inglês se não der
/// para saber.
pub fn system_language() -> Language {
    // Nos testes o idioma não depende da máquina.
    if cfg!(test) {
        return Language::English;
    }
    language_from_env(|var| std::env::var(var).ok())
        .or_else(platform_language)
        .unwrap_or_default()
}

/// Idioma pelas variáveis de ambiente: a primeira não vazia entre
/// `LC_ALL`, `LC_MESSAGES` e `LANG`. `C`/`POSIX` não dizem idioma.
fn language_from_env(get: impl Fn(&str) -> Option<String>) -> Option<Language> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|var| get(var))
        .find(|value| !value.is_empty())
        .and_then(|value| Language::from_code(&value))
}

/// Primeiro idioma da lista do `defaults read -g AppleLanguages`:
/// `( "pt-BR", "en-US" )`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn language_from_apple_languages(output: &str) -> Option<Language> {
    let first = output.split('"').nth(1).or_else(|| {
        output
            .trim_matches(|c: char| c == '(' || c == ')' || c.is_whitespace())
            .split([',', '\n'])
            .map(str::trim)
            .find(|entry| !entry.is_empty())
    })?;
    Language::from_code(first)
}

/// Idioma de um LANGID do Windows (o idioma primário são os 10 bits de
/// baixo: 0x16 português, 0x09 inglês).
#[cfg_attr(not(windows), allow(dead_code))]
fn language_from_langid(langid: u16) -> Option<Language> {
    match langid & 0x3ff {
        0x16 => Some(Language::PortugueseBr),
        0x09 => Some(Language::English),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn platform_language() -> Option<Language> {
    let output = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    language_from_apple_languages(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(windows)]
fn platform_language() -> Option<Language> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // SAFETY: função sem argumentos que só devolve o LANGID do usuário.
    language_from_langid(unsafe { GetUserDefaultUILanguage() })
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_language() -> Option<Language> {
    None
}

/// Idioma pela configuração: "auto" (ou vazio) segue o sistema.
pub fn language_from_config(value: &str) -> Language {
    match value.trim() {
        "" | "auto" => system_language(),
        code => Language::from_code(code).unwrap_or_default(),
    }
}

// Nos testes cada thread tem o seu idioma (os testes rodam em paralelo);
// no app é um só para todas as janelas.
#[cfg(not(test))]
static CURRENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[cfg(test)]
thread_local! {
    static CURRENT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// Idioma em uso.
pub fn language() -> Language {
    #[cfg(not(test))]
    let index = CURRENT.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    let index = CURRENT.with(std::cell::Cell::get);
    Language::from_index(index)
}

/// Troca o idioma: o próximo frame já sai no novo.
pub fn set_language(language: Language) {
    #[cfg(not(test))]
    CURRENT.store(language.index(), std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    CURRENT.with(|current| current.set(language.index()));
}

/// Declara os textos: `Nome => ("inglês", "português")`.
macro_rules! texts {
    ($($name:ident => ($en:expr, $pt:expr)),* $(,)?) => {
        /// Um texto da interface.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Text {
            $($name,)*
        }

        impl Text {
            /// Todos os textos (para os testes de tradução).
            pub const ALL: &'static [Text] = &[$(Text::$name,)*];
        }

        /// O texto `text` no idioma `language`.
        pub fn tr_in(language: Language, text: Text) -> &'static str {
            match language {
                Language::English => match text {
                    $(Text::$name => $en,)*
                },
                Language::PortugueseBr => match text {
                    $(Text::$name => $pt,)*
                },
            }
        }
    };
}

texts! {
    // Tela de configurações
    SettingsTitle => ("Settings", "Configura\u{e7}\u{f5}es"),
    SectionAppearance => ("Appearance", "Apar\u{ea}ncia"),
    SectionTabs => ("Tabs", "Abas"),
    SectionSsh => ("SSH", "SSH"),
    SectionLanguage => ("Language", "Idioma"),
    SectionShortcuts => ("Shortcuts", "Atalhos"),
    SectionIntegrations => ("Links and AI", "Links e IA"),
    SidebarWidthLabel => ("Sidebar width", "Largura da barra lateral"),
    SidebarMemoryLabel => ("Show memory in the sidebar", "Mostrar mem\u{f3}ria na barra lateral"),
    OpenLinksIn => ("Open links in", "Abrir links no"),
    LinksBuiltin => ("built-in browser", "navegador embutido"),
    LinksSystem => ("system browser", "navegador do sistema"),
    AiEnabledLabel => ("AI assistant", "Assistente de IA"),
    AiProviderLabel => ("AI provider", "Provedor de IA"),
    AiModelLabel => ("Model", "Modelo"),
    AiKeyEnvLabel => ("API key variable", "Vari\u{e1}vel da chave da API"),
    SectionAbout => ("About", "Sobre"),
    AboutCreator => ("Created by", "Criado por"),
    AboutPublisher => ("Published by", "Publicado por"),
    AboutLicense => ("Open source license", "Licença de código aberto"),
    Theme => ("Theme", "Tema"),
    Font => ("Font", "Fonte"),
    FontSize => ("Font size", "Tamanho da fonte"),
    Cursor => ("Cursor", "Cursor"),
    Sidebar => ("Sidebar", "Barra lateral"),
    RestoreTabs => ("Restore tabs on start", "Restaurar abas ao abrir"),
    ConfirmClose => ("Confirm before closing", "Confirmar antes de fechar"),
    PersistentByDefault => ("New tabs are persistent", "Abas novas j\u{e1} persistentes"),
    NewPersistentTab => ("New persistent tab", "Nova aba persistente"),
    Open => ("Open", "Abrir"),
    SshNewConnection => ("New connection\u{2026}", "Nova conex\u{e3}o\u{2026}"),
    SshNoHosts => ("No hosts in ~/.ssh/config", "Nenhum host no ~/.ssh/config"),
    SshConnect => ("Connect", "Conectar"),
    SshEditHost => ("Edit \u{203a}", "Editar \u{203a}"),
    SshNewHost => ("New host\u{2026}", "Novo host\u{2026}"),
    SshFieldAlias => ("Name (Host)", "Nome (Host)"),
    SshFieldHostName => ("Address (HostName)", "Endere\u{e7}o (HostName)"),
    SshFieldUserLabel => ("User", "Usu\u{e1}rio"),
    SshFieldPortLabel => ("Port", "Porta"),
    SshFieldIdentity => ("Key file (IdentityFile)", "Arquivo da chave (IdentityFile)"),
    SshSaveHost => ("Save to ~/.ssh/config", "Salvar no ~/.ssh/config"),
    SshConnectHost => ("Connect to this host", "Conectar a este host"),
    SshInvalidPort => ("Port must be a number from 1 to 65535", "A porta precisa ser um n\u{fa}mero de 1 a 65535"),
    SshInvalidAlias => (
        "The name cannot be empty, have spaces or wildcards (* ? !)",
        "O nome n\u{e3}o pode ficar vazio nem ter espa\u{e7}os ou curingas (* ? !)"
    ),
    SshAliasExists => ("There is already a host with this name", "J\u{e1} existe um host com esse nome"),
    SshInvalidValue => ("Values cannot have line breaks", "Os valores n\u{e3}o podem ter quebra de linha"),
    SshHostGone => (
        "This host is no longer in ~/.ssh/config (changed outside?)",
        "Este host n\u{e3}o est\u{e1} mais no ~/.ssh/config (mudou por fora?)"
    ),
    PaneShell => ("shell", "shell"),
    InterfaceLanguage => ("Interface language", "Idioma da interface"),
    LanguageAuto => ("Automatic (system)", "Autom\u{e1}tico (sistema)"),
    ShortcutsFilter => ("Type to filter", "Digite para filtrar"),
    ShortcutsPress => ("Press the new shortcut (Esc cancels)", "Aperte o atalho novo (Esc cancela)"),
    ShortcutsNone => ("No shortcut matches", "Nenhum atalho encontrado"),
    Version => ("Version", "Vers\u{e3}o"),
    Memory => ("Memory (this process)", "Mem\u{f3}ria (este processo)"),
    ReleaseGpuAfter => (
        "Free the GPU with no window open (server)",
        "Soltar a GPU sem janela aberta (servidor)"
    ),
    Never => ("never", "nunca"),
    ConfigFile => ("Config file", "Arquivo de configura\u{e7}\u{e3}o"),
    On => ("on", "ligado"),
    Off => ("off", "desligado"),
    Save => ("Save", "Salvar"),
    Cancel => ("Cancel", "Cancelar"),
    SettingsHint => (
        "\u{2191}\u{2193} move  \u{2190}\u{2192} change  Tab section  Enter save  Esc cancel",
        "\u{2191}\u{2193} mover  \u{2190}\u{2192} mudar  Tab se\u{e7}\u{e3}o  Enter salva  Esc cancela"
    ),
    SettingsHintShortcuts => (
        "type to filter  Enter change key  Tab section  Esc cancel",
        "digite para filtrar  Enter troca a tecla  Tab se\u{e7}\u{e3}o  Esc cancela"
    ),
    // Rodapé da sidebar
    FooterThemes => ("Themes", "Temas"),
    FooterSsh => ("SSH connections", "Conex\u{f5}es SSH"),
    FooterBrowser => ("Browser window", "Janela do navegador"),
    FooterNotifications => ("Notifications", "Avisos"),
    FooterAi => ("AI assistant", "Assistente de IA"),
    FooterSettings => ("Settings (Ctrl+,)", "Configura\u{e7}\u{f5}es (Ctrl+,)"),
    // Confirmação de fechar
    CloseTabQuestion => ("Close this tab?", "Fechar esta aba?"),
    ClosePaneQuestion => ("Close this pane?", "Fechar este painel?"),
    ConfirmCloseHint => ("Enter: close    Esc: cancel", "Enter: fechar    Esc: cancelar"),
    // Cores da tab (#24)
    TabColorMenu => ("Tab color\u{2026}", "Cor da aba\u{2026}"),
    BackgroundMenu => ("Terminal background\u{2026}", "Fundo do terminal\u{2026}"),
    Back => ("\u{2039} Back", "\u{2039} Voltar"),
    CustomColor => ("Custom\u{2026} (#rrggbb)", "Personalizada\u{2026} (#rrggbb)"),
    NoColor => ("None", "Nenhuma"),
    LowContrast => ("low contrast", "pouco contraste"),
    ApplyColor => ("Apply this color", "Aplicar esta cor"),
    HexColorPlaceholder => (
        "Type a color as #rrggbb (Enter applies, Esc cancels)",
        "Digite a cor como #rrggbb (Enter aplica, Esc cancela)"
    ),
    NoValidColor => ("Not a color yet: use #rrggbb or #rgb", "Ainda n\u{e3}o \u{e9} uma cor: use #rrggbb ou #rgb"),
    // Animação da tab (#26)
    TabAnimationLabel => ("Tab animation", "Anima\u{e7}\u{e3}o da aba"),
    AnimationSpeedLabel => ("Animation speed", "Velocidade da anima\u{e7}\u{e3}o"),
    FollowReducedMotion => ("Follow system reduced motion", "Seguir \"menos movimento\" do sistema"),
    ReplaceProgramSpinner => (
        "Terminal spinner instead of the program's",
        "Indicador do terminal no lugar do programa"
    ),
    StartMaximized => ("Open maximized", "Abrir maximizada"),
    AnimationRing => ("Color ring", "Anel colorido"),
    AnimationPulse => ("Pulse", "Pulso"),
    AnimationWave => ("Color wave", "Onda de cor"),
    AnimationSpinner => ("Spinner", "Indicador girat\u{f3}rio"),
    AnimationDot => ("Dot only", "S\u{f3} um ponto"),
    AnimationNone => ("None", "Nenhuma"),
    SpeedSlow => ("slow", "lenta"),
    SpeedNormal => ("normal", "normal"),
    SpeedFast => ("fast", "r\u{e1}pida"),
    Preview => ("Preview", "Pr\u{e9}via"),
    // Paleta de comandos
    CmdNewTab => ("New Tab", "Nova aba"),
    CmdCloseTab => ("Close Tab", "Fechar aba"),
    CmdNextTab => ("Next Tab", "Pr\u{f3}xima aba"),
    CmdPreviousTab => ("Previous Tab", "Aba anterior"),
    CmdNewWindow => ("New Window", "Nova janela"),
    CmdSettings => ("Settings", "Configura\u{e7}\u{f5}es"),
    CmdChooseTheme => ("Choose Theme\u{2026}", "Escolher tema\u{2026}"),
    CmdSplitHorizontal => ("Split Horizontal", "Dividir na horizontal"),
    CmdSplitVertical => ("Split Vertical", "Dividir na vertical"),
    CmdSearch => ("Search", "Buscar"),
    CmdToggleSidebar => ("Toggle Sidebar", "Mostrar/esconder a barra lateral"),
    CmdRenameTab => ("Rename Tab", "Renomear aba"),
    CmdJumpToUnread => ("Jump to Unread Notification", "Ir para o aviso n\u{e3}o lido"),
    CmdShowNotifications => ("Show Notifications", "Mostrar avisos"),
    CmdToggleInspector => ("Toggle Protocol Inspector", "Inspetor de protocolo"),
    CmdAiExplain => ("AI: Explain Last Output", "IA: explicar a \u{fa}ltima sa\u{ed}da"),
    CmdAiSuggest => ("AI: Suggest Command", "IA: sugerir comando"),
    CmdNewPersistentTab => (
        "New Persistent Tab (keeps running when closed)",
        "Nova aba persistente (continua rodando ao fechar)"
    ),
    CmdOpenBrowser => (
        "Open Browser Window (Ctrl+click opens links there)",
        "Abrir janela do navegador (Ctrl+clique abre links nela)"
    ),
    CmdTogglePersistent => ("Toggle Persistent Tab", "Tornar persistente / deixar de ser"),
    CmdBackgroundSessions => ("Show Background Sessions", "Sess\u{f5}es em segundo plano"),
    CmdQuickConnectSsh => ("Quick Connect SSH", "Conectar via SSH"),
    CmdToggleTheme => ("Toggle Theme", "Pr\u{f3}ximo tema"),
    CmdQuit => ("Quit", "Sair"),
    PalettePlaceholder => ("Type a command...", "Digite um comando..."),
    ThemesPlaceholder => (
        "Choose a theme (Enter applies, Esc goes back)",
        "Escolha um tema (Enter aplica, Esc volta)"
    ),
    NoMatchingCommands => ("No matching commands", "Nenhum comando encontrado"),
    CurrentMark => ("current", "atual"),
    // Menu do botão direito da tab
    MenuRename => ("Rename  (F2)", "Renomear  (F2)"),
    MenuUnnest => ("Remove from group", "Tirar do grupo"),
    MenuExpand => ("Expand group", "Expandir grupo"),
    MenuCollapse => ("Collapse group", "Recolher grupo"),
    MenuNestInto => ("Nest into", "Aninhar em"),
    MenuClose => ("Close tab", "Fechar aba"),
    // Boas-vindas
    Welcome => ("Welcome to Code Terminal", "Boas-vindas ao Code Terminal"),
    PressAnyKey => ("Press any key to close", "Aperte qualquer tecla para fechar"),
    CommandPalette => ("Command palette", "Paleta de comandos"),
    NewWindowLabel => ("New window", "Nova janela"),
    // Avisos rápidos
    PasteFailed => ("Could not paste", "N\u{e3}o foi poss\u{ed}vel colar"),
    NothingToPaste => ("No text to paste", "Nada de texto para colar"),
    CopyFailed => ("Could not copy", "N\u{e3}o foi poss\u{ed}vel copiar"),
    // Painéis
    WaitingForOutput => ("Waiting for output\u{2026}", "Esperando sa\u{ed}da\u{2026}"),
    ConnectViaSsh => ("Connect via SSH", "Conectar via SSH"),
    SshNoMatch => (
        "No saved hosts match - Enter connects to what you typed",
        "Nenhum host salvo combina - Enter conecta ao que voc\u{ea} digitou"
    ),
    SshFieldHost => ("host", "host"),
    SshFieldUser => ("user", "usu\u{e1}rio"),
    SshFieldPort => ("port", "porta"),
    SshHint => (
        "type to filter  \u{2191}\u{2193} select  Tab next field  Enter connect  Esc cancel",
        "digite para filtrar  \u{2191}\u{2193} escolher  Tab pr\u{f3}ximo campo  Enter conecta  Esc cancela"
    ),
    AiTitle => ("AI", "IA"),
    AiSuggestTitle => ("AI \u{b7} Suggest command", "IA \u{b7} Sugerir comando"),
    AiAskHint => (
        "Describe what you want to do   Enter ask   Esc cancel",
        "Descreva o que quer fazer   Enter pergunta   Esc cancela"
    ),
    Thinking => ("Thinking\u{2026}", "Pensando\u{2026}"),
    EscCancel => ("Esc cancel", "Esc cancela"),
    EscClose => ("Esc close", "Esc fecha"),
    AiInsertHint => (
        "Enter insert command (not run)   Esc close",
        "Enter insere o comando (sem rodar)   Esc fecha"
    ),
    EnterEscClose => ("Enter/Esc close", "Enter/Esc fecha"),
    NoNotifications => ("No notifications yet", "Nenhum aviso ainda"),
    // Abas persistentes (#21)
    MenuMakePersistent => ("Make persistent", "Tornar persistente"),
    MenuAutoResumeOn => ("Resume automatically: yes", "Retomar automaticamente: sim"),
    MenuAutoResumeOff => ("Resume automatically: no", "Retomar automaticamente: n\u{e3}o"),
    AutoResumeOn => (
        "After a restart, this tab resumes the program by itself",
        "Depois de reiniciar, esta aba retoma o programa sozinha"
    ),
    AutoResumeOff => (
        "After a restart, the resume command is only typed in",
        "Depois de reiniciar, o comando para retomar s\u{f3} fica digitado"
    ),
    MenuStopPersistent => ("Stop being persistent", "Deixar de ser persistente"),
    PersistentOn => ("Persistent tab: survives closing the window", "Aba persistente: sobrevive a fechar a janela"),
    PersistentOff => ("Regular tab: closes with the window", "Aba comum: fecha junto com a janela"),
    ClosePersistentQuestion => ("Close this persistent tab?", "Fechar esta aba persistente?"),
    ClosePersistentHint => (
        "Enter: end it    H: just hide    Esc: cancel",
        "Enter: encerrar    H: s\u{f3} esconder    Esc: cancelar"
    ),
    Now => ("now", "agora"),
    AiExplainTitle => ("Explain output", "Explicar a sa\u{ed}da"),
    AiSuggestCommandTitle => ("Suggest command", "Sugerir comando"),
}

/// O texto no idioma em uso.
pub fn tr(text: Text) -> &'static str {
    tr_in(language(), text)
}

/// Nome da cor pronta `index` (a ordem de `tab_colors::presets`).
pub fn preset_color_name(index: usize) -> &'static str {
    const EN: [&str; 8] = [
        "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "Orange", "Gray",
    ];
    const PT: [&str; 8] = [
        "Vermelho", "Verde", "Amarelo", "Azul", "Magenta", "Ciano", "Laranja", "Cinza",
    ];
    let names = match language() {
        Language::English => EN,
        Language::PortugueseBr => PT,
    };
    names.get(index).copied().unwrap_or("?")
}

/// Aviso de fundo com pouco contraste com o texto do tema.
pub fn low_contrast_warning(ratio: f32) -> String {
    match language() {
        Language::English => {
            format!("Low contrast with the text: {ratio:.1}:1 (4.5:1 or more is easier to read)")
        }
        Language::PortugueseBr => {
            format!("Pouco contraste com o texto: {ratio:.1}:1 (4,5:1 ou mais \u{e9} mais leg\u{ed}vel)")
        }
    }
}

/// Resumo de um grupo recolhido: "3 tabs · 1 alert".
pub fn group_summary(tabs: usize, alerts: usize) -> String {
    let (tab, tabs_word, alert, alerts_word) = match language() {
        Language::English => ("tab", "tabs", "alert", "alerts"),
        Language::PortugueseBr => ("aba", "abas", "aviso", "avisos"),
    };
    let tabs_text = if tabs == 1 {
        format!("1 {tab}")
    } else {
        format!("{tabs} {tabs_word}")
    };
    match alerts {
        0 => tabs_text,
        1 => format!("{tabs_text} \u{b7} 1 {alert}"),
        n => format!("{tabs_text} \u{b7} {n} {alerts_word}"),
    }
}

/// "Copied · 12 characters".
pub fn copied_message(chars: usize) -> String {
    match (language(), chars) {
        (Language::English, 1) => "Copied \u{b7} 1 character".to_string(),
        (Language::English, n) => format!("Copied \u{b7} {n} characters"),
        (Language::PortugueseBr, 1) => "Copiado \u{b7} 1 caractere".to_string(),
        (Language::PortugueseBr, n) => format!("Copiado \u{b7} {n} caracteres"),
    }
}

/// Programa que vai ser encerrado ao fechar.
pub fn still_running(program: &str) -> String {
    match language() {
        Language::English => {
            format!("\u{201c}{program}\u{201d} is still running and will be terminated.")
        }
        Language::PortugueseBr => {
            format!("\u{201c}{program}\u{201d} ainda est\u{e1} rodando e vai ser encerrado.")
        }
    }
}

/// Erro ao tornar (ou abrir) uma aba persistente.
pub fn persistent_failed(opening: bool, error: &str) -> String {
    match (language(), opening) {
        (Language::English, false) => format!("Could not make the tab persistent: {error}"),
        (Language::English, true) => format!("Could not open the persistent tab: {error}"),
        (Language::PortugueseBr, false) => format!("N\u{e3}o deu para tornar persistente: {error}"),
        (Language::PortugueseBr, true) => {
            format!("N\u{e3}o deu para abrir a aba persistente: {error}")
        }
    }
}

/// Quantas sessões em segundo plano voltaram para a janela.
pub fn background_sessions_restored(count: usize) -> String {
    match (language(), count) {
        (Language::English, 0) => "No background sessions".to_string(),
        (Language::English, 1) => "1 background session is back".to_string(),
        (Language::English, n) => format!("{n} background sessions are back"),
        (Language::PortugueseBr, 0) => "Nenhuma sess\u{e3}o em segundo plano".to_string(),
        (Language::PortugueseBr, 1) => "1 sess\u{e3}o em segundo plano voltou".to_string(),
        (Language::PortugueseBr, n) => format!("{n} sess\u{f5}es em segundo plano voltaram"),
    }
}

/// Texto da confirmação de fechar aba persistente.
pub fn persistent_close_detail(program: Option<&str>) -> String {
    let running = |p: &str| match language() {
        Language::English => format!("\u{201c}{p}\u{201d} is running. "),
        Language::PortugueseBr => format!("\u{201c}{p}\u{201d} est\u{e1} rodando. "),
    };
    let rest = match language() {
        Language::English => "Hiding keeps everything alive in the background.",
        Language::PortugueseBr => "Esconder mant\u{e9}m tudo vivo em segundo plano.",
    };
    format!("{}{rest}", program.map(running).unwrap_or_default())
}

/// Host gravado no `~/.ssh/config` (com o backup, se havia arquivo).
pub fn ssh_saved(backup: Option<&str>) -> String {
    match (language(), backup) {
        (Language::English, Some(b)) => format!("Saved to ~/.ssh/config (backup: {b})"),
        (Language::English, None) => "Saved to ~/.ssh/config".to_string(),
        (Language::PortugueseBr, Some(b)) => format!("Salvo no ~/.ssh/config (backup: {b})"),
        (Language::PortugueseBr, None) => "Salvo no ~/.ssh/config".to_string(),
    }
}

/// Não deu para gravar o `~/.ssh/config`.
pub fn ssh_save_failed(error: &str) -> String {
    match language() {
        Language::English => format!("Could not save ~/.ssh/config: {error}"),
        Language::PortugueseBr => format!("N\u{e3}o deu para salvar o ~/.ssh/config: {error}"),
    }
}

/// "N panes" na linha de detalhes do cartão.
pub fn panes_count(panes: usize) -> String {
    match language() {
        Language::English => format!("{panes} panes"),
        Language::PortugueseBr => format!("{panes} pain\u{e9}is"),
    }
}

/// Título do histórico de avisos.
pub fn notifications_title(unread: usize) -> String {
    match language() {
        Language::English => format!("Notifications  ({unread} unread)"),
        Language::PortugueseBr => format!("Avisos  ({unread} n\u{e3}o lidos)"),
    }
}

/// Rótulo da barra de busca.
pub fn search_label(query: &str) -> String {
    match language() {
        Language::English => format!("Search: {query}"),
        Language::PortugueseBr => format!("Buscar: {query}"),
    }
}

/// Título do inspetor de protocolo.
pub fn inspector_title(pane: u64) -> String {
    match language() {
        Language::English => format!("Protocol inspector \u{b7} pane {pane}"),
        Language::PortugueseBr => format!("Inspetor de protocolo \u{b7} painel {pane}"),
    }
}

/// Nome visível de um atalho de `config::keybindings::EDITABLE_SHORTCUTS`.
pub fn shortcut_name(id: &str) -> &'static str {
    let (en, pt) = match id {
        "new_tab" => ("New tab", "Nova aba"),
        "close_tab" => ("Close tab", "Fechar aba"),
        "next_tab" => ("Next tab", "Pr\u{f3}xima aba"),
        "previous_tab" => ("Previous tab", "Aba anterior"),
        "rename_tab" => ("Rename tab", "Renomear aba"),
        "toggle_sidebar" => ("Show/hide sidebar", "Mostrar/esconder a barra lateral"),
        "open_settings" => ("Settings", "Configura\u{e7}\u{f5}es"),
        "copy" => ("Copy selection", "Copiar a sele\u{e7}\u{e3}o"),
        "paste" => ("Paste", "Colar"),
        "search_open" => ("Search", "Buscar"),
        "search_next" => ("Next match", "Pr\u{f3}ximo resultado"),
        "search_previous" => ("Previous match", "Resultado anterior"),
        "jump_to_unread" => (
            "Jump to unread notification",
            "Ir para o aviso n\u{e3}o lido",
        ),
        "show_notifications" => ("Notifications", "Avisos"),
        "toggle_inspector" => ("Protocol inspector", "Inspetor de protocolo"),
        "ai_explain" => ("AI: explain output", "IA: explicar a sa\u{ed}da"),
        "ai_suggest" => ("AI: suggest command", "IA: sugerir comando"),
        "splits.split_horizontal" => ("Split horizontally", "Dividir na horizontal"),
        "splits.split_vertical" => ("Split vertically", "Dividir na vertical"),
        "splits.close_pane" => ("Close pane", "Fechar painel"),
        "splits.nav_up" => ("Pane above", "Painel de cima"),
        "splits.nav_down" => ("Pane below", "Painel de baixo"),
        "splits.nav_left" => ("Pane to the left", "Painel da esquerda"),
        "splits.nav_right" => ("Pane to the right", "Painel da direita"),
        "splits.next_pane" => ("Next pane", "Pr\u{f3}ximo painel"),
        "splits.prev_pane" => ("Previous pane", "Painel anterior"),
        "splits.increase_size" => ("Grow pane", "Aumentar painel"),
        "splits.decrease_size" => ("Shrink pane", "Diminuir painel"),
        _ => ("?", "?"),
    };
    match language() {
        Language::English => en,
        Language::PortugueseBr => pt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #32: idioma pelas variáveis, depois pelo sistema (macOS/Windows).
    #[test]
    fn test_system_language_sources() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |var: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == var)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            language_from_env(env(&[("LANG", "pt_BR.UTF-8")])),
            Some(Language::PortugueseBr)
        );
        assert_eq!(
            language_from_env(env(&[("LC_ALL", "en_US.UTF-8"), ("LANG", "pt_BR.UTF-8")])),
            Some(Language::English)
        );
        // Vazia não conta; C/POSIX não dizem idioma (vale o do sistema).
        assert_eq!(
            language_from_env(env(&[("LC_ALL", ""), ("LANG", "pt_PT")])),
            Some(Language::PortugueseBr)
        );
        assert_eq!(language_from_env(env(&[("LANG", "C.UTF-8")])), None);
        assert_eq!(language_from_env(env(&[])), None);

        assert_eq!(
            language_from_apple_languages("(\n    \"pt-BR\",\n    \"en-US\"\n)\n"),
            Some(Language::PortugueseBr)
        );
        assert_eq!(
            language_from_apple_languages("(\n    \"en-GB\"\n)"),
            Some(Language::English)
        );
        assert_eq!(language_from_apple_languages("(\n    fr,\n    pt\n)"), None);
        assert_eq!(language_from_apple_languages(""), None);

        assert_eq!(language_from_langid(0x0416), Some(Language::PortugueseBr)); // pt-BR
        assert_eq!(language_from_langid(0x0816), Some(Language::PortugueseBr)); // pt-PT
        assert_eq!(language_from_langid(0x0409), Some(Language::English));
        assert_eq!(language_from_langid(0x040c), None); // fr-FR
    }

    /// Nenhum texto vazio em nenhum idioma; a maioria muda de um para o
    /// outro (os iguais são nomes próprios ou siglas).
    #[test]
    fn every_text_exists_in_both_languages() {
        let mut same = Vec::new();
        for &text in Text::ALL {
            let (en, pt) = (
                tr_in(Language::English, text),
                tr_in(Language::PortugueseBr, text),
            );
            assert!(!en.trim().is_empty(), "{text:?} sem inglês");
            assert!(!pt.trim().is_empty(), "{text:?} sem português");
            if en == pt {
                same.push(text);
            }
        }
        let allowed = [
            Text::SectionSsh,
            Text::Cursor,
            Text::SshFieldHost,
            Text::SpeedNormal,
            Text::PaneShell,
        ];
        let unexpected: Vec<_> = same.iter().filter(|t| !allowed.contains(t)).collect();
        assert!(unexpected.is_empty(), "sem tradução: {unexpected:?}");
    }

    #[test]
    fn every_editable_shortcut_has_a_name() {
        for &lang in Language::ALL {
            set_language(lang);
            for id in crate::config::keybindings::EDITABLE_SHORTCUTS {
                assert_ne!(shortcut_name(id), "?", "{lang:?} {id}");
            }
        }
        set_language(Language::English);
    }

    #[test]
    fn switching_language_changes_texts_at_once() {
        set_language(Language::PortugueseBr);
        assert_eq!(tr(Text::Save), "Salvar");
        assert_eq!(group_summary(3, 1), "3 abas \u{b7} 1 aviso");
        assert_eq!(copied_message(2), "Copiado \u{b7} 2 caracteres");
        assert_eq!(preset_color_name(0), "Vermelho");
        set_language(Language::English);
        assert_eq!(tr(Text::Save), "Save");
        assert_eq!(group_summary(1, 0), "1 tab");
        assert_eq!(group_summary(2, 5), "2 tabs \u{b7} 5 alerts");
        assert_eq!(copied_message(1), "Copied \u{b7} 1 character");
    }

    #[test]
    fn language_codes_and_config_values() {
        assert_eq!(
            Language::from_code("pt_BR.UTF-8"),
            Some(Language::PortugueseBr)
        );
        assert_eq!(Language::from_code("pt-BR"), Some(Language::PortugueseBr));
        assert_eq!(Language::from_code("en_US.UTF-8"), Some(Language::English));
        assert_eq!(Language::from_code("de_DE"), None);
        assert_eq!(language_from_config("pt-BR"), Language::PortugueseBr);
        assert_eq!(language_from_config("en"), Language::English);
        assert_eq!(language_from_config("xx"), Language::English);
        for &lang in Language::ALL {
            assert_eq!(Language::from_code(lang.code()), Some(lang));
            assert!(!lang.native_name().is_empty());
        }
    }
}
