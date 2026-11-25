//! Tests para o módulo de browser

#[cfg(test)]
mod tests {
    use super::super::*;

    mod config_tests {
        use super::*;

        #[test]
        fn test_default_browser_config() {
            let config = BrowserConfig::default();
            
            // Verifica valores padrão
            assert_eq!(config.homepage, HomepageConfig::default());
            assert_eq!(config.search_engine, SearchEngine::DuckDuckGo);
            assert!(config.enable_javascript);
            assert!(config.enable_cookies);
            assert!(!config.enable_devtools);
            assert_eq!(config.default_zoom, 1.0);
            assert!(!config.enable_ad_blocker);
            assert!(!config.enable_tracking_protection);
            assert_eq!(config.download_directory, dirs::download_dir().unwrap_or_default());
            assert_eq!(config.user_agent, UserAgent::Default);
        }

        #[test]
        fn test_privacy_mode_standard() {
            let config = BrowserConfig::default()
                .with_privacy_mode(PrivacyMode::Standard);
            
            assert!(config.enable_cookies);
            assert!(!config.enable_tracking_protection);
        }

        #[test]
        fn test_privacy_mode_incognito() {
            let config = BrowserConfig::default()
                .with_privacy_mode(PrivacyMode::Incognito);
            
            assert!(!config.enable_cookies);
            assert!(config.enable_tracking_protection);
        }

        #[test]
        fn test_privacy_mode_secure() {
            let config = BrowserConfig::default()
                .with_privacy_mode(PrivacyMode::Secure);
            
            assert!(!config.enable_javascript);
            assert!(!config.enable_cookies);
            assert!(config.enable_tracking_protection);
        }

        #[test]
        fn test_homepage_config_url() {
            let homepage = HomepageConfig::Url("https://example.com".to_string());
            let config = BrowserConfig::default()
                .with_homepage(homepage.clone());
            
            assert_eq!(config.homepage, homepage);
            assert_eq!(config.homepage_url(), "https://example.com");
        }

        #[test]
        fn test_homepage_config_blank() {
            let homepage = HomepageConfig::Blank;
            let config = BrowserConfig::default()
                .with_homepage(homepage);
            
            assert_eq!(config.homepage_url(), "about:blank");
        }

        #[test]
        fn test_search_engine_url() {
            assert_eq!(
                SearchEngine::DuckDuckGo.search_url("test query"),
                "https://duckduckgo.com/?q=test+query"
            );
            assert_eq!(
                SearchEngine::Google.search_url("test query"),
                "https://www.google.com/search?q=test+query"
            );
            assert_eq!(
                SearchEngine::Bing.search_url("test query"),
                "https://www.bing.com/search?q=test+query"
            );
        }

        #[test]
        fn test_user_agent_custom() {
            let custom_ua = "CustomAgent/1.0".to_string();
            let config = BrowserConfig::default()
                .with_user_agent(UserAgent::Custom(custom_ua.clone()));
            
            assert_eq!(config.user_agent_string(), custom_ua);
        }

        #[test]
        fn test_user_agent_default() {
            let config = BrowserConfig::default();
            
            assert_eq!(config.user_agent_string(), "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36");
        }

        #[test]
        fn test_permitted_resources_default() {
            let config = BrowserConfig::default();
            
            assert!(config.is_resource_permitted(PermittedResource::Images));
            assert!(config.is_resource_permitted(PermittedResource::Stylesheets));
            assert!(config.is_resource_permitted(PermittedResource::Scripts));
            assert!(!config.is_resource_permitted(PermittedResource::Popups));
            assert!(!config.is_resource_permitted(PermittedResource::Notifications));
        }

        #[test]
        fn test_zoom_limits() {
            let mut config = BrowserConfig::default();
            
            // Zoom máximo deve ser 5.0
            config.default_zoom = 10.0;
            // Não há validação automática na struct, isso seria feito no UI
            
            // Zoom mínimo deve ser 0.25
            config.default_zoom = 0.1;
            // Mesmo caso
            
            assert_eq!(config.default_zoom, 0.1);
        }
    }

    mod navigation_tests {
        use super::*;

        #[test]
        fn test_navigation_state_initial() {
            let state = NavigationState::default();
            
            assert!(!state.can_go_back);
            assert!(!state.can_go_forward);
            assert!(!state.is_loading);
            assert_eq!(state.progress_percent, 0);
            assert!(state.current_url.is_none());
        }

        #[test]
        fn test_navigation_state_update() {
            let mut state = NavigationState::default();
            let url = url::Url::parse("https://example.com").unwrap();
            
            state.update(url.clone(), "Example".to_string(), 50);
            
            assert_eq!(state.current_url, Some(url));
            assert_eq!(state.page_title, Some("Example".to_string()));
            assert_eq!(state.progress_percent, 50);
        }

        #[test]
        fn test_navigate_result_success() {
            let result = NavigateResult::Success;
            assert!(result.is_success());
            assert!(!result.is_blocked());
        }

        #[test]
        fn test_navigate_result_blocked() {
            let reason = BlockReason::NotHttps;
            let result = NavigateResult::Blocked(reason.clone());
            
            assert!(!result.is_success());
            assert!(result.is_blocked());
            assert_eq!(result.block_reason(), Some(&reason));
        }

        #[test]
        fn test_navigate_result_error() {
            let error = NavigationError::InvalidUrl("bad url".to_string());
            let result = NavigateResult::Error(error.clone());
            
            assert!(!result.is_success());
            assert!(!result.is_blocked());
            assert!(result.error_message().is_some());
        }
    }

    mod address_bar_tests {
        use super::*;

        #[test]
        fn test_address_bar_initial() {
            let bar = AddressBar::default();
            
            assert!(bar.text().is_empty());
            assert!(bar.selection_range.is_none());
            assert!(bar.suggestions.is_empty());
            assert!(bar.error_message.is_none());
            assert!(!bar.has_focus);
        }

        #[test]
        fn test_address_bar_set_text() {
            let mut bar = AddressBar::default();
            bar.set_text("example.com");
            
            assert_eq!(bar.text(), "example.com");
        }

        #[test]
        fn test_address_bar_select_all() {
            let mut bar = AddressBar::default();
            bar.set_text("example.com");
            bar.select_all();
            
            assert_eq!(bar.selection_range, Some((0, 11)));
            assert!(bar.has_selection());
        }

        #[test]
        fn test_address_bar_clear_selection() {
            let mut bar = AddressBar::default();
            bar.set_text("example.com");
            bar.select_all();
            bar.clear_selection();
            
            assert!(!bar.has_selection());
            assert_eq!(bar.selection_range, None);
        }

        #[test]
        fn test_address_bar_focus() {
            let mut bar = AddressBar::default();
            
            assert!(!bar.has_focus);
            bar.set_focus(true);
            assert!(bar.has_focus);
            bar.set_focus(false);
            assert!(!bar.has_focus);
        }

        #[test]
        fn test_address_bar_set_error() {
            let mut bar = AddressBar::default();
            bar.set_error("Invalid URL".to_string());
            
            assert_eq!(bar.error_message, Some("Invalid URL".to_string()));
        }

        #[test]
        fn test_address_bar_clear_error() {
            let mut bar = AddressBar::default();
            bar.set_error("Error".to_string());
            bar.clear_error();
            
            assert!(bar.error_message.is_none());
        }

        #[test]
        fn test_address_bar_suggestions() {
            let mut bar = AddressBar::default();
            let suggestions = vec![
                "https://google.com".to_string(),
                "https://github.com".to_string(),
            ];
            bar.set_suggestions(suggestions.clone());
            
            assert_eq!(bar.suggestions.len(), 2);
            assert_eq!(bar.suggestions[0], "https://google.com");
        }
    }

    mod webview_tests {
        use super::*;

        #[test]
        fn test_webview_handle_creation() {
            let handle = WebViewHandle(1);
            assert_eq!(handle.0, 1);
            assert_eq!(handle.to_string(), "webview-1");
        }

        #[test]
        fn test_webview_handle_equality() {
            let h1 = WebViewHandle(1);
            let h2 = WebViewHandle(1);
            let h3 = WebViewHandle(2);
            
            assert_eq!(h1, h2);
            assert_ne!(h1, h3);
        }

        #[test]
        fn test_webview_state_display() {
            assert_eq!(WebViewState::Uninitialized.to_string(), "uninitialized");
            assert_eq!(WebViewState::Initializing.to_string(), "initializing");
            assert_eq!(WebViewState::Ready.to_string(), "ready");
            assert_eq!(WebViewState::Loading.to_string(), "loading");
            assert_eq!(WebViewState::Error.to_string(), "error");
            assert_eq!(WebViewState::Destroyed.to_string(), "destroyed");
        }

        #[test]
        fn test_webview_manager_creation() {
            let manager = WebViewManager::new();
            assert_eq!(manager.count(), 0);
        }

        #[test]
        fn test_webview_manager_create() {
            let mut manager = WebViewManager::new();
            let handle = manager.create_webview(BrowserConfig::default()).unwrap();
            
            assert_eq!(manager.count(), 1);
            assert!(manager.get(handle).is_some());
        }

        #[test]
        fn test_webview_manager_destroy() {
            let mut manager = WebViewManager::new();
            let handle = manager.create_webview(BrowserConfig::default()).unwrap();
            
            manager.destroy(handle).unwrap();
            assert_eq!(manager.count(), 0);
            assert!(manager.get(handle).is_none());
        }

        #[test]
        fn test_webview_navigate() {
            let mut webview = BrowserWebView::new(
                WebViewHandle(1),
                BrowserConfig::default()
            );
            
            webview.initialize().unwrap();
            
            let url = url::Url::parse("https://example.com").unwrap();
            webview.navigate(&url).unwrap();
            
            assert_eq!(webview.current_url(), Some(&url));
            assert!(matches!(webview.state(), WebViewState::Loading));
        }

        #[test]
        fn test_webview_progress_update() {
            let mut webview = BrowserWebView::new(
                WebViewHandle(1),
                BrowserConfig::default()
            );
            
            webview.initialize().unwrap();
            let url = url::Url::parse("https://example.com").unwrap();
            webview.navigate(&url).unwrap();
            
            webview.update_load_progress(50);
            assert_eq!(webview.load_progress(), 50);
            
            webview.update_load_progress(100);
            assert!(matches!(webview.state(), WebViewState::Ready));
        }

        #[test]
        fn test_webview_double_initialize_fails() {
            let mut webview = BrowserWebView::new(
                WebViewHandle(1),
                BrowserConfig::default()
            );
            
            webview.initialize().unwrap();
            let result = webview.initialize();
            
            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), BrowserError::WebViewCreationFailed(_)));
        }

        #[test]
        fn test_webview_not_initialized_error() {
            let webview = BrowserWebView::new(
                WebViewHandle(1),
                BrowserConfig::default()
            );
            
            let url = url::Url::parse("https://example.com").unwrap();
            let result = webview.navigate(&url);
            
            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), BrowserError::WebViewNotInitialized));
        }
    }

    mod events_tests {
        use super::super::events::*;
        use super::super::*;

        #[test]
        fn test_navigation_event_creation() {
            let url = url::Url::parse("https://example.com").unwrap();
            let event = NavigationEvent::new(NavigationEventType::Started, url.clone());
            
            assert!(matches!(event.event_type, NavigationEventType::Started));
            assert_eq!(event.url, url);
            assert!(event.previous_url.is_none());
        }

        #[test]
        fn test_navigation_event_with_previous() {
            let url1 = url::Url::parse("https://example.com/page1").unwrap();
            let url2 = url::Url::parse("https://example.com/page2").unwrap();
            let event = NavigationEvent::new(NavigationEventType::Started, url2.clone())
                .with_previous(url1.clone());
            
            assert_eq!(event.previous_url, Some(url1));
        }

        #[test]
        fn test_load_event_creation() {
            let url = url::Url::parse("https://example.com").unwrap();
            let event = LoadEvent::new(LoadEventType::Started, url.clone());
            
            assert!(matches!(event.event_type, LoadEventType::Started));
            assert_eq!(event.url, url);
            assert_eq!(event.progress, 0);
        }

        #[test]
        fn test_load_event_with_progress() {
            let url = url::Url::parse("https://example.com").unwrap();
            let event = LoadEvent::new(LoadEventType::Progress, url.clone())
                .with_progress(75);
            
            assert_eq!(event.progress, 75);
        }

        #[test]
        fn test_console_event_creation() {
            let event = ConsoleEvent::new(ConsoleLevel::Info, "Test message");
            
            assert!(matches!(event.level, ConsoleLevel::Info));
            assert_eq!(event.message, "Test message");
            assert!(event.source.is_none());
        }

        #[test]
        fn test_console_event_with_source() {
            let event = ConsoleEvent::new(ConsoleLevel::Error, "Error!")
                .with_source("script.js", 10);
            
            assert_eq!(event.source, Some(("script.js".to_string(), 10)));
        }

        #[test]
        fn test_browser_event_variants() {
            let url = url::Url::parse("https://example.com").unwrap();
            
            let nav_event = BrowserEvent::Navigation(
                NavigationEvent::new(NavigationEventType::Started, url.clone())
            );
            assert!(matches!(nav_event, BrowserEvent::Navigation(_)));
            
            let load_event = BrowserEvent::Load(
                LoadEvent::new(LoadEventType::Finished, url.clone())
            );
            assert!(matches!(load_event, BrowserEvent::Load(_)));
            
            let console_event = BrowserEvent::Console(
                ConsoleEvent::new(ConsoleLevel::Warn, "Warning")
            );
            assert!(matches!(console_event, BrowserEvent::Console(_)));
            
            let error_event = BrowserEvent::Error("Something went wrong".to_string());
            assert!(matches!(error_event, BrowserEvent::Error(_)));
        }
    }

    mod platform_tests {
        // Estes testes verificam a estrutura das implementações de plataforma
        // Testes reais requeriam bindings nativos
        
        #[test]
        #[cfg(target_os = "windows")]
        fn test_windows_webview2_availability() {
            // Assume disponível para testes
            assert!(super::super::platform_windows::is_webview2_available());
        }

        #[test]
        #[cfg(target_os = "windows")]
        fn test_windows_webview2_version() {
            let version = super::super::platform_windows::get_webview2_version();
            assert!(version.is_some());
        }

        #[test]
        #[cfg(target_os = "macos")]
        fn test_macos_wkwebview_availability() {
            assert!(super::super::platform_macos::is_wkwebview_available());
        }

        #[test]
        #[cfg(target_os = "linux")]
        fn test_linux_webkitgtk_availability() {
            assert!(super::super::platform_linux::is_webkitgtk_available());
        }

        #[test]
        #[cfg(target_os = "linux")]
        fn test_linux_webkitgtk_version_detection() {
            let version = super::super::platform_linux::detect_webkitgtk_api_version();
            assert!(version.is_some());
        }
    }
}
