use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use crate::installed_app::InstalledApp;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug)]
pub struct UninstallTarget {
    pub app_id: String,
    pub name: String,
    pub raw_command: String,
    pub command: String,
    pub launch_mode: CommandLaunchMode,
    pub assessment: UninstallAssessment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UninstallAssessment {
    pub status: UninstallReadiness,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UninstallReadiness {
    Verified,
    NeedsShell,
    Unsupported,
}

impl UninstallAssessment {
    pub fn is_selectable(&self) -> bool {
        !matches!(self.status, UninstallReadiness::Unsupported)
    }

    pub fn label(&self) -> &'static str {
        match self.status {
            UninstallReadiness::Verified => "검증됨",
            UninstallReadiness::NeedsShell => "셸 검증 제한",
            UninstallReadiness::Unsupported => "실행 불가",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandLaunchMode {
    Direct,
    Shell,
}

impl CommandLaunchMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Direct => "직접 실행",
            Self::Shell => "셸 실행",
        }
    }
}

#[derive(Debug)]
pub enum UninstallEvent {
    Launching {
        app_id: String,
        index: usize,
        total: usize,
        name: String,
    },
    Launched {
        app_id: String,
        name: String,
        pid: u32,
    },
    Exited {
        app_id: String,
        name: String,
        code: Option<i32>,
    },
    Failed {
        app_id: String,
        name: String,
        message: String,
        elevation_required: bool,
        target: Option<UninstallTarget>,
    },
    Done,
}

impl From<&InstalledApp> for UninstallTarget {
    fn from(app: &InstalledApp) -> Self {
        Self {
            app_id: app.id.clone(),
            name: app.display_name.clone(),
            raw_command: app.uninstall_string.clone(),
            command: make_uninstall_command(&app.uninstall_string),
            launch_mode: launch_mode_for_command(&app.uninstall_string),
            assessment: assess_uninstall_command(&app.uninstall_string),
        }
    }
}

pub fn spawn_uninstall_queue(targets: Vec<UninstallTarget>, sender: Sender<UninstallEvent>) {
    thread::spawn(move || {
        let total = targets.len();

        for (index, target) in targets.into_iter().enumerate() {
            let _ = sender.send(UninstallEvent::Launching {
                app_id: target.app_id.clone(),
                index: index + 1,
                total,
                name: target.name.clone(),
            });

            match spawn_target_process(&target, false) {
                Ok(mut child) => {
                    let pid = child.id();
                    let _ = sender.send(UninstallEvent::Launched {
                        app_id: target.app_id.clone(),
                        name: target.name.clone(),
                        pid,
                    });

                    match child.wait() {
                        Ok(status) => {
                            let _ = sender.send(UninstallEvent::Exited {
                                app_id: target.app_id,
                                name: target.name,
                                code: status.code(),
                            });
                        }
                        Err(error) => {
                            let _ = sender.send(UninstallEvent::Failed {
                                app_id: target.app_id.clone(),
                                name: target.name.clone(),
                                message: error.to_string(),
                                elevation_required: is_elevation_required_error(&error),
                                target: Some(target),
                            });
                        }
                    }
                }
                Err(error) => {
                    let elevation_required = is_elevation_required_error(&error);
                    let _ = sender.send(UninstallEvent::Failed {
                        app_id: target.app_id.clone(),
                        name: target.name.clone(),
                        message: error.to_string(),
                        elevation_required,
                        target: elevation_required.then_some(target),
                    });
                }
            }

            thread::sleep(Duration::from_millis(400));
        }

        let _ = sender.send(UninstallEvent::Done);
    });
}

pub fn spawn_elevated_uninstall(target: UninstallTarget, sender: Sender<UninstallEvent>) {
    thread::spawn(move || {
        let _ = sender.send(UninstallEvent::Launching {
            app_id: target.app_id.clone(),
            index: 1,
            total: 1,
            name: format!("{} (관리자 권한)", target.name),
        });

        match spawn_target_process(&target, true) {
            Ok(mut child) => {
                let pid = child.id();
                let _ = sender.send(UninstallEvent::Launched {
                    app_id: target.app_id.clone(),
                    name: target.name.clone(),
                    pid,
                });

                match child.wait() {
                    Ok(status) => {
                        let _ = sender.send(UninstallEvent::Exited {
                            app_id: target.app_id,
                            name: target.name,
                            code: status.code(),
                        });
                    }
                    Err(error) => {
                        let _ = sender.send(UninstallEvent::Failed {
                            app_id: target.app_id.clone(),
                            name: target.name.clone(),
                            message: error.to_string(),
                            elevation_required: is_elevation_required_error(&error),
                            target: Some(target),
                        });
                    }
                }
            }
            Err(error) => {
                let _ = sender.send(UninstallEvent::Failed {
                    app_id: target.app_id.clone(),
                    name: target.name.clone(),
                    message: error.to_string(),
                    elevation_required: is_elevation_required_error(&error),
                    target: Some(target),
                });
            }
        }

        let _ = sender.send(UninstallEvent::Done);
    });
}

pub fn make_uninstall_command(raw: &str) -> String {
    let trimmed = raw.trim();
    if looks_like_msiexec_install(trimmed) {
        replace_first_msi_install_flag(trimmed)
    } else {
        trimmed.to_owned()
    }
}

pub fn assess_uninstall_command(raw: &str) -> UninstallAssessment {
    let command = make_uninstall_command(raw);
    if command.trim().is_empty() {
        return UninstallAssessment {
            status: UninstallReadiness::Unsupported,
            detail: "제거 명령이 비어 있습니다.".to_owned(),
        };
    }

    if launch_mode_for_command(&command) == CommandLaunchMode::Shell {
        return UninstallAssessment {
            status: UninstallReadiness::NeedsShell,
            detail: "환경 변수나 셸 연산자가 있어 실행 전 파일 존재 검증은 제한됩니다.".to_owned(),
        };
    }

    let Some((program, _args)) = split_program_and_args(&command) else {
        return UninstallAssessment {
            status: UninstallReadiness::Unsupported,
            detail: "실행할 프로그램을 해석하지 못했습니다.".to_owned(),
        };
    };

    if is_known_windows_launcher(&program) || resolve_program(&program).is_some() {
        return UninstallAssessment {
            status: UninstallReadiness::Verified,
            detail: "실행 파일 또는 Windows 제거 런처를 확인했습니다.".to_owned(),
        };
    }

    UninstallAssessment {
        status: UninstallReadiness::Unsupported,
        detail: format!("실행 파일을 찾지 못했습니다: {program}"),
    }
}

fn spawn_target_process(
    target: &UninstallTarget,
    elevated: bool,
) -> std::io::Result<std::process::Child> {
    let mut command = if elevated {
        build_elevated_process_command(&target.command)
    } else {
        build_process_command(&target.command)
    };

    #[cfg(windows)]
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);

    command.spawn()
}

fn build_process_command(command_line: &str) -> Command {
    if launch_mode_for_command(command_line) == CommandLaunchMode::Shell {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", command_line]);
        return command;
    }

    let Some((program, args)) = split_program_and_args(command_line) else {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", command_line]);
        return command;
    };

    let mut command = Command::new(program);
    command.args(args);
    command
}

fn build_elevated_process_command(command_line: &str) -> Command {
    let script = format!(
        "$p = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/C', {}) -Verb RunAs -Wait -PassThru; if ($null -ne $p.ExitCode) {{ exit $p.ExitCode }}",
        powershell_single_quoted(command_line)
    );

    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script,
    ]);
    command
}

fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn is_elevation_required_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(740) || error.to_string().contains("os error 740")
}

fn launch_mode_for_command(command_line: &str) -> CommandLaunchMode {
    if requires_shell(command_line) || split_program_and_args(command_line).is_none() {
        CommandLaunchMode::Shell
    } else {
        CommandLaunchMode::Direct
    }
}

fn requires_shell(command_line: &str) -> bool {
    command_line.contains('%')
        || command_line.contains('&')
        || command_line.contains('|')
        || command_line.contains('<')
        || command_line.contains('>')
}

fn looks_like_msiexec_install(command: &str) -> bool {
    let tokens = split_command_line(command);
    let Some(program) = tokens.first() else {
        return false;
    };

    is_msiexec_program(program)
        && tokens.iter().skip(1).any(|arg| is_msi_install_arg(arg))
        && !tokens.iter().skip(1).any(|arg| is_msi_uninstall_arg(arg))
}

fn replace_first_msi_install_flag(command: &str) -> String {
    let mut tokens = split_command_line(command);

    for arg in tokens.iter_mut().skip(1) {
        if is_msi_install_arg(arg) {
            let prefix = &arg[0..1];
            let suffix = &arg[2..];
            *arg = format!("{prefix}X{suffix}");
            break;
        }
    }

    join_command_line(&tokens)
}

fn split_program_and_args(command: &str) -> Option<(String, Vec<String>)> {
    let tokens = split_command_line(command);
    let (program, args) = tokens.split_first()?;
    (!program.trim().is_empty()).then(|| (program.clone(), args.to_vec()))
}

fn split_command_line(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in command.trim().chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            ch if ch.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn join_command_line(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| {
            if token.chars().any(char::is_whitespace) || token.contains('"') {
                format!("\"{}\"", token.replace('"', "\\\""))
            } else {
                token.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_msiexec_program(program: &str) -> bool {
    let lower = program.replace('/', "\\").to_ascii_lowercase();
    let file_name = lower.rsplit('\\').next().unwrap_or(&lower);
    matches!(file_name, "msiexec" | "msiexec.exe")
}

fn is_known_windows_launcher(program: &str) -> bool {
    let lower = program.replace('/', "\\").to_ascii_lowercase();
    let file_name = lower.rsplit('\\').next().unwrap_or(&lower);
    matches!(
        file_name,
        "msiexec"
            | "msiexec.exe"
            | "rundll32"
            | "rundll32.exe"
            | "control"
            | "control.exe"
            | "powershell"
            | "powershell.exe"
            | "pwsh"
            | "pwsh.exe"
            | "wscript"
            | "wscript.exe"
            | "cscript"
            | "cscript.exe"
    )
}

fn resolve_program(program: &str) -> Option<PathBuf> {
    let program_path = Path::new(program);
    if program_path.is_absolute() || program.contains('\\') || program.contains('/') {
        return program_path.exists().then(|| program_path.to_path_buf());
    }

    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(program);
        if candidate.exists() {
            return Some(candidate);
        }

        if Path::new(program).extension().is_none() {
            let exe_candidate = dir.join(format!("{program}.exe"));
            if exe_candidate.exists() {
                return Some(exe_candidate);
            }
        }
    }

    None
}

fn is_msi_install_arg(arg: &str) -> bool {
    let lower = arg.to_ascii_lowercase();
    lower == "/i" || lower == "-i" || lower.starts_with("/i{") || lower.starts_with("-i{")
}

fn is_msi_uninstall_arg(arg: &str) -> bool {
    let lower = arg.to_ascii_lowercase();
    lower == "/x" || lower == "-x" || lower.starts_with("/x{") || lower.starts_with("-x{")
}

#[cfg(test)]
mod tests {
    use super::{
        CommandLaunchMode, UninstallReadiness, assess_uninstall_command, launch_mode_for_command,
        make_uninstall_command,
    };

    #[test]
    fn converts_msiexec_install_to_uninstall() {
        let command = make_uninstall_command("MsiExec.exe /I{ABC-123}");
        assert_eq!(command, "MsiExec.exe /X{ABC-123}");
    }

    #[test]
    fn keeps_existing_uninstall_command() {
        let command = make_uninstall_command(r#""C:\App\uninstall.exe" /remove"#);
        assert_eq!(command, r#""C:\App\uninstall.exe" /remove"#);
    }

    #[test]
    fn converts_quoted_msiexec_install_argument() {
        let command = make_uninstall_command(r#""C:\Windows\System32\msiexec.exe" /I {ABC-123}"#);
        assert_eq!(command, r#"C:\Windows\System32\msiexec.exe /X {ABC-123}"#);
    }

    #[test]
    fn keeps_non_msiexec_install_flags() {
        let command = make_uninstall_command(r#""C:\App\setup.exe" /install"#);
        assert_eq!(command, r#""C:\App\setup.exe" /install"#);
    }

    #[test]
    fn uses_direct_launch_for_plain_executables() {
        assert_eq!(
            launch_mode_for_command(r#""C:\App\uninstall.exe" /remove"#),
            CommandLaunchMode::Direct
        );
    }

    #[test]
    fn falls_back_to_shell_for_environment_expansion() {
        assert_eq!(
            launch_mode_for_command(r#"%ProgramFiles%\App\uninstall.exe /remove"#),
            CommandLaunchMode::Shell
        );
    }

    #[test]
    fn marks_missing_direct_program_as_unsupported() {
        let assessment =
            assess_uninstall_command(r#""C:\DefinitelyMissingApp\uninstall.exe" /remove"#);
        assert_eq!(assessment.status, UninstallReadiness::Unsupported);
    }

    #[test]
    fn marks_shell_commands_as_limited_verification() {
        let assessment = assess_uninstall_command(r#"%ProgramFiles%\App\uninstall.exe /remove"#);
        assert_eq!(assessment.status, UninstallReadiness::NeedsShell);
    }
}
