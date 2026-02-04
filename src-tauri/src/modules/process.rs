use std::collections::HashSet;
use std::process::Command;
use std::thread;
use std::time::Duration;
use sysinfo::System;
#[cfg(target_os = "windows")]
use crate::modules::config;

const OPENCODE_APP_NAME: &str = "OpenCode";
#[cfg(target_os = "macos")]
const CODEX_APP_PATH: &str = "/Applications/Codex.app/Contents/MacOS/Codex";

fn normalize_custom_path(value: Option<&str>) -> Option<String> {
    let trimmed = value.unwrap_or("").trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// 检查 Antigravity 是否在运行
pub fn is_antigravity_running() -> bool {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let current_pid = std::process::id();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        // 通用的辅助进程排除逻辑
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = args_str.contains("--type=")
            || name.contains("helper")
            || name.contains("plugin")
            || name.contains("renderer")
            || name.contains("gpu")
            || name.contains("crashpad")
            || name.contains("utility")
            || name.contains("audio")
            || name.contains("sandbox")
            || exe_path.contains("crashpad");

        #[cfg(target_os = "macos")]
        {
            if exe_path.contains("antigravity.app") && !is_helper {
                return true;
            }
        }

        #[cfg(target_os = "windows")]
        {
            if name == "antigravity.exe" && !is_helper {
                return true;
            }
        }

        #[cfg(target_os = "linux")]
        {
            if (name.contains("antigravity") || exe_path.contains("/antigravity"))
                && !name.contains("tools")
                && !is_helper
            {
                return true;
            }
        }
    }

    false
}

fn extract_user_data_dir(args: &[std::ffi::OsString]) -> Option<String> {
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        let value = arg.to_string_lossy().to_string();
        if value.starts_with("--user-data-dir=") {
            return Some(value.trim_start_matches("--user-data-dir=").to_string());
        }
        if value == "--user-data-dir" {
            if let Some(next) = iter.next() {
                return Some(next.to_string_lossy().to_string());
            }
        }
    }
    None
}

fn parse_user_data_dir_value(raw: &str) -> Option<String> {
    let rest = raw.trim_start();
    if rest.is_empty() {
        return None;
    }
    let value = if rest.starts_with('"') {
        let end = rest[1..].find('"').map(|idx| idx + 1).unwrap_or(rest.len());
        &rest[1..end]
    } else if rest.starts_with('\'') {
        let end = rest[1..].find('\'').map(|idx| idx + 1).unwrap_or(rest.len());
        &rest[1..end]
    } else {
        let end = rest.find(" --").unwrap_or(rest.len());
        &rest[..end]
    };
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn extract_user_data_dir_from_command_line(command_line: &str) -> Option<String> {
    if let Some(pos) = command_line.find("--user-data-dir=") {
        let rest = &command_line[pos + "--user-data-dir=".len()..];
        return parse_user_data_dir_value(rest);
    }
    if let Some(pos) = command_line.find("--user-data-dir") {
        let rest = &command_line[pos + "--user-data-dir".len()..];
        return parse_user_data_dir_value(rest);
    }
    None
}

fn parse_env_value(raw: &str) -> Option<String> {
    let rest = raw.trim_start();
    if rest.is_empty() {
        return None;
    }
    let value = if rest.starts_with('"') {
        let end = rest[1..].find('"').map(|idx| idx + 1).unwrap_or(rest.len());
        &rest[1..end]
    } else if rest.starts_with('\'') {
        let end = rest[1..].find('\'').map(|idx| idx + 1).unwrap_or(rest.len());
        &rest[1..end]
    } else {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        &rest[..end]
    };
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn split_command_tokens(command_line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in command_line.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch.is_whitespace() {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                } else {
                    current.push(ch);
                }
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn is_env_token(token: &str) -> bool {
    let (key, _) = match token.split_once('=') {
        Some(parts) => parts,
        None => return false,
    };
    if key.is_empty() {
        return false;
    }
    let mut chars = key.chars();
    let first = match chars.next() {
        Some(value) => value,
        None => return false,
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn extract_env_value(command_line: &str, key: &str) -> Option<String> {
    let needle = format!("{}=", key);
    let pos = command_line.find(&needle)?;
    let rest = &command_line[pos + needle.len()..];
    parse_env_value(rest)
}

fn normalize_path_for_compare(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let resolved = std::fs::canonicalize(trimmed)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| trimmed.to_string());

    #[cfg(target_os = "windows")]
    {
        return resolved.to_lowercase();
    }
    #[cfg(not(target_os = "windows"))]
    {
        return resolved;
    }
}

#[cfg(target_os = "macos")]
fn list_user_data_dirs_from_ps() -> Vec<String> {
    let mut result = Vec::new();
    let output = Command::new("ps").args(["-axo", "pid,command"]).output();
    let output = match output {
        Ok(value) => value,
        Err(_) => return result,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_lowercase();
        if !lower.contains("antigravity.app/contents/") {
            continue;
        }
        if lower.contains("antigravity tools.app/contents/") {
            continue;
        }
        if lower.contains("crashpad_handler") {
            continue;
        }
        if let Some(dir) = extract_user_data_dir_from_command_line(line) {
            let normalized = normalize_path_for_compare(&dir);
            if !normalized.is_empty() {
                result.push(normalized);
            }
        }
    }
    result
}

#[cfg(target_os = "windows")]
fn list_user_data_dirs_from_powershell() -> Vec<String> {
    let mut result = Vec::new();
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_Process -Filter \"Name='Antigravity.exe'\" | Select-Object -Expand CommandLine",
        ])
        .output();
    let output = match output {
        Ok(value) => value,
        Err(_) => return result,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(dir) = extract_user_data_dir_from_command_line(line) {
            let normalized = normalize_path_for_compare(&dir);
            if !normalized.is_empty() {
                result.push(normalized);
            }
        }
    }
    result
}

#[cfg(target_os = "linux")]
fn list_user_data_dirs_from_proc() -> Vec<String> {
    let mut result = Vec::new();
    let entries = match std::fs::read_dir("/proc") {
        Ok(value) => value,
        Err(_) => return result,
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let pid = file_name.to_string_lossy();
        if !pid.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let cmdline_path = format!("/proc/{}/cmdline", pid);
        let cmdline = match std::fs::read(&cmdline_path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if cmdline.is_empty() {
            continue;
        }
        let cmdline_str = String::from_utf8_lossy(&cmdline).replace('\0', " ");
        let cmd_lower = cmdline_str.to_lowercase();
        let exe_path = std::fs::read_link(format!("/proc/{}/exe", pid))
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_lowercase()))
            .unwrap_or_default();
        if !cmd_lower.contains("antigravity") && !exe_path.contains("antigravity") {
            continue;
        }
        if cmd_lower.contains("tools") || exe_path.contains("tools") {
            continue;
        }
        if let Some(dir) = extract_user_data_dir_from_command_line(&cmdline_str) {
            let normalized = normalize_path_for_compare(&dir);
            if !normalized.is_empty() {
                result.push(normalized);
            }
        }
    }
    result
}

fn collect_antigravity_pids_by_user_data_dir(user_data_dir: &str) -> Vec<u32> {
    let target = normalize_path_for_compare(user_data_dir);
    if target.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let current_pid = std::process::id();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        #[cfg(target_os = "macos")]
        let is_antigravity =
            exe_path.contains("antigravity.app") && !exe_path.contains("antigravity tools.app");
        #[cfg(target_os = "windows")]
        let is_antigravity = name == "antigravity.exe" || exe_path.ends_with("\\antigravity.exe");
        #[cfg(target_os = "linux")]
        let is_antigravity = (name.contains("antigravity") || exe_path.contains("/antigravity"))
            && !name.contains("tools")
            && !exe_path.contains("tools");

        if !is_antigravity {
            continue;
        }

        let args = process.cmd();
        if let Some(dir) = extract_user_data_dir(&args) {
            let normalized = normalize_path_for_compare(&dir);
            if normalized == target {
                result.push(pid_u32);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("ps").args(["-axo", "pid,command"]).output();
        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let mut parts = line.splitn(2, |ch: char| ch.is_whitespace());
                let pid_str = parts.next().unwrap_or("").trim();
                let cmdline = parts.next().unwrap_or("").trim();
                let pid = match pid_str.parse::<u32>() {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let lower = cmdline.to_lowercase();
                if !lower.contains("antigravity.app/contents/")
                    || lower.contains("antigravity tools.app/contents/")
                    || lower.contains("crashpad_handler")
                {
                    continue;
                }
                if let Some(dir) = extract_user_data_dir_from_command_line(cmdline) {
                    let normalized = normalize_path_for_compare(&dir);
                    if normalized == target {
                        result.push(pid);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_Process -Filter \"Name='Antigravity.exe'\" | ForEach-Object { \"$($_.ProcessId)|$($_.CommandLine)\" }",
            ])
            .output();
        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let mut parts = line.splitn(2, '|');
                let pid_str = parts.next().unwrap_or("").trim();
                let cmdline = parts.next().unwrap_or("").trim();
                let pid = match pid_str.parse::<u32>() {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if let Some(dir) = extract_user_data_dir_from_command_line(cmdline) {
                    let normalized = normalize_path_for_compare(&dir);
                    if normalized == target {
                        result.push(pid);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let entries = match std::fs::read_dir("/proc") {
            Ok(value) => value,
            Err(_) => return result,
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let pid_str = file_name.to_string_lossy();
            if !pid_str.chars().all(|ch| ch.is_ascii_digit()) {
                continue;
            }
            let pid = match pid_str.parse::<u32>() {
                Ok(value) => value,
                Err(_) => continue,
            };
            let cmdline_path = format!("/proc/{}/cmdline", pid);
            let cmdline = match std::fs::read(&cmdline_path) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if cmdline.is_empty() {
                continue;
            }
            let cmdline_str = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            let cmd_lower = cmdline_str.to_lowercase();
            let exe_path = std::fs::read_link(format!("/proc/{}/exe", pid))
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_lowercase()))
                .unwrap_or_default();
            if !cmd_lower.contains("antigravity") && !exe_path.contains("antigravity") {
                continue;
            }
            if cmd_lower.contains("tools") || exe_path.contains("tools") {
                continue;
            }
            if let Some(dir) = extract_user_data_dir_from_command_line(&cmdline_str) {
                let normalized = normalize_path_for_compare(&dir);
                if normalized == target {
                    result.push(pid);
                }
            }
        }
    }

    result.sort();
    result.dedup();
    result
}

pub fn parse_extra_args(raw: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for ch in raw.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// 获取正在运行的 Antigravity 实例的 user-data-dir
pub fn list_antigravity_user_data_dirs() -> Vec<String> {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let current_pid = std::process::id();
    let mut result = Vec::new();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        let args = process.cmd();

        #[cfg(target_os = "macos")]
        let is_antigravity =
            exe_path.contains("antigravity.app") && !exe_path.contains("antigravity tools.app");
        #[cfg(target_os = "windows")]
        let is_antigravity = name == "antigravity.exe" || exe_path.ends_with("\\antigravity.exe");
        #[cfg(target_os = "linux")]
        let is_antigravity = (name.contains("antigravity") || exe_path.contains("/antigravity"))
            && !name.contains("tools")
            && !exe_path.contains("tools");

        if !is_antigravity {
            continue;
        }

        if let Some(dir) = extract_user_data_dir(&args) {
            let normalized = normalize_path_for_compare(&dir);
            if !normalized.is_empty() {
                result.push(normalized);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let mut ps_dirs = list_user_data_dirs_from_ps();
        if !ps_dirs.is_empty() {
            result.append(&mut ps_dirs);
            result.sort();
            result.dedup();
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut ps_dirs = list_user_data_dirs_from_powershell();
        if !ps_dirs.is_empty() {
            result.append(&mut ps_dirs);
            result.sort();
            result.dedup();
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut proc_dirs = list_user_data_dirs_from_proc();
        if !proc_dirs.is_empty() {
            result.append(&mut proc_dirs);
            result.sort();
            result.dedup();
        }
    }

    result
}

/// 获取所有 Antigravity 进程的 PID（包括主进程和Helper进程）
fn get_antigravity_pids() -> Vec<u32> {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let mut pids = Vec::new();
    let current_pid = std::process::id();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();

        // 排除自身 PID
        if pid_u32 == current_pid {
            continue;
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        // 通用的辅助进程排除逻辑
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = args_str.contains("--type=")
            || name.contains("helper")
            || name.contains("plugin")
            || name.contains("renderer")
            || name.contains("gpu")
            || name.contains("crashpad")
            || name.contains("utility")
            || name.contains("audio")
            || name.contains("sandbox")
            || exe_path.contains("crashpad");

        #[cfg(target_os = "macos")]
        {
            // 匹配 Antigravity 主程序包内的进程，但排除 Helper/Plugin/Renderer 等辅助进程
            if exe_path.contains("antigravity.app") && !is_helper {
                pids.push(pid_u32);
            }
        }

        #[cfg(target_os = "windows")]
        {
            if name == "antigravity.exe" && !is_helper {
                pids.push(pid_u32);
            }
        }

        #[cfg(target_os = "linux")]
        {
            if (name == "antigravity" || exe_path.contains("/antigravity"))
                && !name.contains("tools")
                && !is_helper
            {
                pids.push(pid_u32);
            }
        }
    }

    if !pids.is_empty() {
        crate::modules::logger::log_info(&format!(
            "找到 {} 个 Antigravity 进程: {:?}",
            pids.len(),
            pids
        ));
    }

    pids
}

/// 关闭 Antigravity 进程
pub fn close_antigravity(timeout_secs: u64) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let _ = timeout_secs; // Silence unused warning on Windows
    crate::modules::logger::log_info("正在关闭 Antigravity...");

    let pids = get_antigravity_pids();
    if pids.is_empty() {
        crate::modules::logger::log_info("Antigravity 未在运行，无需关闭");
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        crate::modules::logger::log_info(&format!(
            "正在 Windows 上关闭 {} 个 Antigravity 进程...",
            pids.len()
        ));
        for pid in &pids {
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output();
        }
        thread::sleep(Duration::from_millis(200));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        // 阶段 1: 优雅退出 (SIGTERM)
        crate::modules::logger::log_info(&format!(
            "向 {} 个 Antigravity 进程发送 SIGTERM...",
            pids.len()
        ));
        for pid in &pids {
            let _ = Command::new("kill")
                .args(["-15", &pid.to_string()])
                .output();
        }

        // 等待优雅退出（最多 timeout_secs 的 70%）
        let graceful_timeout = (timeout_secs * 7) / 10;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(graceful_timeout) {
            if !is_antigravity_running() {
                crate::modules::logger::log_info("所有 Antigravity 进程已优雅关闭");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(500));
        }

        // 阶段 2: 强制杀死 (SIGKILL)
        if is_antigravity_running() {
            let remaining_pids = get_antigravity_pids();
            if !remaining_pids.is_empty() {
                crate::modules::logger::log_warn(&format!(
                    "优雅关闭超时，强制杀死 {} 个残留进程 (SIGKILL)",
                    remaining_pids.len()
                ));
                for pid in &remaining_pids {
                    let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    // 最终检查
    if is_antigravity_running() {
        return Err("无法关闭 Antigravity 进程，请手动关闭后重试".to_string());
    }

    crate::modules::logger::log_info("Antigravity 已成功关闭");
    Ok(())
}

/// 关闭指定实例（按 user-data-dir 匹配）
pub fn close_antigravity_instance(user_data_dir: &str, timeout_secs: u64) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let _ = timeout_secs;
    let target = normalize_path_for_compare(user_data_dir);
    if target.is_empty() {
        return Err("实例目录为空，无法关闭".to_string());
    }

    let mut pids = collect_antigravity_pids_by_user_data_dir(user_data_dir);
    if pids.is_empty() {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        for pid in &pids {
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }
        thread::sleep(Duration::from_millis(200));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        for pid in &pids {
            let _ = Command::new("kill").args(["-15", &pid.to_string()]).output();
        }

        let graceful_timeout = (timeout_secs * 7) / 10;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(graceful_timeout) {
            if collect_antigravity_pids_by_user_data_dir(user_data_dir).is_empty() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(400));
        }

        pids = collect_antigravity_pids_by_user_data_dir(user_data_dir);
        if !pids.is_empty() {
            for pid in &pids {
                let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
            }
            thread::sleep(Duration::from_millis(800));
        }
    }

    if !collect_antigravity_pids_by_user_data_dir(user_data_dir).is_empty() {
        return Err("无法关闭实例进程，请手动关闭后重试".to_string());
    }

    Ok(())
}

/// 强制关闭指定实例（按 user-data-dir 匹配，直接 SIGKILL / taskkill /F）
pub fn force_kill_antigravity_instance(user_data_dir: &str) -> Result<(), String> {
    let target = normalize_path_for_compare(user_data_dir);
    if target.is_empty() {
        return Err("实例目录为空，无法关闭".to_string());
    }

    let pids = collect_antigravity_pids_by_user_data_dir(user_data_dir);
    if pids.is_empty() {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        for pid in &pids {
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }
        thread::sleep(Duration::from_millis(200));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        for pid in &pids {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
        thread::sleep(Duration::from_millis(300));
    }

    if !collect_antigravity_pids_by_user_data_dir(user_data_dir).is_empty() {
        return Err("无法强制关闭实例进程，请手动关闭后重试".to_string());
    }

    Ok(())
}

/// 启动 Antigravity
pub fn start_antigravity() -> Result<(), String> {
    start_antigravity_with_args("", &[])
}

/// 启动 Antigravity（支持 user-data-dir 与附加参数）
pub fn start_antigravity_with_args(user_data_dir: &str, extra_args: &[String]) -> Result<(), String> {
    crate::modules::logger::log_info("正在启动 Antigravity...");

    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        let force_new = !user_data_dir.trim().is_empty()
            || extra_args.iter().any(|arg| !arg.trim().is_empty());
        if force_new {
            cmd.args(["-n", "-a", "Antigravity", "--args"]);
            if !user_data_dir.trim().is_empty() {
                cmd.arg("--user-data-dir");
                cmd.arg(user_data_dir.trim());
            }
            for arg in extra_args {
                if !arg.trim().is_empty() {
                    cmd.arg(arg);
                }
            }
        } else {
            cmd.args(["-a", "Antigravity"]);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("启动 Antigravity 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("Unable to find application") {
                return Err("未找到 Antigravity 应用，请确保已安装 Antigravity".to_string());
            }
            return Err(format!("启动 Antigravity 失败: {}", stderr));
        }
        crate::modules::logger::log_info("Antigravity 启动命令已发送");
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        let mut candidates: Vec<String> = Vec::new();
        let custom_path = normalize_custom_path(Some(&config::get_user_config().antigravity_app_path));
        if let Some(custom) = custom_path {
            candidates.push(custom);
        }

        if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
            candidates.push(format!("{}/Programs/Antigravity/Antigravity.exe", local_appdata));
        }

        if let Ok(program_files) = std::env::var("PROGRAMFILES") {
            candidates.push(format!("{}/Antigravity/Antigravity.exe", program_files));
        }

        for candidate in candidates {
            if candidate.contains('/') || candidate.contains('\\') {
                if !std::path::Path::new(&candidate).exists() {
                    continue;
                }
            }
            let mut cmd = Command::new(&candidate);
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            if !user_data_dir.trim().is_empty() {
                cmd.arg("--user-data-dir");
                cmd.arg(user_data_dir.trim());
            }
            for arg in extra_args {
                if !arg.trim().is_empty() {
                    cmd.arg(arg);
                }
            }
            cmd.spawn()
                .map_err(|e| format!("启动 Antigravity 失败: {}", e))?;
            crate::modules::logger::log_info(&format!("Antigravity 已启动: {}", candidate));
            return Ok(());
        }
        return Err("未找到 Antigravity 可执行文件，请在设置中配置启动路径".to_string());
    }

    #[cfg(target_os = "linux")]
    {
        // 尝试常见安装路径
        let possible_paths = ["/usr/bin/antigravity", "/opt/antigravity/antigravity"];

        for path in possible_paths {
            if std::path::Path::new(path).exists() {
                let mut cmd = Command::new(path);
                if !user_data_dir.trim().is_empty() {
                    cmd.arg("--user-data-dir");
                    cmd.arg(user_data_dir.trim());
                }
                for arg in extra_args {
                    if !arg.trim().is_empty() {
                        cmd.arg(arg);
                    }
                }
                cmd.spawn()
                    .map_err(|e| format!("启动 Antigravity 失败: {}", e))?;
                crate::modules::logger::log_info(&format!("Antigravity 已启动: {}", path));
                return Ok(());
            }
        }

        // 尝试 PATH 中的 antigravity
        let mut cmd = Command::new("antigravity");
        if !user_data_dir.trim().is_empty() {
            cmd.arg("--user-data-dir");
            cmd.arg(user_data_dir.trim());
        }
        for arg in extra_args {
            if !arg.trim().is_empty() {
                cmd.arg(arg);
            }
        }
        if cmd.spawn().is_ok() {
            crate::modules::logger::log_info("Antigravity 已启动 (从 PATH)");
            return Ok(());
        }

        return Err("未找到 Antigravity 可执行文件".to_string());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    Err("不支持的操作系统".to_string())
}

#[cfg(target_os = "macos")]
fn collect_codex_process_entries() -> Vec<(u32, Option<String>)> {
    let mut result = Vec::new();
    let mut pids: Vec<u32> = Vec::new();
    if let Ok(output) = Command::new("pgrep")
        .args(["-f", "Codex.app/Contents/MacOS/Codex"])
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Ok(pid) = line.trim().parse::<u32>() {
                    pids.push(pid);
                }
            }
        }
    }

    if pids.is_empty() {
        let output = Command::new("ps")
            .args(["-Eww", "-o", "pid=,command="])
            .output();
        let output = match output {
            Ok(value) => value,
            Err(_) => return result,
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.splitn(2, |ch: char| ch.is_whitespace());
            let pid_str = parts.next().unwrap_or("").trim();
            let cmdline = parts.next().unwrap_or("").trim();
            let pid = match pid_str.parse::<u32>() {
                Ok(value) => value,
                Err(_) => continue,
            };
            if !cmdline.to_lowercase().contains("codex.app/contents/macos/codex") {
                continue;
            }
            pids.push(pid);
        }
    }

    pids.sort();
    pids.dedup();

    for pid in pids {
        let output = Command::new("ps")
            .args(["-Eww", "-p", &pid.to_string(), "-o", "command="])
            .output();
        let output = match output {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !output.status.success() {
            continue;
        }
        let cmdline = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if cmdline.is_empty() {
            continue;
        }
        let lower = cmdline.to_lowercase();
        if !lower.contains("codex.app/contents/macos/codex") {
            continue;
        }
        crate::modules::logger::log_info(&format!(
            "[Codex Instances] ps line pid={} cmdline={}",
            pid, cmdline
        ));
        let tokens = split_command_tokens(&cmdline);
        let mut args: Vec<String> = Vec::new();
        let mut env_tokens: Vec<String> = Vec::new();
        let mut saw_env = false;
        for (idx, token) in tokens.into_iter().enumerate() {
            if idx == 0 {
                args.push(token);
                continue;
            }
            if !saw_env && is_env_token(&token) {
                saw_env = true;
                env_tokens.push(token);
                continue;
            }
            if saw_env {
                env_tokens.push(token);
            } else {
                args.push(token);
            }
        }
        let args_lower = args.join(" ").to_lowercase();
        let is_helper = args_lower.contains("--type=")
            || args_lower.contains("helper")
            || args_lower.contains("renderer")
            || args_lower.contains("gpu")
            || args_lower.contains("crashpad")
            || args_lower.contains("utility")
            || args_lower.contains("audio")
            || args_lower.contains("sandbox");
        if is_helper {
            continue;
        }
        let mut codex_home = env_tokens
            .iter()
            .find_map(|token| token.strip_prefix("CODEX_HOME="))
            .map(|value| value.to_string());
        if codex_home.is_none() {
            codex_home = extract_env_value(&cmdline, "CODEX_HOME");
        }
        crate::modules::logger::log_info(&format!(
            "[Codex Instances] pid={} parsed CODEX_HOME={:?}",
            pid, codex_home
        ));
        result.push((pid, codex_home));
    }
    result
}

#[cfg(target_os = "macos")]
fn collect_codex_pids_by_home(target_home: &str, default_home: &str) -> Vec<u32> {
    let target = normalize_path_for_compare(target_home);
    if target.is_empty() {
        return Vec::new();
    }
    let default_normalized = normalize_path_for_compare(default_home);
    let mut result = Vec::new();
    for (pid, home) in collect_codex_process_entries() {
        let resolved = home
            .as_ref()
            .map(|value| normalize_path_for_compare(value))
            .unwrap_or_else(|| default_normalized.clone());
        if resolved == target {
            result.push(pid);
        }
    }
    result.sort();
    result.dedup();
    result
}

/// 获取正在运行的 Codex 实例的 CODEX_HOME
pub fn list_codex_home_dirs(default_home: &str) -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let mut result = Vec::new();
        let mut has_default = false;
        for (_, home) in collect_codex_process_entries() {
            if let Some(value) = home {
                let normalized = normalize_path_for_compare(&value);
                if !normalized.is_empty() {
                    result.push(normalized);
                }
            } else {
                has_default = true;
            }
        }
        if has_default {
            let normalized = normalize_path_for_compare(default_home);
            if !normalized.is_empty() {
                result.push(normalized);
            }
        }
        result.sort();
        result.dedup();
        return result;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = default_home;
        Vec::new()
    }
}

/// 启动 Codex（支持 CODEX_HOME 与附加参数，仅 macOS）
pub fn start_codex_with_args(codex_home: &str, extra_args: &[String]) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if !std::path::Path::new(CODEX_APP_PATH).exists() {
            return Err("未找到 Codex 应用，请确保已安装 Codex".to_string());
        }
        let mut cmd = Command::new(CODEX_APP_PATH);
        cmd.env("CODEX_HOME", codex_home.trim());
        for arg in extra_args {
            if !arg.trim().is_empty() {
                cmd.arg(arg);
            }
        }
        cmd.spawn()
            .map_err(|e| format!("启动 Codex 失败: {}", e))?;
        crate::modules::logger::log_info("Codex 启动命令已发送");
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (codex_home, extra_args);
        Err("Codex 多开实例仅支持 macOS".to_string())
    }
}

/// 关闭 Codex 进程（仅 macOS）
pub fn close_codex(timeout_secs: u64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::modules::logger::log_info("正在关闭 Codex...");
        let pids: Vec<u32> = collect_codex_process_entries().into_iter().map(|(pid, _)| pid).collect();
        if pids.is_empty() {
            return Ok(());
        }

        for pid in &pids {
            let _ = Command::new("kill").args(["-15", &pid.to_string()]).output();
        }

        let graceful_timeout = (timeout_secs * 7) / 10;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(graceful_timeout) {
            if collect_codex_process_entries().is_empty() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(500));
        }

        let remaining: Vec<u32> = collect_codex_process_entries().into_iter().map(|(pid, _)| pid).collect();
        for pid in &remaining {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
        thread::sleep(Duration::from_secs(1));

        if !collect_codex_process_entries().is_empty() {
            return Err("无法关闭 Codex 进程，请手动关闭后重试".to_string());
        }
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = timeout_secs;
        Err("Codex 多开实例仅支持 macOS".to_string())
    }
}

/// 关闭指定 Codex 实例（按 CODEX_HOME 匹配）
pub fn close_codex_instance(codex_home: &str, timeout_secs: u64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let default_home = crate::modules::codex_account::get_codex_home()
            .to_string_lossy()
            .to_string();
        let target = normalize_path_for_compare(codex_home);
        if target.is_empty() {
            return Err("实例目录为空，无法关闭".to_string());
        }

        let mut pids = collect_codex_pids_by_home(codex_home, &default_home);
        if pids.is_empty() {
            return Ok(());
        }

        for pid in &pids {
            let _ = Command::new("kill").args(["-15", &pid.to_string()]).output();
        }

        let graceful_timeout = (timeout_secs * 7) / 10;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(graceful_timeout) {
            if collect_codex_pids_by_home(codex_home, &default_home).is_empty() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(400));
        }

        pids = collect_codex_pids_by_home(codex_home, &default_home);
        if !pids.is_empty() {
            for pid in &pids {
                let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
            }
            thread::sleep(Duration::from_millis(800));
        }

        if !collect_codex_pids_by_home(codex_home, &default_home).is_empty() {
            return Err("无法关闭实例进程，请手动关闭后重试".to_string());
        }
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (codex_home, timeout_secs);
        Err("Codex 多开实例仅支持 macOS".to_string())
    }
}

/// 强制关闭指定 Codex 实例（按 CODEX_HOME 匹配）
pub fn force_kill_codex_instance(codex_home: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let default_home = crate::modules::codex_account::get_codex_home()
            .to_string_lossy()
            .to_string();
        let target = normalize_path_for_compare(codex_home);
        if target.is_empty() {
            return Err("实例目录为空，无法关闭".to_string());
        }

        let pids = collect_codex_pids_by_home(codex_home, &default_home);
        if pids.is_empty() {
            return Ok(());
        }

        for pid in &pids {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
        thread::sleep(Duration::from_millis(300));

        if !collect_codex_pids_by_home(codex_home, &default_home).is_empty() {
            return Err("无法强制关闭实例进程，请手动关闭后重试".to_string());
        }
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = codex_home;
        Err("Codex 多开实例仅支持 macOS".to_string())
    }
}

/// 检查 OpenCode（桌面端）是否在运行
pub fn is_opencode_running() -> bool {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let current_pid = std::process::id();
    let app_lower = OPENCODE_APP_NAME.to_lowercase();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = args_str.contains("--type=")
            || name.contains("helper")
            || name.contains("plugin")
            || name.contains("renderer")
            || name.contains("gpu")
            || name.contains("crashpad")
            || name.contains("utility")
            || name.contains("audio")
            || name.contains("sandbox")
            || exe_path.contains("crashpad");

        #[cfg(target_os = "macos")]
        {
            let bundle_lower = format!("{}.app", app_lower);
            if exe_path.contains(&bundle_lower) && !is_helper {
                return true;
            }
        }

        #[cfg(target_os = "windows")]
        {
            if (name == "opencode.exe"
                || name == "opencode"
                || name == app_lower
                || exe_path.contains("opencode"))
                && !is_helper
            {
                return true;
            }
        }

        #[cfg(target_os = "linux")]
        {
            if (name.contains("opencode") || exe_path.contains("/opencode")) && !is_helper {
                return true;
            }
        }
    }

    false
}

fn get_opencode_pids() -> Vec<u32> {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let mut pids = Vec::new();
    let current_pid = std::process::id();
    let app_lower = OPENCODE_APP_NAME.to_lowercase();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = args_str.contains("--type=")
            || name.contains("helper")
            || name.contains("plugin")
            || name.contains("renderer")
            || name.contains("gpu")
            || name.contains("crashpad")
            || name.contains("utility")
            || name.contains("audio")
            || name.contains("sandbox")
            || exe_path.contains("crashpad");

        #[cfg(target_os = "macos")]
        {
            let bundle_lower = format!("{}.app", app_lower);
            if exe_path.contains(&bundle_lower) && !is_helper {
                pids.push(pid_u32);
            }
        }

        #[cfg(target_os = "windows")]
        {
            if (name.contains("opencode") || exe_path.contains("opencode")) && !is_helper {
                pids.push(pid_u32);
            }
        }

        #[cfg(target_os = "linux")]
        {
            if (name.contains("opencode") || exe_path.contains("/opencode")) && !is_helper {
                pids.push(pid_u32);
            }
        }
    }

    if !pids.is_empty() {
        crate::modules::logger::log_info(&format!(
            "找到 {} 个 OpenCode 进程: {:?}",
            pids.len(),
            pids
        ));
    }

    pids
}

/// 关闭 OpenCode（桌面端）
pub fn close_opencode(timeout_secs: u64) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let _ = timeout_secs;

    crate::modules::logger::log_info("正在关闭 OpenCode...");
    let pids = get_opencode_pids();
    if pids.is_empty() {
        crate::modules::logger::log_info("OpenCode 未在运行，无需关闭");
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        for pid in &pids {
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(0x08000000)
                .output();
        }
        thread::sleep(Duration::from_millis(200));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        crate::modules::logger::log_info(&format!(
            "向 {} 个 OpenCode 进程发送 SIGTERM...",
            pids.len()
        ));
        for pid in &pids {
            let _ = Command::new("kill")
                .args(["-15", &pid.to_string()])
                .output();
        }

        let graceful_timeout = (timeout_secs * 7) / 10;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(graceful_timeout) {
            if !is_opencode_running() {
                crate::modules::logger::log_info("所有 OpenCode 进程已优雅关闭");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(500));
        }

        if is_opencode_running() {
            let remaining = get_opencode_pids();
            if !remaining.is_empty() {
                crate::modules::logger::log_warn(&format!(
                    "优雅关闭超时，强制杀死 {} 个残留进程 (SIGKILL)",
                    remaining.len()
                ));
                for pid in &remaining {
                    let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    if is_opencode_running() {
        return Err("无法关闭 OpenCode 进程，请手动关闭后重试".to_string());
    }

    crate::modules::logger::log_info("OpenCode 已成功关闭");
    Ok(())
}

/// 启动 OpenCode（桌面端）
pub fn start_opencode_with_path(custom_path: Option<&str>) -> Result<(), String> {
    crate::modules::logger::log_info("正在启动 OpenCode...");

    #[cfg(target_os = "macos")]
    {
        let target = normalize_custom_path(custom_path).unwrap_or_else(|| OPENCODE_APP_NAME.to_string());

        let output = Command::new("open")
            .args(["-a", &target])
            .output()
            .map_err(|e| format!("启动 OpenCode 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("Unable to find application") {
                return Err("未找到 OpenCode 应用，请在设置中配置启动路径".to_string());
            }
            return Err(format!("启动 OpenCode 失败: {}", stderr));
        }
        crate::modules::logger::log_info(&format!("OpenCode 启动命令已发送: {}", target));
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut candidates = Vec::new();
        if let Some(custom) = normalize_custom_path(custom_path) {
            candidates.push(custom);
        }

        if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
            candidates.push(format!("{}/Programs/OpenCode/OpenCode.exe", local_appdata));
        }

        if let Ok(program_files) = std::env::var("PROGRAMFILES") {
            candidates.push(format!("{}/OpenCode/OpenCode.exe", program_files));
        }

        for candidate in candidates {
            if candidate.contains('/') || candidate.contains('\\') {
                if !std::path::Path::new(&candidate).exists() {
                    continue;
                }
            }
            if Command::new(&candidate)
                .creation_flags(0x08000000)
                .spawn()
                .is_ok()
            {
                crate::modules::logger::log_info(&format!("OpenCode 已启动: {}", candidate));
                return Ok(());
            }
        }

        return Err("未找到 OpenCode 可执行文件，请在设置中配置启动路径".to_string());
    }

    #[cfg(target_os = "linux")]
    {
        let mut candidates = Vec::new();
        if let Some(custom) = normalize_custom_path(custom_path) {
            candidates.push(custom);
        }

        candidates.push("/usr/bin/opencode".to_string());
        candidates.push("/opt/opencode/opencode".to_string());
        candidates.push("opencode".to_string());

        for candidate in candidates {
            if candidate.contains('/') {
                if !std::path::Path::new(&candidate).exists() {
                    continue;
                }
            }
            if Command::new(&candidate).spawn().is_ok() {
                crate::modules::logger::log_info(&format!("OpenCode 已启动: {}", candidate));
                return Ok(());
            }
        }

        return Err("未找到 OpenCode 可执行文件，请在设置中配置启动路径".to_string());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    Err("不支持的操作系统".to_string())
}

pub fn find_pids_by_port(port: u16) -> Result<Vec<u32>, String> {
    let current_pid = std::process::id();
    let mut pids = HashSet::new();

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let output = Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{}", port), "-sTCP:LISTEN", "-t"])
            .output()
            .map_err(|e| format!("执行 lsof 失败: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                if pid != current_pid {
                    pids.insert(pid);
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let output = Command::new("netstat")
            .args(["-ano", "-p", "tcp"])
            .output()
            .map_err(|e| format!("执行 netstat 失败: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let port_suffix = format!(":{}", port);
        for line in stdout.lines() {
            let line = line.trim();
            if !line.starts_with("TCP") {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                continue;
            }
            let local = parts[1];
            let state = parts[3];
            let pid_str = parts[4];
            if !state.eq_ignore_ascii_case("LISTENING") {
                continue;
            }
            if !local.ends_with(&port_suffix) {
                continue;
            }
            if let Ok(pid) = pid_str.parse::<u32>() {
                if pid != current_pid {
                    pids.insert(pid);
                }
            }
        }
    }

    Ok(pids.into_iter().collect())
}

pub fn is_port_in_use(port: u16) -> Result<bool, String> {
    Ok(!find_pids_by_port(port)?.is_empty())
}

pub fn kill_port_processes(port: u16) -> Result<usize, String> {
    let pids = find_pids_by_port(port)?;
    if pids.is_empty() {
        return Ok(0);
    }

    let mut failed = Vec::new();

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        for pid in &pids {
            let output = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(0x08000000)
                .output();
            match output {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    failed.push(format!("pid {}: {}", pid, stderr.trim()));
                }
                Err(e) => failed.push(format!("pid {}: {}", pid, e)),
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        for pid in &pids {
            let output = Command::new("kill").args(["-9", &pid.to_string()]).output();
            match output {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    failed.push(format!("pid {}: {}", pid, stderr.trim()));
                }
                Err(e) => failed.push(format!("pid {}: {}", pid, e)),
            }
        }
    }

    if !failed.is_empty() {
        return Err(format!("关闭进程失败: {}", failed.join("; ")));
    }

    Ok(pids.len())
}
