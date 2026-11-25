//! Handler de mensagens JSON-RPC
//!
//! Processa requisições e direciona para os handlers apropriados.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::RwLock;

use crate::remote::auth::{AuthConfig, AuthContext, AuthError, AuthVerifier, Credentials};
use crate::remote::protocol::{Id, Message, Request, Response, RpcError};

/// Tipo de função handler (clonável via Arc)
pub type HandlerFn = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send>> + Send + Sync,
>;

/// Handler de métodos JSON-RPC
pub struct RpcHandler {
    methods: Arc<RwLock<HashMap<String, HandlerFn>>>,
    auth_verifier: Option<AuthVerifier>,
}

impl Clone for RpcHandler {
    fn clone(&self) -> Self {
        Self {
            methods: Arc::clone(&self.methods),
            auth_verifier: None, // AuthVerifier não é clonável por design
        }
    }
}

impl std::fmt::Debug for RpcHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcHandler")
            .field("methods", &"<HandlerFn HashMap>")
            .field("auth_enabled", &self.auth_verifier.is_some())
            .finish()
    }
}

impl Default for RpcHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RpcHandler {
    /// Criar novo handler vazio
    pub fn new() -> Self {
        Self {
            methods: Arc::new(RwLock::new(HashMap::new())),
            auth_verifier: None,
        }
    }

    /// Criar handler com autenticação
    pub fn with_auth(auth_config: AuthConfig) -> Self {
        let auth_enabled = auth_config.enabled;
        Self {
            methods: Arc::new(RwLock::new(HashMap::new())),
            auth_verifier: if auth_enabled {
                Some(AuthVerifier::new(auth_config))
            } else {
                None
            },
        }
    }

    /// Verificar autenticação
    pub async fn authenticate(
        &self,
        credentials: &Credentials,
        client_ip: Option<std::net::IpAddr>,
    ) -> Result<AuthContext, AuthError> {
        match &self.auth_verifier {
            Some(verifier) => verifier.verify(credentials, client_ip).await,
            None => Ok(AuthContext::unauthenticated()),
        }
    }

    /// Verificar se autenticação está habilitada
    pub fn is_auth_enabled(&self) -> bool {
        self.auth_verifier.is_some()
    }

    /// Registrar um método
    pub async fn register<F, Fut>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, RpcError>> + Send + 'static,
    {
        let arc: HandlerFn = Arc::new(move |params| Box::pin(handler(params)));
        let name: String = name.into();
        let mut methods: tokio::sync::RwLockWriteGuard<'_, HashMap<String, HandlerFn>> =
            self.methods.write().await;
        methods.insert(name, arc);
    }

    /// Verificar se método existe
    pub async fn has_method(&self, name: &str) -> bool {
        let methods: tokio::sync::RwLockReadGuard<'_, HashMap<String, HandlerFn>> =
            self.methods.read().await;
        methods.contains_key(name)
    }

    /// Listar métodos registrados
    pub async fn list_methods(&self) -> Vec<String> {
        let methods: tokio::sync::RwLockReadGuard<'_, HashMap<String, HandlerFn>> =
            self.methods.read().await;
        methods.keys().cloned().collect()
    }

    /// Processar uma mensagem
    pub async fn handle_message(&self, message: Message) -> Option<Message> {
        match message {
            Message::Request(req) => self.handle_request(req).await.map(Message::Response),
            Message::BatchRequest(batch) => self.handle_batch(batch).await,
            // Respostas não devem ser recebidas pelo servidor
            Message::Response(_) => None,
            Message::BatchResponse(_) => None,
        }
    }

    /// Processar requisição única
    pub async fn handle_request(&self, request: Request) -> Option<Response> {
        // Clonar ID para uso posterior
        let id = request.id.clone();

        // Validar requisição
        if let Err(e) = request.validate() {
            return Some(Response::from_error(id, e));
        }

        // Verificar se método existe
        if !self.has_method(&request.method).await {
            return Some(Response::from_error(
                id,
                RpcError::method_not_found(&request.method),
            ));
        }

        // Executar método
        let result = self
            .execute_method(&request.method, request.params.clone())
            .await;

        // Notificações não retornam resposta
        if request.is_notification() {
            return None;
        }

        match result {
            Ok(value) => {
                let id_val = id.clone().unwrap_or(Id::Null);
                let resp = Response::success(id_val, value);
                Some(resp.unwrap_or_else(|e| {
                    Response::from_error(id.clone(), RpcError::internal_error(e.to_string()))
                }))
            }
            Err(e) => Some(Response::from_error(id, e)),
        }
    }

    /// Executar método (público para testes)
    pub async fn execute_method(
        &self,
        name: &str,
        params: Option<Value>,
    ) -> Result<Value, RpcError> {
        let handler: HandlerFn = {
            let methods: tokio::sync::RwLockReadGuard<'_, HashMap<String, HandlerFn>> =
                self.methods.read().await;
            methods
                .get(name)
                .ok_or_else(|| RpcError::method_not_found(name))?
                .clone()
        };

        handler(params.unwrap_or(Value::Null)).await
    }

    /// Processar batch de requisições
    async fn handle_batch(&self, batch: Vec<Request>) -> Option<Message> {
        let mut responses = Vec::new();

        for request in batch {
            if let Some(response) = self.handle_request(request).await {
                responses.push(response);
            }
        }

        if responses.is_empty() {
            None
        } else {
            Some(Message::BatchResponse(responses))
        }
    }

    /// Parse e processar mensagem JSON
    pub async fn process_json(&self, json: &str) -> Option<String> {
        match Message::from_str(json) {
            Ok(message) => self
                .handle_message(message)
                .await
                .and_then(|m| m.to_json_string().ok()),
            Err(e) => {
                // Erro de parse - retornar resposta de erro
                Some(serde_json::to_string(&Response::from_error(None, e)).unwrap_or_default())
            }
        }
    }
}

/// Handler com métodos built-in do Code Terminal
#[derive(Debug)]
pub struct TerminalRpcHandler {
    inner: RpcHandler,
    state: Option<std::sync::Arc<super::commands::RemoteState>>,
}

impl Default for TerminalRpcHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalRpcHandler {
    /// Criar handler com métodos do terminal
    pub fn new() -> Self {
        let handler = Self {
            inner: RpcHandler::new(),
            state: None,
        };
        handler.register_builtin_methods();
        handler
    }

    /// Criar handler com estado compartilhado
    pub fn with_state(state: std::sync::Arc<super::commands::RemoteState>) -> Self {
        let handler = Self {
            inner: RpcHandler::new(),
            state: Some(state.clone()),
        };
        handler.register_builtin_methods();

        // Registra comandos adicionais que usam o estado
        let inner = handler.inner.clone();
        tokio::spawn(async move {
            super::commands::register_remote_commands(&inner, state).await;
        });

        handler
    }

    /// Registrar métodos built-in
    fn register_builtin_methods(&self) {
        // Métodos serão registrados em uma task separada
        let inner = self.inner.clone();

        tokio::spawn(async move {
            // Ping - verificar se servidor está vivo
            inner
                .register("ping", |_params| async { Ok(serde_json::json!("pong")) })
                .await;

            // Listar métodos disponíveis
            let inner_clone = inner.clone();
            inner
                .register("rpc.listMethods", move |_params| {
                    let inner = inner_clone.clone();
                    async move {
                        let methods = inner.list_methods().await;
                        Ok(serde_json::to_value(methods).unwrap())
                    }
                })
                .await;

            // Info do servidor
            inner
                .register("server.info", |_params| async {
                    Ok(serde_json::json!({
                        "name": "Code Terminal",
                        "version": env!("CARGO_PKG_VERSION"),
                        "protocol": "jsonrpc-2.0"
                    }))
                })
                .await;
        });
    }

    /// Delegar para handler interno
    pub async fn handle_message(&self, message: Message) -> Option<Message> {
        self.inner.handle_message(message).await
    }

    /// Delegar processamento JSON
    pub async fn process_json(&self, json: &str) -> Option<String> {
        self.inner.process_json(json).await
    }

    /// Registrar método customizado
    pub async fn register<F, Fut>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, RpcError>> + Send + 'static,
    {
        self.inner.register(name, handler).await;
    }

    /// Retorna referência ao handler interno
    pub fn inner(&self) -> &RpcHandler {
        &self.inner
    }

    /// Retorna referência ao estado (se configurado)
    pub fn state(&self) -> Option<&std::sync::Arc<super::commands::RemoteState>> {
        self.state.as_ref()
    }
}

/// Resultado de processamento de mensagem
#[derive(Debug, Clone)]
pub enum HandlerResult {
    /// Resposta a ser enviada
    Response(String),
    /// Notificação processada (sem resposta)
    Acknowledged,
    /// Erro no processamento
    Error(String),
}

/// Middleware para processamento de mensagens
pub struct RpcMiddleware {
    handler: Arc<RpcHandler>,
    pre_hooks: Vec<Box<dyn Fn(&Request) + Send + Sync>>,
    post_hooks: Vec<Box<dyn Fn(&Response) + Send + Sync>>,
}

impl std::fmt::Debug for RpcMiddleware {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcMiddleware")
            .field("handler", &self.handler)
            .field("pre_hooks", &self.pre_hooks.len())
            .field("post_hooks", &self.post_hooks.len())
            .finish()
    }
}

impl RpcMiddleware {
    /// Criar novo middleware
    pub fn new(handler: Arc<RpcHandler>) -> Self {
        Self {
            handler,
            pre_hooks: Vec::new(),
            post_hooks: Vec::new(),
        }
    }

    /// Adicionar hook pré-processamento
    pub fn before<F>(&mut self, hook: F)
    where
        F: Fn(&Request) + Send + Sync + 'static,
    {
        self.pre_hooks.push(Box::new(hook));
    }

    /// Adicionar hook pós-processamento
    pub fn after<F>(&mut self, hook: F)
    where
        F: Fn(&Response) + Send + Sync + 'static,
    {
        self.post_hooks.push(Box::new(hook));
    }

    /// Processar mensagem com hooks
    pub async fn process(&self, message: Message) -> Option<Message> {
        // Executar pré-hooks
        if let Message::Request(ref req) = message {
            for hook in &self.pre_hooks {
                hook(req);
            }
        }

        // Processar
        let result = self.handler.handle_message(message).await;

        // Executar pós-hooks
        if let Some(Message::Response(ref resp)) = result {
            for hook in &self.post_hooks {
                hook(resp);
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::protocol::error_codes;

    #[tokio::test]
    async fn test_handler_registration() {
        let handler = RpcHandler::new();

        handler
            .register("test.method", |_params| async {
                Ok(serde_json::json!("result"))
            })
            .await;

        assert!(handler.has_method("test.method").await);
        assert!(!handler.has_method("other.method").await);
    }

    #[tokio::test]
    async fn test_handle_request() {
        let handler = RpcHandler::new();

        handler
            .register("echo", |params| async move { Ok(params) })
            .await;

        let request = Request {
            jsonrpc: "2.0".to_string(),
            id: Some(Id::Num(1)),
            method: "echo".to_string(),
            params: Some(serde_json::json!("hello")),
        };

        let response = handler.handle_request(request).await.unwrap();
        assert!(response.is_success());
        assert_eq!(response.id, Some(Id::Num(1)));
    }

    #[tokio::test]
    async fn test_handle_unknown_method() {
        let handler = RpcHandler::new();

        let request = Request {
            jsonrpc: "2.0".to_string(),
            id: Some(Id::Num(1)),
            method: "unknown".to_string(),
            params: None,
        };

        let response = handler.handle_request(request).await.unwrap();
        assert!(response.is_error());
        assert_eq!(response.error.unwrap().code, error_codes::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn test_handle_notification() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let handler = RpcHandler::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        handler
            .register("notify", move |_params| {
                called_clone.store(true, Ordering::SeqCst);
                async move { Ok(serde_json::json!(null)) }
            })
            .await;

        let notification = Request {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notify".to_string(),
            params: None,
        };

        // Notificações não retornam resposta
        let response = handler.handle_request(notification).await;
        assert!(response.is_none());
    }

    #[tokio::test]
    async fn test_process_json() {
        let handler = RpcHandler::new();

        handler
            .register("test", |_params| async {
                Ok(serde_json::json!({"status": "ok"}))
            })
            .await;

        let json = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{}}"#;
        let response = handler.process_json(json).await.unwrap();

        assert!(response.contains("ok"));
        assert!(response.contains("2.0"));
    }

    #[tokio::test]
    async fn test_process_invalid_json() {
        let handler = RpcHandler::new();

        let json = "not valid json";
        let response = handler.process_json(json).await.unwrap();

        assert!(response.contains("error"));
        assert!(response.contains("-32700")); // Parse error
    }

    #[tokio::test]
    async fn test_list_methods() {
        let handler = RpcHandler::new();

        handler
            .register("method1", |_p| async { Ok(Value::Null) })
            .await;
        handler
            .register("method2", |_p| async { Ok(Value::Null) })
            .await;

        let methods = handler.list_methods().await;
        assert_eq!(methods.len(), 2);
        assert!(methods.contains(&"method1".to_string()));
        assert!(methods.contains(&"method2".to_string()));
    }

    #[tokio::test]
    async fn test_batch_request() {
        let handler = RpcHandler::new();

        handler
            .register("add", |params| async move {
                let nums: Vec<i32> = serde_json::from_value(params).unwrap_or_default();
                let sum: i32 = nums.iter().sum();
                Ok(serde_json::to_value(sum).unwrap())
            })
            .await;

        let json = r#"[{"jsonrpc":"2.0","id":1,"method":"add","params":[1,2]},{"jsonrpc":"2.0","id":2,"method":"add","params":[3,4]}]"#;
        let response = handler.process_json(json).await.unwrap();

        // Deve retornar batch response
        assert!(response.starts_with('['));
        assert!(response.contains("3")); // 1+2
        assert!(response.contains("7")); // 3+4
    }

    fn no_token() -> Credentials {
        Credentials {
            token: None,
            client_ip: None,
        }
    }

    fn request(id: Option<Id>, method: &str) -> Request {
        Request {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params: None,
        }
    }

    /// Espera os métodos registrados em background (tokio::spawn) aparecerem.
    async fn wait_for_method(handler: &RpcHandler, name: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !handler.has_method(name).await {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("método não foi registrado a tempo");
    }

    #[tokio::test]
    async fn test_default_debug_and_clone_drop_auth() {
        let handler = RpcHandler::default();
        assert!(!handler.is_auth_enabled());
        assert!(format!("{:?}", handler).contains("auth_enabled: false"));

        let authed = RpcHandler::with_auth(AuthConfig {
            enabled: true,
            token: Some("segredo".to_string()),
            ..AuthConfig::default()
        });
        assert!(authed.is_auth_enabled());
        assert!(format!("{:?}", authed).contains("auth_enabled: true"));

        authed.register("m", |_| async { Ok(Value::Null) }).await;
        let cloned = authed.clone();
        // Clone compartilha os métodos mas não o verificador de auth
        assert!(cloned.has_method("m").await);
        assert!(!cloned.is_auth_enabled());
    }

    #[tokio::test]
    async fn test_authenticate_with_and_without_verifier() {
        let open = RpcHandler::new();
        let ctx = open.authenticate(&no_token(), None).await.unwrap();
        assert!(!ctx.is_authenticated());

        let disabled = RpcHandler::with_auth(AuthConfig::default());
        assert!(!disabled.is_auth_enabled());

        let secured = RpcHandler::with_auth(AuthConfig {
            enabled: true,
            token: Some("segredo".to_string()),
            ..AuthConfig::default()
        });
        let error = secured.authenticate(&no_token(), None).await.unwrap_err();
        assert!(matches!(error, AuthError::AuthenticationRequired));

        let ctx = secured
            .authenticate(&Credentials::with_token("segredo"), None)
            .await
            .unwrap();
        assert!(ctx.is_authenticated());
    }

    #[tokio::test]
    async fn test_handle_message_ignores_responses_and_rejects_invalid_requests() {
        let handler = RpcHandler::new();
        let response = Response::success(Id::Num(1), "x").unwrap();
        assert!(handler
            .handle_message(Message::Response(response.clone()))
            .await
            .is_none());
        assert!(handler
            .handle_message(Message::BatchResponse(vec![response]))
            .await
            .is_none());

        let mut invalid = request(Some(Id::Num(9)), "m");
        invalid.jsonrpc = "1.0".to_string();
        let response = handler.handle_request(invalid).await.unwrap();
        assert_eq!(response.id, Some(Id::Num(9)));
        assert_eq!(response.error.unwrap().code, error_codes::INVALID_REQUEST);
    }

    #[tokio::test]
    async fn test_handle_request_propagates_method_errors() {
        let handler = RpcHandler::new();
        handler
            .register("fail", |_| async { Err(RpcError::invalid_params("ruim")) })
            .await;
        let response = handler
            .handle_request(request(Some(Id::Num(2)), "fail"))
            .await
            .unwrap();
        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::INVALID_PARAMS);
        assert_eq!(error.message, "ruim");
    }

    #[tokio::test]
    async fn test_batch_of_notifications_has_no_response() {
        let handler = RpcHandler::new();
        handler.register("n", |_| async { Ok(Value::Null) }).await;
        let json = r#"[{"jsonrpc":"2.0","method":"n"},{"jsonrpc":"2.0","method":"n"}]"#;
        assert!(handler.process_json(json).await.is_none());
    }

    #[tokio::test]
    async fn test_terminal_rpc_handler_builtins() {
        let handler = TerminalRpcHandler::default();
        assert!(handler.state().is_none());
        wait_for_method(handler.inner(), "server.info").await;
        wait_for_method(handler.inner(), "rpc.listMethods").await;
        wait_for_method(handler.inner(), "ping").await;

        handler
            .register("custom", |_| async { Ok(serde_json::json!(42)) })
            .await;

        let pong = handler
            .process_json(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#)
            .await
            .unwrap();
        assert!(pong.contains("\"pong\""), "{pong}");

        let info = handler
            .handle_message(Message::Request(request(Some(Id::Num(2)), "server.info")))
            .await;
        let Some(Message::Response(info)) = info else {
            panic!("esperava resposta");
        };
        let info = info.result.unwrap();
        assert_eq!(info["name"], "Code Terminal");
        assert_eq!(info["protocol"], "jsonrpc-2.0");

        let methods = handler
            .inner()
            .execute_method("rpc.listMethods", None)
            .await
            .unwrap();
        let methods: Vec<String> = serde_json::from_value(methods).unwrap();
        assert!(methods.contains(&"custom".to_string()));
        assert!(methods.contains(&"ping".to_string()));
    }

    #[tokio::test]
    async fn test_terminal_rpc_handler_with_state_registers_remote_commands() {
        let state = Arc::new(super::super::commands::RemoteState::new());
        let handler = TerminalRpcHandler::with_state(state.clone());
        assert!(Arc::ptr_eq(handler.state().unwrap(), &state));
        wait_for_method(handler.inner(), "tab.new").await;

        let reply = handler
            .process_json(r#"{"jsonrpc":"2.0","id":1,"method":"tab.new","params":{"title":"rpc"}}"#)
            .await
            .unwrap();
        assert!(reply.contains("\"result\":0"), "{reply}");
        assert_eq!(state.tabs.read().await[0].title, "rpc");
    }

    #[tokio::test]
    async fn test_middleware_runs_hooks_around_requests() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let handler = Arc::new(RpcHandler::new());
        handler
            .register("echo", |params| async move { Ok(params) })
            .await;

        let before = Arc::new(AtomicUsize::new(0));
        let after = Arc::new(AtomicUsize::new(0));
        let mut middleware = RpcMiddleware::new(handler);
        let before_clone = before.clone();
        middleware.before(move |req| {
            assert_eq!(req.method, "echo");
            before_clone.fetch_add(1, Ordering::SeqCst);
        });
        let after_clone = after.clone();
        middleware.after(move |resp| {
            assert!(resp.is_success());
            after_clone.fetch_add(1, Ordering::SeqCst);
        });
        let debug = format!("{:?}", middleware);
        assert!(
            debug.contains("pre_hooks: 1") && debug.contains("post_hooks: 1"),
            "{debug}"
        );

        let result = middleware
            .process(Message::Request(request(Some(Id::Num(1)), "echo")))
            .await;
        assert!(matches!(result, Some(Message::Response(_))));
        assert_eq!(before.load(Ordering::SeqCst), 1);
        assert_eq!(after.load(Ordering::SeqCst), 1);

        // Notificação: roda o hook "before", mas não há resposta para o "after"
        let result = middleware
            .process(Message::Request(request(None, "echo")))
            .await;
        assert!(result.is_none());
        assert_eq!(before.load(Ordering::SeqCst), 2);
        assert_eq!(after.load(Ordering::SeqCst), 1);

        // Batch não dispara hooks de requisição única
        let result = middleware
            .process(Message::BatchRequest(vec![request(
                Some(Id::Num(3)),
                "echo",
            )]))
            .await;
        assert!(matches!(result, Some(Message::BatchResponse(_))));
        assert_eq!(before.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_handler_result_variants() {
        let variants = [
            HandlerResult::Response("{}".to_string()),
            HandlerResult::Acknowledged,
            HandlerResult::Error("x".to_string()),
        ];
        let debug: Vec<String> = variants
            .iter()
            .map(|v| format!("{:?}", v.clone()))
            .collect();
        assert_eq!(debug[1], "Acknowledged");
        assert!(debug[2].contains("\"x\""));
    }
}
