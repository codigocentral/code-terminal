use code_terminal::plugin::{Plugin, PluginContext, PluginInfo, PluginResult};
use std::collections::HashMap;
use std::time::SystemTime;

type CommandHandler = fn(&mut PluginContext, &[&str]);

#[derive(Debug)]
pub struct CommandPlugin {
    commands: HashMap<String, CommandHandler>,
    prefix: String,
    case_sensitive: bool,
}

impl Default for CommandPlugin {
    fn default() -> Self {
        let mut plugin = Self {
            commands: HashMap::new(),
            prefix: "/".to_string(),
            case_sensitive: false,
        };
        plugin.register_builtin_commands();
        plugin
    }
}

impl CommandPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn register_builtin_commands(&mut self) {
        self.register("hello", Self::cmd_hello);
        self.register("time", Self::cmd_time);
        self.register("echo", Self::cmd_echo);
        self.register("help", Self::cmd_help);
    }

    fn register(&mut self, name: &str, handler: CommandHandler) {
        let key = if self.case_sensitive {
            format!("{}{}", self.prefix, name)
        } else {
            format!("{}{}", self.prefix, name.to_lowercase())
        };
        self.commands.insert(key, handler);
    }

    fn cmd_hello(_ctx: &mut PluginContext, args: &[&str]) {
        if args.is_empty() {
            println!("Olá! Use /hello <nome> para uma saudação personalizada.");
        } else {
            println!("Olá, {}! Bem-vindo ao Codeterm!", args.join(" "));
        }
    }

    fn cmd_time(_ctx: &mut PluginContext, _args: &[&str]) {
        match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => {
                let secs = duration.as_secs();
                let hours = (secs % 86400) / 3600;
                let minutes = (secs % 3600) / 60;
                let seconds = secs % 60;
                println!(
                    "Horário atual: {:02}:{:02}:{:02} UTC",
                    hours, minutes, seconds
                );
            }
            Err(e) => {
                println!("Erro ao obter horário: {}", e);
            }
        }
    }

    fn cmd_echo(_ctx: &mut PluginContext, args: &[&str]) {
        println!("{}", args.join(" "));
    }

    fn cmd_help(_ctx: &mut PluginContext, _args: &[&str]) {
        println!("Comandos disponíveis:");
        println!("  /hello [nome]  - Exibe saudação");
        println!("  /time          - Mostra horário atual");
        println!("  /echo <texto>  - Repete o texto");
        println!("  /help          - Exibe esta ajuda");
    }
}

impl Plugin for CommandPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "command-plugin".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "Codeterm Team".to_string(),
            description: "Plugin que registra comandos customizados".to_string(),
            homepage: None,
            repository: None,
        }
    }

    fn init(&mut self, ctx: &mut PluginContext) -> PluginResult<()> {
        ctx.log_info("CommandPlugin carregado!");
        ctx.log_info(&format!(
            "Comandos: {}",
            self.commands.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
        Ok(())
    }

    fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {}

    fn shutdown(&mut self, ctx: &mut PluginContext) {
        ctx.log_info("CommandPlugin encerrado.");
    }

    fn on_input(&mut self, ctx: &mut PluginContext, input: &str) -> bool {
        let input_lower = input.to_lowercase();
        let input_to_check = if self.case_sensitive {
            input
        } else {
            &input_lower
        };

        let parts: Vec<&str> = input_to_check.split_whitespace().collect();
        if parts.is_empty() {
            return false;
        }

        let cmd = parts[0];
        if let Some(&handler) = self.commands.get(cmd) {
            let original_parts: Vec<&str> = input.split_whitespace().skip(1).collect();
            handler(ctx, &original_parts);
            return true;
        }

        false
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("prefix".to_string(), "/".to_string());
        config.insert("case_sensitive".to_string(), "false".to_string());
        config
    }

    fn apply_config(&mut self, config: &HashMap<String, String>) {
        if let Some(prefix) = config.get("prefix") {
            self.prefix = prefix.clone();
            self.commands.clear();
            self.register_builtin_commands();
        }
        if let Some(cs) = config.get("case_sensitive") {
            self.case_sensitive = cs == "true";
        }
    }
}

#[no_mangle]
pub extern "C" fn create_plugin() -> *mut dyn Plugin {
    let plugin = Box::new(CommandPlugin::new());
    Box::into_raw(plugin)
}
