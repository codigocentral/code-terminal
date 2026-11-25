use code_terminal::plugin::{Plugin, PluginContext, PluginInfo, PluginResult};

#[derive(Debug, Default)]
pub struct HelloPlugin {
    init_count: u32,
}

impl Plugin for HelloPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "hello-plugin".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "Codeterm Team".to_string(),
            description: "Plugin de exemplo que imprime saudações".to_string(),
            homepage: None,
            repository: None,
        }
    }

    fn init(&mut self, ctx: &mut PluginContext) -> PluginResult<()> {
        self.init_count += 1;
        ctx.log_info(&format!(
            "Olá! HelloPlugin inicializado (vez #{})",
            self.init_count
        ));
        Ok(())
    }

    fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {}

    fn shutdown(&mut self, ctx: &mut PluginContext) {
        ctx.log_info("Adeus! HelloPlugin encerrado.");
    }
}

impl HelloPlugin {
    pub fn new() -> Self {
        Self::default()
    }
}

#[no_mangle]
pub extern "C" fn create_plugin() -> *mut dyn Plugin {
    let plugin = Box::new(HelloPlugin::new());
    Box::into_raw(plugin)
}
