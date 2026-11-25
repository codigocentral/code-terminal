pub mod ai_suggestions;
pub mod command_palette;
pub mod glow;
pub mod hover;
pub mod i18n;
pub mod key_encoding;
pub mod layout;
pub mod line_edit;
pub mod manager;
pub mod mouse_report;
pub mod mouse_resize;
pub mod mouse_selection;
pub mod onboarding;
pub mod overlay_hits;
pub mod pane_layout;
pub mod quick_connect;
pub mod search_bar;
pub mod settings;
pub mod settings_ssh;
pub mod sidebar;
pub mod sidebar_info;
pub mod sidebar_rows;
pub mod splits;

// Re-exports do módulo splits
pub use splits::{
    action_to_split_op, generate_split_render_info, NavigationDirection, SplitManager,
    SplitOperation, SplitPane, SplitRenderInfo, SplitTree,
};
pub mod tab_animation;
pub mod tab_colors;
pub mod tab_title;
pub mod tabs;
pub mod toast;
pub mod url_detection;
pub mod window;

// Re-exports do módulo manager
pub use manager::UIManager;

// Re-exports principais do módulo window
pub use window::{Window, WindowCreateInfo, WindowEventMsg, WindowId, WindowManager, WindowState};
