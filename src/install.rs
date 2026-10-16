use std::{
    path::{Path, PathBuf},
    process::Command,
};
pub fn directory() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("Programs").join("WaifuGallery"))
        .ok_or("Не найдена папка LOCALAPPDATA".into())
}
pub fn data_directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("WaifuGallery"))
}
pub fn hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
}
pub fn powershell(script: &Path) -> Command {
    let mut c = Command::new("powershell.exe");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
    ])
    .arg(script);
    hidden(&mut c);
    c
}
pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let script = std::env::temp_dir().join(format!("waifu-install-{}.ps1", std::process::id()));
    std::fs::write(&script, include_str!("../windows/install.ps1")).map_err(|e| e.to_string())?;
    let result = powershell(&script)
        .arg("-SourceExe")
        .arg(&exe)
        .output()
        .map_err(|e| e.to_string());
    let _ = std::fs::remove_file(script);
    let result = result?;
    if !result.status.success() {
        return Err(format!(
            "Не удалось установить приложение: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let mut launch = Command::new(directory()?.join("WaifuGallery.exe"));
    hidden(&mut launch);
    launch.spawn().map_err(|e| e.to_string())?;
    Ok(())
}
pub fn uninstall() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let program = directory()?;
    if exe.parent() != Some(program.as_path()) {
        return Err("Удаление доступно для установленной версии".into());
    }
    let script = std::env::temp_dir().join(format!("waifu-uninstall-{}.ps1", std::process::id()));
    std::fs::write(&script, include_str!("../windows/uninstall.ps1")).map_err(|e| e.to_string())?;
    powershell(&script)
        .arg("-OldPid")
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
