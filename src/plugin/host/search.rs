//! Host Functions - Search
//!
//! Funções para busca de texto no terminal.

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};

pub fn all_functions() -> Vec<Box<dyn HostFunction>> {
    vec![
        Box::new(SearchFind),
        Box::new(SearchNext),
        Box::new(SearchPrev),
        Box::new(SearchClear),
        Box::new(GetSearchResults),
    ]
}

/// Contexto de execução para funções de busca
static SEARCH_CONTEXT: std::sync::Mutex<SearchContext> =
    std::sync::Mutex::new(SearchContext::new());

pub struct SearchContext {
    query: String,
    results: Vec<SearchResult>,
    current_index: usize,
    case_sensitive: bool,
}

impl Default for SearchContext {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchContext {
    pub const fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            current_index: 0,
            case_sensitive: false,
        }
    }

    pub fn find(&mut self, query: &str, case_sensitive: bool) -> usize {
        self.query = query.to_string();
        self.case_sensitive = case_sensitive;
        self.results.clear();
        self.current_index = 0;

        // Simulação: em implementação real, buscaria no buffer do terminal
        // Por enquanto retorna 0 resultados
        0
    }

    pub fn next(&mut self) -> Option<&SearchResult> {
        if self.results.is_empty() {
            return None;
        }
        self.current_index = (self.current_index + 1) % self.results.len();
        self.results.get(self.current_index)
    }

    pub fn prev(&mut self) -> Option<&SearchResult> {
        if self.results.is_empty() {
            return None;
        }
        if self.current_index == 0 {
            self.current_index = self.results.len() - 1;
        } else {
            self.current_index -= 1;
        }
        self.results.get(self.current_index)
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.results.clear();
        self.current_index = 0;
    }

    pub fn results(&self) -> &[SearchResult] {
        &self.results
    }

    pub fn current(&self) -> Option<&SearchResult> {
        self.results.get(self.current_index)
    }
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub text: String,
}

pub struct SearchFind;

impl HostFunction for SearchFind {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "search_find",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength, // query
                HostParamType::I32,               // case_sensitive (0/1)
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }

    fn description(&self) -> &'static str {
        "Busca texto no terminal (query, case_sensitive) -> count"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 8 {
            return Err(HostFunctionError::InvalidParams);
        }

        // Parse query length and content
        let query_len = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as usize;
        let query_start = 4usize;
        // Tamanho negativo vira usize enorme: soma verificada evita pânico
        let query_end = query_start
            .checked_add(query_len)
            .ok_or(HostFunctionError::InvalidParams)?;

        if params.len() < query_end.saturating_add(4) {
            return Err(HostFunctionError::InvalidParams);
        }

        let query = String::from_utf8(params[query_start..query_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let case_sensitive = i32::from_le_bytes([
            params[query_end],
            params[query_end + 1],
            params[query_end + 2],
            params[query_end + 3],
        ]) != 0;

        let mut ctx = SEARCH_CONTEXT.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock search context".to_string())
        })?;

        let count = ctx.find(&query, case_sensitive);
        Ok((count as i32).to_le_bytes().to_vec())
    }
}

pub struct SearchNext;

impl HostFunction for SearchNext {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "search_next",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer], // out: (line, start_col, end_col)
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }

    fn description(&self) -> &'static str {
        "Próximo resultado de busca -> 1=sucesso, 0=sem resultados"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let mut ctx = SEARCH_CONTEXT.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock search context".to_string())
        })?;

        match ctx.next() {
            Some(result) => {
                // Retorna: [line: i32, start_col: i32, end_col: i32]
                let mut output = Vec::with_capacity(12);
                output.extend_from_slice(&(result.line as i32).to_le_bytes());
                output.extend_from_slice(&(result.start_col as i32).to_le_bytes());
                output.extend_from_slice(&(result.end_col as i32).to_le_bytes());
                Ok(output)
            }
            None => Ok((-1i32).to_le_bytes().to_vec()),
        }
    }
}

pub struct SearchPrev;

impl HostFunction for SearchPrev {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "search_prev",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer], // out: (line, start_col, end_col)
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }

    fn description(&self) -> &'static str {
        "Resultado anterior de busca -> 1=sucesso, 0=sem resultados"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let mut ctx = SEARCH_CONTEXT.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock search context".to_string())
        })?;

        match ctx.prev() {
            Some(result) => {
                let mut output = Vec::with_capacity(12);
                output.extend_from_slice(&(result.line as i32).to_le_bytes());
                output.extend_from_slice(&(result.start_col as i32).to_le_bytes());
                output.extend_from_slice(&(result.end_col as i32).to_le_bytes());
                Ok(output)
            }
            None => Ok((-1i32).to_le_bytes().to_vec()),
        }
    }
}

pub struct SearchClear;

impl HostFunction for SearchClear {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "search_clear",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }

    fn description(&self) -> &'static str {
        "Limpa busca atual"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let mut ctx = SEARCH_CONTEXT.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock search context".to_string())
        })?;
        ctx.clear();
        Ok(0i32.to_le_bytes().to_vec())
    }
}

pub struct GetSearchResults;

impl HostFunction for GetSearchResults {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "get_search_results",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }

    fn description(&self) -> &'static str {
        "Obtém todos os resultados como JSON (out_ptr, out_len) -> bytes escritos"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 4 {
            return Err(HostFunctionError::InvalidParams);
        }

        let _max_len = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as usize;

        let ctx = SEARCH_CONTEXT.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock search context".to_string())
        })?;

        // Cria JSON com resultados
        let results: Vec<_> = ctx
            .results()
            .iter()
            .map(|r| {
                serde_json::json!({
                    "line": r.line,
                    "start_col": r.start_col,
                    "end_col": r.end_col,
                    "text": &r.text,
                })
            })
            .collect();

        let json_str = serde_json::to_string(&results)
            .map_err(|e| HostFunctionError::ExecutionError(format!("JSON error: {}", e)))?;

        Ok(json_str.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_functions_exist() {
        let functions = all_functions();
        assert_eq!(functions.len(), 5);
    }

    #[test]
    fn test_search_context_find() {
        let mut ctx = SearchContext::new();
        let count = ctx.find("test", false);
        assert_eq!(count, 0); // Sem terminal real, retorna 0
    }

    #[test]
    fn test_search_context_navigation() {
        let mut ctx = SearchContext::new();

        // Sem resultados, next/prev retornam None
        assert!(ctx.next().is_none());
        assert!(ctx.prev().is_none());
    }

    #[test]
    fn test_search_clear() {
        let mut ctx = SearchContext::new();
        ctx.find("query", false);
        ctx.clear();

        assert!(ctx.query.is_empty());
        assert!(ctx.results().is_empty());
    }

    /// Os testes abaixo usam o contexto global: rodam em série.
    static CONTEXT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        CONTEXT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn result(line: usize) -> SearchResult {
        SearchResult {
            line,
            start_col: line * 2,
            end_col: line * 2 + 3,
            text: format!("hit{line}"),
        }
    }

    fn with_results(lines: &[usize]) {
        let mut ctx = SEARCH_CONTEXT.lock().unwrap();
        ctx.clear();
        ctx.results = lines.iter().map(|l| result(*l)).collect();
    }

    fn triple(bytes: Vec<u8>) -> Vec<i32> {
        bytes
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn find_params(query: &[u8], case_sensitive: i32) -> Vec<u8> {
        let mut p = (query.len() as i32).to_le_bytes().to_vec();
        p.extend_from_slice(query);
        p.extend(case_sensitive.to_le_bytes());
        p
    }

    #[test]
    fn test_context_wraps_around_results() {
        let mut ctx = SearchContext::default();
        ctx.results = vec![result(1), result(2), result(3)];
        assert_eq!(ctx.current().unwrap().line, 1);
        assert_eq!(ctx.next().unwrap().line, 2);
        assert_eq!(ctx.next().unwrap().line, 3);
        assert_eq!(ctx.next().unwrap().line, 1);
        assert_eq!(ctx.prev().unwrap().line, 3);
        assert_eq!(ctx.prev().unwrap().line, 2);
    }

    #[test]
    fn test_find_records_query_and_resets_state() {
        let _guard = serial();
        with_results(&[5]);
        let out = SearchFind.execute(&find_params(b"erro", 1)).unwrap();
        assert_eq!(i32::from_le_bytes(out.try_into().unwrap()), 0);
        let ctx = SEARCH_CONTEXT.lock().unwrap();
        assert_eq!(ctx.query, "erro");
        assert!(ctx.case_sensitive);
        assert!(ctx.results().is_empty());
    }

    #[test]
    fn test_find_rejects_malformed_params() {
        let invalid = Err(HostFunctionError::InvalidParams);
        assert_eq!(SearchFind.execute(&[0; 7]), invalid);
        let mut p = 50i32.to_le_bytes().to_vec();
        p.extend([0; 4]);
        assert_eq!(SearchFind.execute(&p), invalid);
        assert_eq!(SearchFind.execute(&find_params(&[0xff], 0)), invalid);
        // Regressão: tamanhos negativos estouravam a soma de offsets
        for len in [-1i32, -4] {
            let mut p = len.to_le_bytes().to_vec();
            p.extend([0; 8]);
            assert_eq!(SearchFind.execute(&p), invalid, "{len}");
        }
    }

    #[test]
    fn test_next_prev_and_results_through_host_functions() {
        let _guard = serial();
        with_results(&[]);
        assert_eq!(SearchNext.execute(&[]).unwrap(), (-1i32).to_le_bytes());
        assert_eq!(SearchPrev.execute(&[]).unwrap(), (-1i32).to_le_bytes());

        with_results(&[1, 4]);
        assert_eq!(triple(SearchNext.execute(&[]).unwrap()), vec![4, 8, 11]);
        assert_eq!(triple(SearchPrev.execute(&[]).unwrap()), vec![1, 2, 5]);

        let json: Vec<serde_json::Value> =
            serde_json::from_slice(&GetSearchResults.execute(&64i32.to_le_bytes()).unwrap())
                .unwrap();
        assert_eq!(json.len(), 2);
        assert_eq!(json[1]["text"], "hit4");
        assert_eq!(json[1]["end_col"], 11);
        assert_eq!(
            GetSearchResults.execute(&[1]),
            Err(HostFunctionError::InvalidParams)
        );

        assert_eq!(SearchClear.execute(&[]).unwrap(), 0i32.to_le_bytes());
        assert!(SEARCH_CONTEXT.lock().unwrap().results().is_empty());
    }
}
