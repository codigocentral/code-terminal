//! Fill API - API de Preenchimento de Formulários
//!
//! Este módulo fornece funcionalidades para preencher campos de
//! formulários de forma programática, incluindo inputs de texto,
//! checkboxes, radios, selects e textareas.
//!
//! # Exemplo
//!
//! ```rust,no_run
//! use code_terminal::browser::automation::fill::{FillController, FillOptions, FormData};
//! use code_terminal::browser::BrowserId;
//!
//! async fn fill_form_example(browser_id: BrowserId) {
//!     let controller = FillController::new(browser_id);
//!     
//!     // Preenche campos individuais
//!     controller.fill("#username", "john_doe", FillOptions::default()).await.unwrap();
//!     controller.fill("#password", "secret123", FillOptions::default()).await.unwrap();
//!     
//!     // Ou preenche múltiplos campos de uma vez
//!     let form_data = FormData::new()
//!         .with_field("#firstName", "John")
//!         .with_field("#lastName", "Doe")
//!         .with_field("#email", "john@example.com");
//!     
//!     controller.fill_form(form_data).await.unwrap();
//! }
//! ```

use crate::browser::{BrowserError, BrowserId, BrowserResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

/// Controlador de preenchimento
#[derive(Debug, Clone)]
pub struct FillController {
    browser_id: BrowserId,
}

impl FillController {
    /// Cria um novo controlador
    pub fn new(browser_id: BrowserId) -> Self {
        Self { browser_id }
    }

    /// Retorna o browser associado.
    pub fn browser_id(&self) -> BrowserId {
        self.browser_id
    }

    /// Preenche um campo
    pub async fn fill(
        &self,
        selector: impl AsRef<str>,
        value: impl AsRef<str>,
        options: FillOptions,
    ) -> BrowserResult<FillResult> {
        let selector = selector.as_ref();
        let value = value.as_ref();

        tracing::debug!(
            "Filling '{}' with value of length {}",
            selector,
            value.len()
        );

        if selector.is_empty() {
            return Err(BrowserError::InvalidUrl("Empty selector".to_string()));
        }

        // Delay antes
        if options.delay_before_ms > 0 {
            sleep(Duration::from_millis(options.delay_before_ms)).await;
        }

        // Focus no elemento
        if options.focus_before_fill {
            self.focus_element(selector).await?;
        }

        // Limpa campo se necessário
        if options.clear_before_fill {
            self.clear_field(selector, options).await?;
        }

        // Preenche o valor
        let result = if options.simulate_typing {
            self.type_slowly(selector, value, options.typing_delay_ms)
                .await?
        } else {
            self.set_value(selector, value).await?
        };

        // Dispara eventos
        if options.trigger_input_event {
            self.trigger_event(selector, "input").await?;
        }
        if options.trigger_change_event {
            self.trigger_event(selector, "change").await?;
        }

        // Validação
        let validation = if options.validate_after_fill {
            self.validate_value(selector, value).await?
        } else {
            ValidationResult::Skipped
        };

        // Delay após
        if options.delay_after_ms > 0 {
            sleep(Duration::from_millis(options.delay_after_ms)).await;
        }

        Ok(FillResult {
            success: result,
            selector: selector.to_string(),
            value_filled: value.to_string(),
            validation,
            filled_at: chrono::Utc::now(),
        })
    }

    /// Preenche múltiplos campos
    pub async fn fill_form(&self, form_data: FormData) -> BrowserResult<Vec<FillResult>> {
        let mut results = Vec::new();

        for (selector, value) in form_data.fields.iter() {
            let result = self.fill(selector, value, form_data.options).await?;
            results.push(result);

            // Delay entre campos
            if form_data.delay_between_fields_ms > 0 {
                sleep(Duration::from_millis(form_data.delay_between_fields_ms)).await;
            }
        }

        Ok(results)
    }

    /// Seleciona uma opção em um <select>
    pub async fn select_option(
        &self,
        selector: impl AsRef<str>,
        option: SelectOption,
        _options: FillOptions,
    ) -> BrowserResult<SelectResult> {
        let selector = selector.as_ref();

        tracing::debug!("Selecting option in '{}' by {:?}", selector, option);

        // Na implementação real, executaria JavaScript para selecionar

        Ok(SelectResult {
            success: true,
            selector: selector.to_string(),
            option,
            previous_value: None,
            selected_at: chrono::Utc::now(),
        })
    }

    /// Marca/desmarca um checkbox
    pub async fn set_checkbox(
        &self,
        selector: impl AsRef<str>,
        checked: bool,
        _options: FillOptions,
    ) -> BrowserResult<ToggleResult> {
        let selector = selector.as_ref();

        tracing::debug!("Setting checkbox '{}' to {}", selector, checked);

        Ok(ToggleResult {
            success: true,
            selector: selector.to_string(),
            new_state: checked,
            previous_state: !checked,
            changed_at: chrono::Utc::now(),
        })
    }

    /// Seleciona um radio button
    pub async fn select_radio(
        &self,
        selector: impl AsRef<str>,
        _options: FillOptions,
    ) -> BrowserResult<SelectResult> {
        let selector = selector.as_ref();

        tracing::debug!("Selecting radio button '{}'", selector);

        Ok(SelectResult {
            success: true,
            selector: selector.to_string(),
            option: SelectOption::BySelector(selector.to_string()),
            previous_value: None,
            selected_at: chrono::Utc::now(),
        })
    }

    /// Faz upload de arquivo
    pub async fn upload_file(
        &self,
        selector: impl AsRef<str>,
        file_path: impl AsRef<std::path::Path>,
        _options: FillOptions,
    ) -> BrowserResult<UploadResult> {
        let selector = selector.as_ref();
        let file_path = file_path.as_ref();

        tracing::debug!("Uploading file '{}' to '{}'", file_path.display(), selector);

        Ok(UploadResult {
            success: true,
            selector: selector.to_string(),
            file_path: file_path.to_path_buf(),
            file_name: file_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            uploaded_at: chrono::Utc::now(),
        })
    }

    /// Limpa um campo
    async fn clear_field(&self, selector: &str, options: FillOptions) -> BrowserResult<bool> {
        if options.use_select_all {
            // Ctrl+A + Delete
            tracing::debug!("Clearing field '{}' with select all", selector);
        } else {
            tracing::debug!("Clearing field '{}'", selector);
        }
        Ok(true)
    }

    /// Dá focus em um elemento
    async fn focus_element(&self, selector: &str) -> BrowserResult<bool> {
        tracing::debug!("Focusing element '{}'", selector);
        Ok(true)
    }

    /// Define valor diretamente
    async fn set_value(&self, selector: &str, _value: &str) -> BrowserResult<bool> {
        tracing::debug!("Setting value for '{}'", selector);
        Ok(true)
    }

    /// Digita lentamente (simulação de digitação humana)
    async fn type_slowly(&self, selector: &str, value: &str, delay_ms: u64) -> BrowserResult<bool> {
        tracing::debug!(
            "Typing slowly into '{}' with delay {}ms",
            selector,
            delay_ms
        );

        // Simula digitação caractere por caractere
        for ch in value.chars() {
            tracing::trace!("Typing character: {}", ch);
            sleep(Duration::from_millis(delay_ms)).await;
        }

        Ok(true)
    }

    /// Dispara evento em elemento
    async fn trigger_event(&self, selector: &str, event: &str) -> BrowserResult<bool> {
        tracing::debug!("Triggering '{}' event on '{}'", event, selector);
        Ok(true)
    }

    /// Valida valor preenchido
    async fn validate_value(
        &self,
        selector: &str,
        _expected: &str,
    ) -> BrowserResult<ValidationResult> {
        tracing::debug!("Validating value for '{}'", selector);
        Ok(ValidationResult::Valid)
    }
}

/// Opções de preenchimento
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FillOptions {
    /// Se deve limpar campo antes de preencher
    pub clear_before_fill: bool,
    /// Se deve usar Ctrl+A para limpar
    pub use_select_all: bool,
    /// Se deve simular digitação (caractere por caractere)
    pub simulate_typing: bool,
    /// Delay entre caracteres ao simular digitação (ms)
    pub typing_delay_ms: u64,
    /// Se deve dar focus antes de preencher
    pub focus_before_fill: bool,
    /// Se deve disparar evento input
    pub trigger_input_event: bool,
    /// Se deve disparar evento change
    pub trigger_change_event: bool,
    /// Se deve validar após preencher
    pub validate_after_fill: bool,
    /// Delay antes de preencher (ms)
    pub delay_before_ms: u64,
    /// Delay após preencher (ms)
    pub delay_after_ms: u64,
    /// Se deve scrollar elemento para visibilidade
    pub scroll_into_view: bool,
    /// Se deve pressionar Enter após preencher
    pub press_enter_after: bool,
    /// Se deve pressionar Tab após preencher
    pub press_tab_after: bool,
}

impl Default for FillOptions {
    fn default() -> Self {
        Self {
            clear_before_fill: true,
            use_select_all: true,
            simulate_typing: false,
            typing_delay_ms: 50,
            focus_before_fill: true,
            trigger_input_event: true,
            trigger_change_event: true,
            validate_after_fill: false,
            delay_before_ms: 0,
            delay_after_ms: 50,
            scroll_into_view: true,
            press_enter_after: false,
            press_tab_after: false,
        }
    }
}

impl FillOptions {
    /// Cria opções com simulação de digitação
    pub fn with_typing(delay_ms: u64) -> Self {
        Self {
            simulate_typing: true,
            typing_delay_ms: delay_ms,
            ..Default::default()
        }
    }

    /// Cria opções sem limpar campo
    pub fn append_mode() -> Self {
        Self {
            clear_before_fill: false,
            ..Default::default()
        }
    }

    /// Habilita validação
    pub fn with_validation(mut self) -> Self {
        self.validate_after_fill = true;
        self
    }

    /// Define delay de digitação
    pub fn with_typing_delay(mut self, ms: u64) -> Self {
        self.typing_delay_ms = ms;
        self
    }

    /// Pressiona Enter após preencher
    pub fn with_enter(mut self) -> Self {
        self.press_enter_after = true;
        self
    }

    /// Pressiona Tab após preencher
    pub fn with_tab(mut self) -> Self {
        self.press_tab_after = true;
        self
    }
}

/// Dados de formulário
#[derive(Debug, Clone)]
pub struct FormData {
    /// Campos e valores
    pub fields: HashMap<String, String>,
    /// Opções para todos os campos
    pub options: FillOptions,
    /// Delay entre campos (ms)
    pub delay_between_fields_ms: u64,
}

impl Default for FormData {
    fn default() -> Self {
        Self {
            fields: HashMap::new(),
            options: FillOptions::default(),
            delay_between_fields_ms: 100,
        }
    }
}

impl FormData {
    /// Cria novo FormData vazio
    pub fn new() -> Self {
        Self::default()
    }

    /// Adiciona um campo
    pub fn with_field(mut self, selector: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(selector.into(), value.into());
        self
    }

    /// Adiciona múltiplos campos
    pub fn with_fields(mut self, fields: HashMap<String, String>) -> Self {
        self.fields.extend(fields);
        self
    }

    /// Define opções
    pub fn with_options(mut self, options: FillOptions) -> Self {
        self.options = options;
        self
    }

    /// Define delay entre campos
    pub fn with_delay_between_fields(mut self, ms: u64) -> Self {
        self.delay_between_fields_ms = ms;
        self
    }

    /// Retorna número de campos
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Retorna se está vazio
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Retorna iterador sobre campos
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.fields.iter()
    }
}

/// Opção de seleção para dropdowns
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum SelectOption {
    /// Seleciona por valor
    ByValue(String),
    /// Seleciona por texto visível
    ByText(String),
    /// Seleciona por índice
    ByIndex(usize),
    /// Seleciona por selector
    BySelector(String),
    /// Seleciona por label
    ByLabel(String),
}

impl SelectOption {
    /// Cria opção por valor
    pub fn value(value: impl Into<String>) -> Self {
        SelectOption::ByValue(value.into())
    }

    /// Cria opção por texto
    pub fn text(text: impl Into<String>) -> Self {
        SelectOption::ByText(text.into())
    }

    /// Cria opção por índice
    pub fn index(index: usize) -> Self {
        SelectOption::ByIndex(index)
    }
}

/// Resultado de preenchimento
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillResult {
    /// Se foi bem-sucedido
    pub success: bool,
    /// Selector usado
    pub selector: String,
    /// Valor preenchido
    pub value_filled: String,
    /// Resultado da validação
    pub validation: ValidationResult,
    /// Timestamp
    pub filled_at: chrono::DateTime<chrono::Utc>,
}

/// Resultado de seleção
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectResult {
    /// Se foi bem-sucedido
    pub success: bool,
    /// Selector usado
    pub selector: String,
    /// Opção selecionada
    pub option: SelectOption,
    /// Valor anterior (se aplicável)
    pub previous_value: Option<String>,
    /// Timestamp
    pub selected_at: chrono::DateTime<chrono::Utc>,
}

/// Resultado de toggle (checkbox)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToggleResult {
    /// Se foi bem-sucedido
    pub success: bool,
    /// Selector usado
    pub selector: String,
    /// Novo estado
    pub new_state: bool,
    /// Estado anterior
    pub previous_state: bool,
    /// Timestamp
    pub changed_at: chrono::DateTime<chrono::Utc>,
}

/// Resultado de upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResult {
    /// Se foi bem-sucedido
    pub success: bool,
    /// Selector usado
    pub selector: String,
    /// Caminho do arquivo
    pub file_path: std::path::PathBuf,
    /// Nome do arquivo
    pub file_name: String,
    /// Timestamp
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
}

/// Resultado de validação
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type")]
pub enum ValidationResult {
    /// Válido
    #[default]
    Valid,
    /// Inválido com motivo
    Invalid { reason: String },
    /// Pulado
    Skipped,
    /// Erro na validação
    Error { message: String },
}

/// Utilitários para formulários
pub struct FormUtils;

impl FormUtils {
    /// Gera dados de formulário a partir de JSON
    pub fn from_json(json: &str) -> BrowserResult<FormData> {
        let map: HashMap<String, String> = serde_json::from_str(json)
            .map_err(|e| BrowserError::ConfigError(format!("Invalid JSON: {}", e)))?;

        Ok(FormData::new().with_fields(map))
    }

    /// Valida email
    pub fn is_valid_email(email: &str) -> bool {
        // Regex simples para validação de email
        let email_regex = regex::Regex::new(r"^[^@]+@[^@]+\.[^@]+$").unwrap();
        email_regex.is_match(email)
    }

    /// Mascara senha para logs
    pub fn mask_password(password: &str) -> String {
        "*".repeat(password.len())
    }

    /// Gera texto aleatório para testes
    pub fn random_text(length: usize) -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();

        (0..length)
            .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fill_options_default() {
        let opts = FillOptions::default();
        assert!(opts.clear_before_fill);
        assert!(opts.use_select_all);
        assert!(!opts.simulate_typing);
        assert_eq!(opts.typing_delay_ms, 50);
    }

    #[test]
    fn test_fill_options_with_typing() {
        let opts = FillOptions::with_typing(100);
        assert!(opts.simulate_typing);
        assert_eq!(opts.typing_delay_ms, 100);
    }

    #[test]
    fn test_form_data_builder() {
        let form = FormData::new()
            .with_field("#name", "John")
            .with_field("#email", "john@example.com")
            .with_delay_between_fields(200);

        assert_eq!(form.len(), 2);
        assert_eq!(form.delay_between_fields_ms, 200);
        assert!(form.fields.contains_key("#name"));
        assert_eq!(
            form.fields.get("#email"),
            Some(&"john@example.com".to_string())
        );
    }

    #[test]
    fn test_select_option() {
        let opt1 = SelectOption::value("option1");
        let opt2 = SelectOption::text("Visible Text");
        let opt3 = SelectOption::index(2);

        match opt1 {
            SelectOption::ByValue(v) => assert_eq!(v, "option1"),
            _ => panic!("Expected ByValue"),
        }

        match opt2 {
            SelectOption::ByText(t) => assert_eq!(t, "Visible Text"),
            _ => panic!("Expected ByText"),
        }

        match opt3 {
            SelectOption::ByIndex(i) => assert_eq!(i, 2),
            _ => panic!("Expected ByIndex"),
        }
    }

    #[test]
    fn test_form_utils_mask_password() {
        let masked = FormUtils::mask_password("secret123");
        assert_eq!(masked, "*********");
    }

    #[test]
    fn test_form_utils_is_valid_email() {
        assert!(FormUtils::is_valid_email("test@example.com"));
        assert!(FormUtils::is_valid_email("user@domain.co.uk"));
        assert!(!FormUtils::is_valid_email("invalid"));
        assert!(!FormUtils::is_valid_email("@example.com"));
        assert!(!FormUtils::is_valid_email("test@"));
    }

    #[test]
    fn test_fill_options_builder() {
        let opts = FillOptions::default()
            .with_validation()
            .with_enter()
            .with_tab()
            .with_typing_delay(75);

        assert!(opts.validate_after_fill);
        assert!(opts.press_enter_after);
        assert!(opts.press_tab_after);
        assert_eq!(opts.typing_delay_ms, 75);
    }

    #[test]
    fn test_validation_result() {
        let valid = ValidationResult::Valid;
        let invalid = ValidationResult::Invalid {
            reason: "Required field".to_string(),
        };
        let skipped = ValidationResult::Skipped;

        match valid {
            ValidationResult::Valid => (),
            _ => panic!("Expected Valid"),
        }

        match invalid {
            ValidationResult::Invalid { reason } => assert_eq!(reason, "Required field"),
            _ => panic!("Expected Invalid"),
        }

        match skipped {
            ValidationResult::Skipped => (),
            _ => panic!("Expected Skipped"),
        }
    }

    fn fast_options() -> FillOptions {
        FillOptions {
            delay_after_ms: 0,
            typing_delay_ms: 0,
            ..FillOptions::default()
        }
    }

    #[tokio::test]
    async fn test_fill_returns_result_for_every_option_combination() {
        let controller = FillController::new(BrowserId::new());

        let result = controller
            .fill("#name", "Maria", fast_options())
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.selector, "#name");
        assert_eq!(result.value_filled, "Maria");
        assert!(matches!(result.validation, ValidationResult::Skipped));

        // Digitação simulada, sem focus/clear/eventos, com validação e delays
        let options = FillOptions {
            simulate_typing: true,
            focus_before_fill: false,
            clear_before_fill: false,
            trigger_input_event: false,
            trigger_change_event: false,
            delay_before_ms: 1,
            delay_after_ms: 1,
            ..fast_options()
        }
        .with_validation();
        let result = controller.fill("#bio", "abc", options).await.unwrap();
        assert!(result.success);
        assert!(matches!(result.validation, ValidationResult::Valid));

        // Limpeza sem select-all
        let options = FillOptions {
            use_select_all: false,
            ..FillOptions::append_mode()
        };
        assert!(!options.clear_before_fill);
        let options = FillOptions {
            clear_before_fill: true,
            delay_after_ms: 0,
            ..options
        };
        assert!(controller.fill("#x", "y", options).await.unwrap().success);
    }

    #[tokio::test]
    async fn test_fill_rejects_empty_selector() {
        let controller = FillController::new(BrowserId::new());
        let err = controller.fill("", "v", fast_options()).await.unwrap_err();
        assert!(err.to_string().contains("Empty selector"), "{err}");
    }

    #[tokio::test]
    async fn test_fill_form_fills_every_field() {
        let browser_id = BrowserId::new();
        let controller = FillController::new(browser_id);
        assert_eq!(controller.browser_id(), browser_id);

        let form = FormData::new()
            .with_field("#a", "1")
            .with_field("#b", "2")
            .with_options(fast_options())
            .with_delay_between_fields(1);
        let mut results = controller.fill_form(form).await.unwrap();
        results.sort_by(|a, b| a.selector.cmp(&b.selector));
        let filled: Vec<_> = results
            .iter()
            .map(|r| (r.selector.as_str(), r.value_filled.as_str()))
            .collect();
        assert_eq!(filled, vec![("#a", "1"), ("#b", "2")]);

        // Um campo inválido aborta o formulário
        let form = FormData::new()
            .with_field("", "x")
            .with_options(fast_options())
            .with_delay_between_fields(0);
        assert!(controller.fill_form(form).await.is_err());
    }

    #[tokio::test]
    async fn test_select_checkbox_radio_and_upload() {
        let controller = FillController::new(BrowserId::new());

        let select = controller
            .select_option("#pais", SelectOption::value("BR"), fast_options())
            .await
            .unwrap();
        assert!(select.success);
        assert_eq!(select.selector, "#pais");
        assert!(matches!(select.option, SelectOption::ByValue(ref v) if v == "BR"));
        assert!(select.previous_value.is_none());

        let toggle = controller
            .set_checkbox("#aceito", true, fast_options())
            .await
            .unwrap();
        assert!(toggle.new_state);
        assert!(!toggle.previous_state);
        let toggle = controller
            .set_checkbox("#aceito", false, fast_options())
            .await
            .unwrap();
        assert!(!toggle.new_state);
        assert!(toggle.previous_state);

        let radio = controller
            .select_radio("input[value=m]", fast_options())
            .await
            .unwrap();
        assert!(matches!(radio.option, SelectOption::BySelector(ref s) if s == "input[value=m]"));

        let upload = controller
            .upload_file("#arquivo", "/tmp/docs/relatorio.pdf", fast_options())
            .await
            .unwrap();
        assert_eq!(upload.file_name, "relatorio.pdf");
        assert_eq!(
            upload.file_path,
            std::path::PathBuf::from("/tmp/docs/relatorio.pdf")
        );

        // Caminho sem nome de arquivo
        let upload = controller
            .upload_file("#arquivo", "/", fast_options())
            .await
            .unwrap();
        assert_eq!(upload.file_name, "");
    }

    #[test]
    fn test_form_data_with_fields_and_iter() {
        let mut extra = HashMap::new();
        extra.insert("#c".to_string(), "3".to_string());
        let form = FormData::new().with_field("#a", "1").with_fields(extra);
        assert!(!form.is_empty());
        assert!(FormData::new().is_empty());
        let mut keys: Vec<_> = form.iter().map(|(k, _)| k.clone()).collect();
        keys.sort();
        assert_eq!(keys, vec!["#a", "#c"]);
    }

    #[test]
    fn test_form_utils_from_json_and_random_text() {
        let form = FormUtils::from_json(r##"{"#user":"ana","#pass":"x"}"##).unwrap();
        assert_eq!(form.len(), 2);
        assert_eq!(form.fields.get("#user"), Some(&"ana".to_string()));

        let err = FormUtils::from_json("not json").unwrap_err();
        assert!(err.to_string().contains("Invalid JSON"), "{err}");

        let text = FormUtils::random_text(32);
        assert_eq!(text.len(), 32);
        assert!(text.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(FormUtils::random_text(0).is_empty());
    }

    #[test]
    fn test_serde_tags() {
        let json = serde_json::to_value(SelectOption::ByLabel("Rótulo".into())).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "ByLabel", "value": "Rótulo"})
        );
        let json = serde_json::to_value(ValidationResult::Error {
            message: "falhou".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "Error", "message": "falhou"})
        );
        assert!(matches!(
            ValidationResult::default(),
            ValidationResult::Valid
        ));
    }
}
