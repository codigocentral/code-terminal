use super::config::SidebarConfig;
use super::SidebarVisibility;

/// Componente de toggle da sidebar
#[derive(Debug)]
pub struct SidebarToggle {
    animation_duration_ms: u64,
    keyboard_shortcut: Option<String>,
    show_button: bool,
    button_position: ToggleButtonPosition,
    auto_hide_delay_ms: Option<u64>,
}

/// Posição do botão de toggle
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToggleButtonPosition {
    /// Dentro da sidebar, no topo
    InsideTop,
    /// Dentro da sidebar, no centro
    InsideCenter,
    /// Fora da sidebar (quando oculta)
    Outside,
    /// Flutuante
    Floating,
}

impl SidebarToggle {
    /// Cria um novo toggle
    pub fn new() -> Self {
        Self {
            animation_duration_ms: 200,
            keyboard_shortcut: Some("Ctrl+B".to_string()),
            show_button: true,
            button_position: ToggleButtonPosition::InsideTop,
            auto_hide_delay_ms: None,
        }
    }

    /// Cria com configuração
    pub fn with_config(_config: &SidebarConfig) -> Self {
        Self {
            animation_duration_ms: 200,
            keyboard_shortcut: Some("Ctrl+B".to_string()),
            show_button: true,
            button_position: ToggleButtonPosition::InsideTop,
            auto_hide_delay_ms: None,
        }
    }

    /// Alterna a visibilidade
    pub fn toggle(&self, current: SidebarVisibility) -> SidebarVisibility {
        match current {
            SidebarVisibility::Visible => SidebarVisibility::Hidden,
            SidebarVisibility::Hidden => SidebarVisibility::Visible,
            SidebarVisibility::Compact => SidebarVisibility::Visible,
        }
    }

    /// Alterna para modo compacto
    pub fn toggle_compact(&self, current: SidebarVisibility) -> SidebarVisibility {
        match current {
            SidebarVisibility::Visible => SidebarVisibility::Compact,
            SidebarVisibility::Hidden => SidebarVisibility::Compact,
            SidebarVisibility::Compact => SidebarVisibility::Visible,
        }
    }

    /// Define o estado diretamente
    pub fn set_state(&self, state: SidebarVisibility) -> SidebarVisibility {
        state
    }

    /// Mostra a sidebar
    pub fn show(&self) -> SidebarVisibility {
        SidebarVisibility::Visible
    }

    /// Esconde a sidebar
    pub fn hide(&self) -> SidebarVisibility {
        SidebarVisibility::Hidden
    }

    /// Coloca em modo compacto
    pub fn compact(&self) -> SidebarVisibility {
        SidebarVisibility::Compact
    }

    /// Define a duração da animação
    pub fn set_animation_duration(&mut self, ms: u64) {
        self.animation_duration_ms = ms;
    }

    /// Retorna a duração da animação
    pub fn animation_duration_ms(&self) -> u64 {
        self.animation_duration_ms
    }

    /// Define o atalho de teclado
    pub fn set_keyboard_shortcut(&mut self, shortcut: Option<String>) {
        self.keyboard_shortcut = shortcut;
    }

    /// Retorna o atalho de teclado
    pub fn keyboard_shortcut(&self) -> Option<&str> {
        self.keyboard_shortcut.as_deref()
    }

    /// Define se mostra o botão
    pub fn set_show_button(&mut self, show: bool) {
        self.show_button = show;
    }

    /// Verifica se mostra o botão
    pub fn show_button(&self) -> bool {
        self.show_button
    }

    /// Define a posição do botão
    pub fn set_button_position(&mut self, position: ToggleButtonPosition) {
        self.button_position = position;
    }

    /// Retorna a posição do botão
    pub fn button_position(&self) -> ToggleButtonPosition {
        self.button_position
    }

    /// Define auto-hide
    pub fn set_auto_hide(&mut self, delay_ms: Option<u64>) {
        self.auto_hide_delay_ms = delay_ms;
    }

    /// Retorna delay de auto-hide
    pub fn auto_hide_delay_ms(&self) -> Option<u64> {
        self.auto_hide_delay_ms
    }

    /// Verifica se auto-hide está habilitado
    pub fn auto_hide_enabled(&self) -> bool {
        self.auto_hide_delay_ms.is_some()
    }

    /// Retorna ícone apropriado para o estado
    pub fn icon_for_state(state: SidebarVisibility) -> &'static str {
        match state {
            SidebarVisibility::Visible => "◀",
            SidebarVisibility::Hidden => "▶",
            SidebarVisibility::Compact => "◀",
        }
    }

    /// Retorna tooltip para o estado
    pub fn tooltip_for_state(state: SidebarVisibility) -> &'static str {
        match state {
            SidebarVisibility::Visible => "Hide Sidebar",
            SidebarVisibility::Hidden => "Show Sidebar",
            SidebarVisibility::Compact => "Expand Sidebar",
        }
    }

    /// Verifica se um atalho corresponde
    pub fn matches_shortcut(&self, modifiers: &str, key: &str) -> bool {
        if let Some(ref shortcut) = self.keyboard_shortcut {
            let normalized = format!("{}+{}", modifiers, key).to_lowercase();
            shortcut.to_lowercase() == normalized
        } else {
            false
        }
    }

    /// Cicla pelos estados: Visible -> Compact -> Hidden -> Visible
    pub fn cycle_states(&self, current: SidebarVisibility) -> SidebarVisibility {
        match current {
            SidebarVisibility::Visible => SidebarVisibility::Compact,
            SidebarVisibility::Compact => SidebarVisibility::Hidden,
            SidebarVisibility::Hidden => SidebarVisibility::Visible,
        }
    }

    /// Calcula a largura alvo baseada no estado
    pub fn target_width(&self, state: SidebarVisibility, config: &SidebarConfig) -> u32 {
        match state {
            SidebarVisibility::Visible => config.width,
            SidebarVisibility::Hidden => 0,
            SidebarVisibility::Compact => config.compact_width,
        }
    }

    /// Verifica se precisa de animação
    pub fn needs_animation(&self, from: SidebarVisibility, to: SidebarVisibility) -> bool {
        from != to && self.animation_duration_ms > 0
    }

    /// Calcula a largura intermediária para animação
    pub fn animated_width(
        &self,
        from_width: u32,
        to_width: u32,
        progress: f32, // 0.0 a 1.0
    ) -> u32 {
        let from = from_width as f32;
        let to = to_width as f32;
        let current = from + (to - from) * progress.clamp(0.0, 1.0);
        current.round() as u32
    }
}

impl Default for SidebarToggle {
    fn default() -> Self {
        Self::new()
    }
}

/// Estado da animação
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationState {
    Idle,
    Expanding,
    Collapsing,
    Compactifying,
}

/// Gerenciador de animação
#[derive(Debug)]
pub struct ToggleAnimation {
    state: AnimationState,
    start_width: u32,
    target_width: u32,
    progress: f32,
    duration_ms: u64,
    start_time: Option<std::time::Instant>,
}

impl ToggleAnimation {
    /// Cria nova animação
    pub fn new(duration_ms: u64) -> Self {
        Self {
            state: AnimationState::Idle,
            start_width: 0,
            target_width: 0,
            progress: 0.0,
            duration_ms,
            start_time: None,
        }
    }

    /// Inicia animação
    pub fn start(&mut self, from: u32, to: u32, state: AnimationState) {
        self.state = state;
        self.start_width = from;
        self.target_width = to;
        self.progress = 0.0;
        self.start_time = Some(std::time::Instant::now());
    }

    /// Atualiza animação
    pub fn update(&mut self) -> Option<u32> {
        if self.state == AnimationState::Idle {
            return None;
        }

        let elapsed = self
            .start_time
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);

        if elapsed >= self.duration_ms {
            self.state = AnimationState::Idle;
            return Some(self.target_width);
        }

        self.progress = elapsed as f32 / self.duration_ms as f32;

        // Easing function (ease-out)
        let eased = 1.0 - (1.0 - self.progress).powi(3);

        let width = self.start_width as f32 + (self.target_width - self.start_width) as f32 * eased;
        Some(width.round() as u32)
    }

    /// Verifica se está animando
    pub fn is_animating(&self) -> bool {
        self.state != AnimationState::Idle
    }

    /// Cancela animação
    pub fn cancel(&mut self) {
        self.state = AnimationState::Idle;
    }

    /// Retorna estado atual
    pub fn state(&self) -> AnimationState {
        self.state
    }

    /// Retorna progresso (0.0 a 1.0)
    pub fn progress(&self) -> f32 {
        self.progress
    }
}

impl Default for ToggleAnimation {
    fn default() -> Self {
        Self::new(200)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggle_creation() {
        let toggle = SidebarToggle::new();
        assert!(toggle.show_button());
        assert_eq!(toggle.animation_duration_ms(), 200);
    }

    #[test]
    fn test_toggle_visibility() {
        let toggle = SidebarToggle::new();

        assert_eq!(
            toggle.toggle(SidebarVisibility::Visible),
            SidebarVisibility::Hidden
        );
        assert_eq!(
            toggle.toggle(SidebarVisibility::Hidden),
            SidebarVisibility::Visible
        );
        assert_eq!(
            toggle.toggle(SidebarVisibility::Compact),
            SidebarVisibility::Visible
        );
    }

    #[test]
    fn test_toggle_compact() {
        let toggle = SidebarToggle::new();

        assert_eq!(
            toggle.toggle_compact(SidebarVisibility::Visible),
            SidebarVisibility::Compact
        );
        assert_eq!(
            toggle.toggle_compact(SidebarVisibility::Hidden),
            SidebarVisibility::Compact
        );
        assert_eq!(
            toggle.toggle_compact(SidebarVisibility::Compact),
            SidebarVisibility::Visible
        );
    }

    #[test]
    fn test_direct_states() {
        let toggle = SidebarToggle::new();

        assert_eq!(toggle.show(), SidebarVisibility::Visible);
        assert_eq!(toggle.hide(), SidebarVisibility::Hidden);
        assert_eq!(toggle.compact(), SidebarVisibility::Compact);
    }

    #[test]
    fn test_cycle_states() {
        let toggle = SidebarToggle::new();

        let state1 = toggle.cycle_states(SidebarVisibility::Visible);
        assert_eq!(state1, SidebarVisibility::Compact);

        let state2 = toggle.cycle_states(state1);
        assert_eq!(state2, SidebarVisibility::Hidden);

        let state3 = toggle.cycle_states(state2);
        assert_eq!(state3, SidebarVisibility::Visible);
    }

    #[test]
    fn test_icons() {
        assert_eq!(
            SidebarToggle::icon_for_state(SidebarVisibility::Visible),
            "◀"
        );
        assert_eq!(
            SidebarToggle::icon_for_state(SidebarVisibility::Hidden),
            "▶"
        );
        assert_eq!(
            SidebarToggle::icon_for_state(SidebarVisibility::Compact),
            "◀"
        );
    }

    #[test]
    fn test_tooltips() {
        assert_eq!(
            SidebarToggle::tooltip_for_state(SidebarVisibility::Visible),
            "Hide Sidebar"
        );
        assert_eq!(
            SidebarToggle::tooltip_for_state(SidebarVisibility::Hidden),
            "Show Sidebar"
        );
        assert_eq!(
            SidebarToggle::tooltip_for_state(SidebarVisibility::Compact),
            "Expand Sidebar"
        );
    }

    #[test]
    fn test_keyboard_shortcut() {
        let mut toggle = SidebarToggle::new();

        assert_eq!(toggle.keyboard_shortcut(), Some("Ctrl+B"));

        toggle.set_keyboard_shortcut(Some("Cmd+Shift+S".to_string()));
        assert_eq!(toggle.keyboard_shortcut(), Some("Cmd+Shift+S"));

        toggle.set_keyboard_shortcut(None);
        assert_eq!(toggle.keyboard_shortcut(), None);
    }

    #[test]
    fn test_matches_shortcut() {
        let toggle = SidebarToggle::new();

        assert!(toggle.matches_shortcut("ctrl", "b"));
        assert!(toggle.matches_shortcut("CTRL", "B"));
        assert!(!toggle.matches_shortcut("cmd", "b"));
        assert!(!toggle.matches_shortcut("ctrl", "s"));
    }

    #[test]
    fn test_target_width() {
        let toggle = SidebarToggle::new();
        let config = SidebarConfig::default();

        assert_eq!(
            toggle.target_width(SidebarVisibility::Visible, &config),
            250
        );
        assert_eq!(toggle.target_width(SidebarVisibility::Hidden, &config), 0);
        assert_eq!(toggle.target_width(SidebarVisibility::Compact, &config), 50);
    }

    #[test]
    fn test_needs_animation() {
        let toggle = SidebarToggle::new();

        assert!(!toggle.needs_animation(SidebarVisibility::Visible, SidebarVisibility::Visible));
        assert!(toggle.needs_animation(SidebarVisibility::Visible, SidebarVisibility::Hidden));
    }

    #[test]
    fn test_animated_width() {
        let toggle = SidebarToggle::new();

        assert_eq!(toggle.animated_width(0, 100, 0.0), 0);
        assert_eq!(toggle.animated_width(0, 100, 0.5), 50);
        assert_eq!(toggle.animated_width(0, 100, 1.0), 100);
        assert_eq!(toggle.animated_width(100, 0, 0.5), 50);
    }

    #[test]
    fn test_auto_hide() {
        let mut toggle = SidebarToggle::new();

        assert!(!toggle.auto_hide_enabled());
        assert_eq!(toggle.auto_hide_delay_ms(), None);

        toggle.set_auto_hide(Some(3000));
        assert!(toggle.auto_hide_enabled());
        assert_eq!(toggle.auto_hide_delay_ms(), Some(3000));

        toggle.set_auto_hide(None);
        assert!(!toggle.auto_hide_enabled());
    }

    #[test]
    fn test_button_position() {
        let mut toggle = SidebarToggle::new();

        assert_eq!(toggle.button_position(), ToggleButtonPosition::InsideTop);

        toggle.set_button_position(ToggleButtonPosition::Floating);
        assert_eq!(toggle.button_position(), ToggleButtonPosition::Floating);
    }

    #[test]
    fn test_animation() {
        let mut anim = ToggleAnimation::new(100);

        assert!(!anim.is_animating());
        assert_eq!(anim.state(), AnimationState::Idle);

        anim.start(0, 100, AnimationState::Expanding);

        assert!(anim.is_animating());
        assert_eq!(anim.state(), AnimationState::Expanding);

        // Primeira atualização deve retornar valor
        let width = anim.update();
        assert!(width.is_some());

        // Cancela
        anim.cancel();
        assert!(!anim.is_animating());
    }

    #[test]
    fn test_animation_completion() {
        use std::thread;

        let mut anim = ToggleAnimation::new(50); // 50ms

        anim.start(0, 100, AnimationState::Expanding);

        // Espera animação completar
        thread::sleep(std::time::Duration::from_millis(60));

        let width = anim.update();
        assert_eq!(width, Some(100));
        assert!(!anim.is_animating());
    }

    #[test]
    fn test_config_with_options() {
        let mut toggle = SidebarToggle::new();

        toggle.set_animation_duration(300);
        assert_eq!(toggle.animation_duration_ms(), 300);

        toggle.set_show_button(false);
        assert!(!toggle.show_button());
    }
}
