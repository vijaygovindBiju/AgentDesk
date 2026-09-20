//! CLI argument parsing and configuration options.
//! See docs/TODO.md P6.5, P6.6, P6.7.

use std::path::PathBuf;
use agentdesk_model::PipelineMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Run(RunOptions),
    TokenShow(TokenOptions),
    TokenRotate(TokenOptions),
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenOptions {
    pub config_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub scenario: Option<PathBuf>,
    pub seed: u64,
    pub mode: PipelineMode,
    pub insecure_dev: bool,
    pub bind: String,
    pub port: u16,
    pub token: Option<String>,
    pub config_dir: Option<PathBuf>,
    pub debug: bool,
    pub agent: Option<String>,
    pub prompt: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            scenario: None,
            seed: 42,
            mode: PipelineMode::Agentdesk,
            insecure_dev: false,
            bind: "127.0.0.1".into(),
            port: 8765,
            token: None,
            config_dir: None,
            debug: false,
            agent: None,
            prompt: None,
        }
    }
}

pub fn parse_args<I, T>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    if args.len() <= 1 {
        return Ok(CliCommand::Help);
    }

    let first = &args[1];
    if first == "-h" || first == "--help" || first == "help" {
        return Ok(CliCommand::Help);
    }

    if first == "token" {
        if args.len() < 3 {
            return Err("Usage: agentdesk token <show|rotate> [--config-dir <path>]".into());
        }
        let sub = &args[2];
        let mut config_dir = None;
        let mut idx = 3;
        while idx < args.len() {
            if args[idx] == "--config-dir" && idx + 1 < args.len() {
                config_dir = Some(PathBuf::from(&args[idx + 1]));
                idx += 2;
            } else {
                return Err(format!("Unknown option for token: '{}'", args[idx]));
            }
        }

        let opts = TokenOptions { config_dir };
        match sub.as_str() {
            "show" => return Ok(CliCommand::TokenShow(opts)),
            "rotate" => return Ok(CliCommand::TokenRotate(opts)),
            other => return Err(format!("Unknown token command: '{}'. Expected 'show' or 'rotate'.", other)),
        }
    }

    if first == "run" {
        let mut opts = RunOptions::default();
        let mut idx = 2;
        while idx < args.len() {
            match args[idx].as_str() {
                "--scenario" => {
                    if idx + 1 >= args.len() {
                        return Err("--scenario requires a file path".into());
                    }
                    opts.scenario = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                }
                "--seed" => {
                    if idx + 1 >= args.len() {
                        return Err("--seed requires a numeric value".into());
                    }
                    opts.seed = args[idx + 1]
                        .parse()
                        .map_err(|_| format!("Invalid seed: '{}'", args[idx + 1]))?;
                    idx += 2;
                }
                "--mode" => {
                    if idx + 1 >= args.len() {
                        return Err("--mode requires raw_lines, raw_events, or agentdesk".into());
                    }
                    opts.mode = match args[idx + 1].as_str() {
                        "raw_lines" => PipelineMode::RawLines,
                        "raw_events" => PipelineMode::RawEvents,
                        "agentdesk" => PipelineMode::Agentdesk,
                        other => return Err(format!("Unknown mode '{}'", other)),
                    };
                    idx += 2;
                }
                "--insecure-dev" => {
                    opts.insecure_dev = true;
                    idx += 1;
                }
                "--bind" => {
                    if idx + 1 >= args.len() {
                        return Err("--bind requires an address".into());
                    }
                    opts.bind = args[idx + 1].clone();
                    idx += 2;
                }
                "--port" => {
                    if idx + 1 >= args.len() {
                        return Err("--port requires a port number".into());
                    }
                    opts.port = args[idx + 1]
                        .parse()
                        .map_err(|_| format!("Invalid port: '{}'", args[idx + 1]))?;
                    idx += 2;
                }
                "--token" => {
                    if idx + 1 >= args.len() {
                        return Err("--token requires a token string".into());
                    }
                    opts.token = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "--config-dir" => {
                    if idx + 1 >= args.len() {
                        return Err("--config-dir requires a path".into());
                    }
                    opts.config_dir = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                }
                "--debug" => {
                    opts.debug = true;
                    idx += 1;
                }
                "--agent" => {
                    if idx + 1 >= args.len() {
                        return Err("--agent requires a command string".into());
                    }
                    opts.agent = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "--prompt" => {
                    if idx + 1 >= args.len() {
                        return Err("--prompt requires a prompt string".into());
                    }
                    opts.prompt = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "-h" | "--help" => return Ok(CliCommand::Help),
                other => return Err(format!("Unknown argument: '{}'", other)),
            }
        }
        return Ok(CliCommand::Run(opts));
    }

    Err(format!("Unknown command: '{}'. Expected 'run' or 'token'.", first))
}

pub fn print_help() {
    eprintln!(
        "AgentDesk Daemon\n\n\
         USAGE:\n\
             agentdesk run [OPTIONS]\n\
             agentdesk token <show|rotate> [--config-dir <path>]\n\n\
         RUN OPTIONS:\n\
             --scenario <path>        Path to simulator scenario JSON (default: standard scenario)\n\
             --seed <u64>             Deterministic random seed (default: 42)\n\
             --mode <mode>            Pipeline mode: raw_lines | raw_events | agentdesk (default: agentdesk)\n\
             --insecure-dev           Run plain ws:// on 127.0.0.1 (strict loopback only)\n\
             --bind <addr>            Bind address (default: 127.0.0.1)\n\
             --port <u16>             Bind port (default: 8765)\n\
             --token <str>            Override authentication token\n\
             --config-dir <path>      Override configuration directory for token storage\n\
             --debug                  Enable payload debug logging (opt-in)\n\
             --agent <cmd>            Run real agent via ACP over stdio (e.g. \"gemini --skip-trust --acp\")\n\
             --prompt <text>          Prompt to send to the real agent\n\
             -h, --help               Print help information\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_help_command() {
        assert_eq!(parse_args(["agentdesk", "--help"]).unwrap(), CliCommand::Help);
        assert_eq!(parse_args(["agentdesk"]).unwrap(), CliCommand::Help);
    }

    #[test]
    fn parse_token_commands() {
        assert_eq!(
            parse_args(["agentdesk", "token", "show"]).unwrap(),
            CliCommand::TokenShow(TokenOptions { config_dir: None })
        );
        assert_eq!(
            parse_args(["agentdesk", "token", "rotate", "--config-dir", "/tmp/cfg"]).unwrap(),
            CliCommand::TokenRotate(TokenOptions {
                config_dir: Some(PathBuf::from("/tmp/cfg"))
            })
        );
    }

    #[test]
    fn parse_run_command_flags() {
        let cmd = parse_args([
            "agentdesk",
            "run",
            "--seed",
            "123",
            "--mode",
            "raw_events",
            "--insecure-dev",
            "--port",
            "9001",
            "--debug",
        ])
        .unwrap();

        match cmd {
            CliCommand::Run(opts) => {
                assert_eq!(opts.seed, 123);
                assert_eq!(opts.mode, PipelineMode::RawEvents);
                assert!(opts.insecure_dev);
                assert_eq!(opts.port, 9001);
                assert!(opts.debug);
            }
            _ => panic!("expected run command"),
        }
    }

    #[test]
    fn parse_run_with_agent() {
        let cmd = parse_args([
            "agentdesk",
            "run",
            "--agent",
            "gemini --skip-trust --acp",
            "--prompt",
            "hello world",
        ])
        .unwrap();

        match cmd {
            CliCommand::Run(opts) => {
                assert_eq!(opts.agent.as_deref(), Some("gemini --skip-trust --acp"));
                assert_eq!(opts.prompt.as_deref(), Some("hello world"));
            }
            _ => panic!("expected run command"),
        }
    }
}
