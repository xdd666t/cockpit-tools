use std::collections::HashSet;
use std::path::Path;

use crate::models::{DefaultInstanceSettings, InstanceProfileView};
use crate::modules;

const DEFAULT_INSTANCE_ID: &str = "__default__";

fn resolve_default_account_id(settings: &DefaultInstanceSettings) -> Option<String> {
    if settings.follow_local_account {
        resolve_local_account_id()
    } else {
        settings.bind_account_id.clone()
    }
}

fn resolve_local_account_id() -> Option<String> {
    let account = modules::codex_account::get_current_account()?;
    Some(account.id)
}

fn normalize_path(value: &str) -> String {
    let trimmed = value.trim();
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

#[tauri::command]
pub async fn codex_get_instance_defaults() -> Result<modules::instance::InstanceDefaults, String> {
    modules::codex_instance::get_instance_defaults()
}

#[tauri::command]
pub async fn codex_list_instances() -> Result<Vec<InstanceProfileView>, String> {
    let store = modules::codex_instance::load_instance_store()?;
    let default_dir = modules::codex_instance::get_default_codex_home()?;
    let default_dir_str = default_dir.to_string_lossy().to_string();
    let running_dirs = modules::process::list_codex_home_dirs(&default_dir_str);
    crate::modules::logger::log_info(&format!(
        "[Codex Instances] running CODEX_HOME: {:?}",
        running_dirs
    ));
    let running_set: HashSet<String> = running_dirs.into_iter().collect();

    let default_settings = store.default_settings.clone();
    let mut result: Vec<InstanceProfileView> = store
        .instances
        .into_iter()
        .map(|instance| {
            let normalized = normalize_path(&instance.user_data_dir);
            let running = !normalized.is_empty() && running_set.contains(&normalized);
            InstanceProfileView::from_profile(instance, running)
        })
        .collect();

    let normalized_default = normalize_path(&default_dir_str);
    let default_running = !normalized_default.is_empty() && running_set.contains(&normalized_default);
    let default_bind_account_id = resolve_default_account_id(&default_settings);
    result.push(InstanceProfileView {
        id: DEFAULT_INSTANCE_ID.to_string(),
        name: String::new(),
        user_data_dir: default_dir_str,
        extra_args: default_settings.extra_args.clone(),
        bind_account_id: default_bind_account_id,
        created_at: 0,
        last_launched_at: None,
        running: default_running,
        is_default: true,
        follow_local_account: default_settings.follow_local_account,
    });

    Ok(result)
}

#[tauri::command]
pub async fn codex_create_instance(
    name: String,
    user_data_dir: String,
    extra_args: Option<String>,
    bind_account_id: Option<String>,
    copy_source_instance_id: Option<String>,
) -> Result<InstanceProfileView, String> {
    let instance = modules::codex_instance::create_instance(modules::codex_instance::CreateInstanceParams {
        name,
        user_data_dir,
        extra_args: extra_args.unwrap_or_default(),
        bind_account_id,
        copy_source_instance_id,
    })?;

    Ok(InstanceProfileView::from_profile(instance, false))
}

#[tauri::command]
pub async fn codex_update_instance(
    instance_id: String,
    name: Option<String>,
    extra_args: Option<String>,
    bind_account_id: Option<Option<String>>,
    follow_local_account: Option<bool>,
) -> Result<InstanceProfileView, String> {
    if instance_id == DEFAULT_INSTANCE_ID {
        let default_dir = modules::codex_instance::get_default_codex_home()?;
        let default_dir_str = default_dir.to_string_lossy().to_string();
        let updated = modules::codex_instance::update_default_settings(
            bind_account_id,
            extra_args,
            follow_local_account,
        )?;
        let normalized = normalize_path(&default_dir_str);
        let running_dirs = modules::process::list_codex_home_dirs(&default_dir_str);
        let running = running_dirs.contains(&normalized);
        let default_bind_account_id = resolve_default_account_id(&updated);
        return Ok(InstanceProfileView {
            id: DEFAULT_INSTANCE_ID.to_string(),
            name: String::new(),
            user_data_dir: default_dir_str,
            extra_args: updated.extra_args,
            bind_account_id: default_bind_account_id,
            created_at: 0,
            last_launched_at: None,
            running,
            is_default: true,
            follow_local_account: updated.follow_local_account,
        });
    }

    let instance = modules::codex_instance::update_instance(modules::codex_instance::UpdateInstanceParams {
        instance_id,
        name,
        extra_args,
        bind_account_id,
    })?;

    let default_dir = modules::codex_instance::get_default_codex_home()?;
    let default_dir_str = default_dir.to_string_lossy().to_string();
    let normalized = normalize_path(&instance.user_data_dir);
    let running_dirs = modules::process::list_codex_home_dirs(&default_dir_str);
    let running = running_dirs.contains(&normalized);
    Ok(InstanceProfileView::from_profile(instance, running))
}

#[tauri::command]
pub async fn codex_delete_instance(instance_id: String) -> Result<(), String> {
    if instance_id == DEFAULT_INSTANCE_ID {
        return Err("默认实例不可删除".to_string());
    }
    modules::codex_instance::delete_instance(&instance_id)
}

#[tauri::command]
pub async fn codex_start_instance(instance_id: String) -> Result<InstanceProfileView, String> {
    if instance_id == DEFAULT_INSTANCE_ID {
        let default_dir = modules::codex_instance::get_default_codex_home()?;
        let default_dir_str = default_dir.to_string_lossy().to_string();
        let default_settings = modules::codex_instance::load_default_settings()?;
        let default_bind_account_id = resolve_default_account_id(&default_settings);
        if let Some(ref account_id) = default_bind_account_id {
            modules::codex_instance::inject_account_to_profile(&default_dir, account_id).await?;
        }
        let extra_args = modules::process::parse_extra_args(&default_settings.extra_args);
        modules::process::start_codex_with_args(&default_dir_str, &extra_args)?;
        let normalized = normalize_path(&default_dir_str);
        let running_dirs = modules::process::list_codex_home_dirs(&default_dir_str);
        let running = running_dirs.contains(&normalized);
        return Ok(InstanceProfileView {
            id: DEFAULT_INSTANCE_ID.to_string(),
            name: String::new(),
            user_data_dir: default_dir_str,
            extra_args: default_settings.extra_args,
            bind_account_id: default_bind_account_id,
            created_at: 0,
            last_launched_at: None,
            running,
            is_default: true,
            follow_local_account: default_settings.follow_local_account,
        });
    }

    let store = modules::codex_instance::load_instance_store()?;
    let instance = store
        .instances
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or("实例不存在")?;

    if let Some(ref account_id) = instance.bind_account_id {
        modules::codex_instance::inject_account_to_profile(Path::new(&instance.user_data_dir), account_id).await?;
    }

    let extra_args = modules::process::parse_extra_args(&instance.extra_args);
    modules::process::start_codex_with_args(&instance.user_data_dir, &extra_args)?;
    let updated = modules::codex_instance::update_instance_last_launched(&instance.id)?;

    let default_dir = modules::codex_instance::get_default_codex_home()?;
    let default_dir_str = default_dir.to_string_lossy().to_string();
    let normalized = normalize_path(&updated.user_data_dir);
    let running_dirs = modules::process::list_codex_home_dirs(&default_dir_str);
    let running = running_dirs.contains(&normalized);
    Ok(InstanceProfileView::from_profile(updated, running))
}

#[tauri::command]
pub async fn codex_stop_instance(instance_id: String) -> Result<InstanceProfileView, String> {
    if instance_id == DEFAULT_INSTANCE_ID {
        let default_dir = modules::codex_instance::get_default_codex_home()?;
        let default_dir_str = default_dir.to_string_lossy().to_string();
        modules::process::close_codex_instance(&default_dir_str, 20)?;
        let normalized = normalize_path(&default_dir_str);
        let running_dirs = modules::process::list_codex_home_dirs(&default_dir_str);
        let running = running_dirs.contains(&normalized);
        let default_settings = modules::codex_instance::load_default_settings()?;
        let default_bind_account_id = resolve_default_account_id(&default_settings);
        return Ok(InstanceProfileView {
            id: DEFAULT_INSTANCE_ID.to_string(),
            name: String::new(),
            user_data_dir: default_dir_str,
            extra_args: default_settings.extra_args,
            bind_account_id: default_bind_account_id,
            created_at: 0,
            last_launched_at: None,
            running,
            is_default: true,
            follow_local_account: default_settings.follow_local_account,
        });
    }

    let store = modules::codex_instance::load_instance_store()?;
    let instance = store
        .instances
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or("实例不存在")?;

    modules::process::close_codex_instance(&instance.user_data_dir, 20)?;
    let default_dir = modules::codex_instance::get_default_codex_home()?;
    let default_dir_str = default_dir.to_string_lossy().to_string();
    let normalized = normalize_path(&instance.user_data_dir);
    let running_dirs = modules::process::list_codex_home_dirs(&default_dir_str);
    let running = running_dirs.contains(&normalized);
    Ok(InstanceProfileView::from_profile(instance, running))
}

#[tauri::command]
pub async fn codex_force_stop_instance(instance_id: String) -> Result<InstanceProfileView, String> {
    if instance_id == DEFAULT_INSTANCE_ID {
        let default_dir = modules::codex_instance::get_default_codex_home()?;
        let default_dir_str = default_dir.to_string_lossy().to_string();
        modules::process::force_kill_codex_instance(&default_dir_str)?;
        let normalized = normalize_path(&default_dir_str);
        let running_dirs = modules::process::list_codex_home_dirs(&default_dir_str);
        let running = running_dirs.contains(&normalized);
        let default_settings = modules::codex_instance::load_default_settings()?;
        let default_bind_account_id = resolve_default_account_id(&default_settings);
        return Ok(InstanceProfileView {
            id: DEFAULT_INSTANCE_ID.to_string(),
            name: String::new(),
            user_data_dir: default_dir_str,
            extra_args: default_settings.extra_args,
            bind_account_id: default_bind_account_id,
            created_at: 0,
            last_launched_at: None,
            running,
            is_default: true,
            follow_local_account: default_settings.follow_local_account,
        });
    }

    let store = modules::codex_instance::load_instance_store()?;
    let instance = store
        .instances
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or("实例不存在")?;

    modules::process::force_kill_codex_instance(&instance.user_data_dir)?;
    let default_dir = modules::codex_instance::get_default_codex_home()?;
    let default_dir_str = default_dir.to_string_lossy().to_string();
    let normalized = normalize_path(&instance.user_data_dir);
    let running_dirs = modules::process::list_codex_home_dirs(&default_dir_str);
    let running = running_dirs.contains(&normalized);
    Ok(InstanceProfileView::from_profile(instance, running))
}

#[tauri::command]
pub async fn codex_close_all_instances() -> Result<(), String> {
    modules::process::close_codex(20)?;
    Ok(())
}

#[tauri::command]
pub async fn codex_open_instance_window(instance_id: String) -> Result<(), String> {
    if instance_id == DEFAULT_INSTANCE_ID {
        let default_dir = modules::codex_instance::get_default_codex_home()?;
        let default_dir_str = default_dir.to_string_lossy().to_string();
        let default_settings = modules::codex_instance::load_default_settings()?;
        let extra_args = modules::process::parse_extra_args(&default_settings.extra_args);
        modules::process::start_codex_with_args(&default_dir_str, &extra_args)?;
        return Ok(());
    }

    let store = modules::codex_instance::load_instance_store()?;
    let instance = store
        .instances
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or("实例不存在")?;

    let extra_args = modules::process::parse_extra_args(&instance.extra_args);
    modules::process::start_codex_with_args(&instance.user_data_dir, &extra_args)?;
    Ok(())
}
